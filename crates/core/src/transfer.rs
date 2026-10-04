//! Скачивание принятых предложений (напрямую или из облака), отдача своих файлов
//! и раскладка полученного по папке с учётом версий и конфликтов.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use iroh::endpoint::{RecvStream, SendStream};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::engine::{Event, Inner, lock};
use crate::model::{InState, Offer, OfferFile};
use crate::proto::{FileReq, FileResp, STREAM_FILE, read_msg, write_msg};
use crate::scan::native;
use crate::util::{is_executable, mark_from_internet, mtime_ms, now_ms, rel_path, safe_join, system_time, unique_path};

const BUF: usize = 1 << 20;
const MAX_PARALLEL: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Peer,
    Cloud,
}

pub(crate) enum Fetched {
    Ok,
    Changed,
}

/// Ошибка загрузки: повторять автоматически или ждать пользователя.
pub(crate) struct DlError {
    pub retry: bool,
    pub msg: String,
}

impl<E: Into<anyhow::Error>> From<E> for DlError {
    fn from(e: E) -> Self {
        let e: anyhow::Error = e.into();
        DlError { retry: true, msg: format!("{e:#}") }
    }
}

pub(crate) async fn scheduler(inner: Arc<Inner>) {
    let mut was_paused = inner.is_paused();
    loop {
        tokio::select! {
            _ = inner.kick.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(3)) => {}
        }
        if inner.closing.load(Ordering::Acquire) {
            break;
        }
        let paused = inner.is_paused();
        if was_paused && !paused {
            // Пауза закончилась сама — сообщить остальным и продолжить облако.
            resumed(&inner);
        }
        was_paused = paused;
        start_downloads(&inner);
    }
}

/// После паузы: сообщить устройствам семьи и продолжить всё, что ждало.
pub(crate) fn resumed(inner: &Arc<Inner>) {
    inner.broadcast_hello();
    inner.kick.notify_one();
    inner.kick_cloud();
    inner.changed();
}

/// Показать «На паузе» у ждущих загрузок.
fn mark_paused(inner: &Arc<Inner>) {
    let msg = crate::t!("in.paused");
    let mut changed = false;
    {
        let mut s = inner.st();
        for i in s.incoming.iter_mut().filter(|i| i.state == InState::Queued) {
            if i.message.as_deref() != Some(msg.as_str()) {
                i.message = Some(msg.clone());
                changed = true;
            }
        }
    }
    if changed {
        inner.changed();
    }
}

fn start_downloads(inner: &Arc<Inner>) {
    let candidates: Vec<(String, String, bool)> = {
        let s = inner.st();
        s.incoming
            .iter()
            .filter(|i| matches!(i.state, InState::Queued | InState::Downloading))
            .map(|i| (i.offer.id.clone(), i.offer.from.clone(), i.offer.in_cloud))
            .collect()
    };
    if inner.is_paused() {
        mark_paused(inner);
        return;
    }
    let cloud_ready = crate::cloud::configured(inner);
    for (id, from, in_cloud) in candidates {
        if lock(&inner.downloads).contains(&id) {
            continue;
        }
        if lock(&inner.downloads).len() >= MAX_PARALLEL {
            break;
        }
        let peer_paused = lock(&inner.peer_paused).contains(&from);
        let source = if inner.is_online(&from) && !peer_paused {
            Source::Peer
        } else if in_cloud && cloud_ready {
            Source::Cloud
        } else {
            let mut s = inner.st();
            let name = s.group.name_of(&from);
            if let Some(i) = s.incoming.iter_mut().find(|i| i.offer.id == id) {
                let msg = if peer_paused {
                    crate::t!("in.sender_paused", name = name)
                } else {
                    crate::t!("in.waiting_peer", name = name)
                };
                if i.message.as_deref() != Some(msg.as_str()) || i.state != InState::Queued {
                    i.state = InState::Queued;
                    i.message = Some(msg);
                    drop(s);
                    inner.changed();
                }
            }
            continue;
        };
        lock(&inner.downloads).insert(id.clone());
        crate::engine::spawn(run_download(inner.clone(), id, source));
    }
}

async fn run_download(inner: Arc<Inner>, id: String, source: Source) {
    let result = download(&inner, &id, source).await;
    lock(&inner.downloads).remove(&id);
    lock(&inner.rates).remove(&id);
    let cancelled = lock(&inner.cancelled).contains(&id);
    if let Err(e) = result
        && !cancelled
    {
        let paused = inner.is_paused();
        if !paused {
            tracing::warn!("загрузка {id}: {}", e.msg);
        }
        {
            let mut s = inner.st();
            if let Some(i) = s.incoming.iter_mut().find(|i| i.offer.id == id)
                && !i.state.is_final()
            {
                i.state = if e.retry { InState::Queued } else { InState::Failed };
                i.message = Some(if paused {
                    crate::t!("in.paused")
                } else if e.retry {
                    crate::t!("in.interrupted", reason = e.msg)
                } else {
                    e.msg.clone()
                });
                i.updated_at = now_ms();
            }
        }
        inner.save_soon();
        inner.send_status(&id);
        inner.changed();
        if e.retry && !paused {
            // Небольшая пауза, чтобы не долбить сразу после обрыва.
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    inner.kick.notify_one();
}

async fn download(inner: &Arc<Inner>, id: &str, source: Source) -> Result<(), DlError> {
    let (offer, mut done_files) = {
        let mut s = inner.st();
        let i = s.incoming.iter_mut().find(|i| i.offer.id == id).ok_or_else(|| anyhow!(crate::t!("err.offer_not_found")))?;
        i.state = InState::Downloading;
        i.message = None;
        i.updated_at = now_ms();
        (i.offer.clone(), i.done_files.clone())
    };
    let staging = inner.staging().join("in").join(id);
    std::fs::create_dir_all(&staging).context(crate::t!("err.staging"))?;
    set_source(inner, id, source, &offer.from);
    inner.send_status(id);
    inner.changed();

    let total = offer.total_size();
    let mut base: u64 = done_files.iter().filter_map(|&i| offer.files.get(i)).map(|f| f.size).sum();
    for (idx, f) in offer.files.iter().enumerate() {
        if done_files.contains(&idx) {
            continue;
        }
        if lock(&inner.cancelled).contains(id) {
            return Err(DlError { retry: false, msg: crate::t!("err.cancelled") });
        }
        let part = staging.join(format!("{idx}.part"));
        let fetched = match source {
            Source::Peer => fetch_peer(inner, &offer, idx, f, &part, base, total).await?,
            Source::Cloud => crate::cloud::fetch(inner, &offer, idx, f, &part, base, total).await?,
        };
        if let Fetched::Changed = fetched {
            {
                let mut s = inner.st();
                if let Some(i) = s.incoming.iter_mut().find(|i| i.offer.id == id) {
                    i.state = InState::Unavailable;
                    i.message = Some(crate::t!("in.unavailable"));
                    i.updated_at = now_ms();
                }
            }
            let _ = std::fs::remove_dir_all(&staging);
            inner.save_soon();
            inner.send_status(id);
            inner.changed();
            return Ok(());
        }
        done_files.push(idx);
        base += f.size;
        {
            let mut s = inner.st();
            if let Some(i) = s.incoming.iter_mut().find(|i| i.offer.id == id) {
                i.done_files = done_files.clone();
                i.done_bytes = base;
            }
        }
        inner.save_soon();
    }

    let applied = {
        let inner = inner.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || apply(&inner, &id)).await.map_err(anyhow::Error::from)?
    };
    let (saved, conflicts) = applied.map_err(|msg| DlError { retry: false, msg })?;
    let first = saved.first().map(|r| native(&inner.root(), r));
    {
        let mut s = inner.st();
        if let Some(i) = s.incoming.iter_mut().find(|i| i.offer.id == id) {
            i.state = InState::Done;
            i.done_bytes = total;
            i.saved = saved;
            i.conflicts = conflicts.clone();
            i.message = None;
            i.updated_at = now_ms();
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    inner.save_soon();
    inner.send_status(id);
    inner.emit(Event::Received {
        id: id.to_string(),
        title: offer.item.clone(),
        path: first.unwrap_or_else(|| inner.root()),
        conflicts: conflicts.len(),
    });
    inner.kick_cloud();
    inner.changed();
    Ok(())
}

fn set_source(inner: &Arc<Inner>, id: &str, source: Source, from: &str) {
    let text = match source {
        Source::Cloud => crate::t!("source.cloud"),
        Source::Peer => crate::view::route_text(inner, from),
    };
    let mut s = inner.st();
    if let Some(i) = s.incoming.iter_mut().find(|i| i.offer.id == id) {
        i.source = Some(text);
    }
}

/// Обновляет прогресс входящего предложения (в памяти; на диск — по завершении файлов).
pub(crate) fn progress(inner: &Arc<Inner>, id: &str, done: u64) {
    {
        let mut s = inner.st();
        if let Some(i) = s.incoming.iter_mut().find(|i| i.offer.id == id) {
            i.done_bytes = done;
        }
    }
    inner.update_rate(id, done);
    inner.send_status(id);
    inner.changed();
}

/// Начало докачки: сколько уже скачано и сумма этой части.
pub(crate) async fn resume_point(part: &Path, size: u64, align: u64) -> Result<(u64, blake3::Hasher)> {
    let mut offset = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
    if offset > size {
        offset = 0;
    }
    if align > 1 {
        offset -= offset % align;
    }
    let file = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(part)?;
    file.set_len(offset)?;
    drop(file);
    let part = part.to_path_buf();
    let hasher = tokio::task::spawn_blocking(move || -> Result<blake3::Hasher> {
        use std::io::Read;
        let mut hasher = blake3::Hasher::new();
        let mut f = std::fs::File::open(&part)?.take(offset);
        let mut buf = vec![0u8; BUF];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(hasher)
    })
    .await??;
    Ok((offset, hasher))
}

async fn fetch_peer(
    inner: &Arc<Inner>,
    offer: &Offer,
    idx: usize,
    f: &OfferFile,
    part: &Path,
    base: u64,
    _total: u64,
) -> Result<Fetched, DlError> {
    let conn = inner
        .conn(&offer.from)
        .ok_or_else(|| DlError { retry: true, msg: crate::t!("err.sender_offline") })?;
    let (offset, mut hasher) = resume_point(part, f.size, 1).await?;
    let (mut send, mut recv) = conn.open_bi().await.context(crate::t!("err.transfer_start"))?;
    send.write_u8(STREAM_FILE).await?;
    write_msg(&mut send, &FileReq { offer_id: offer.id.clone(), index: idx, offset }).await?;
    let resp: FileResp = read_msg(&mut recv).await?;
    match resp {
        FileResp::Changed => return Ok(Fetched::Changed),
        FileResp::Busy => return Err(DlError { retry: true, msg: crate::t!("err.sender_paused") }),
        FileResp::Ok => {}
    }
    let mut file = tokio::fs::OpenOptions::new().append(true).open(part).await?;
    let mut buf = vec![0u8; BUF];
    let mut got = offset;
    let mut since_report = 0usize;
    while got < f.size {
        let n = match recv.read(&mut buf).await {
            Ok(Some(n)) => n,
            Ok(None) => break,
            Err(e) => {
                file.flush().await.ok();
                return Err(DlError { retry: true, msg: crate::t!("err.connection_lost_reason", reason = e) });
            }
        };
        let n = n.min((f.size - got) as usize);
        file.write_all(&buf[..n]).await?;
        hasher.update(&buf[..n]);
        got += n as u64;
        inner.down_limit.take(n).await;
        since_report += n;
        if since_report >= 512 * 1024 || got == f.size {
            since_report = 0;
            progress(inner, &offer.id, base + got);
            if lock(&inner.cancelled).contains(&offer.id) {
                return Err(DlError { retry: false, msg: crate::t!("err.cancelled") });
            }
            if inner.is_paused() {
                file.flush().await.ok();
                return Err(DlError { retry: true, msg: crate::t!("in.paused") });
            }
        }
    }
    file.flush().await?;
    drop(file);
    if got < f.size {
        return Err(DlError { retry: true, msg: crate::t!("err.connection_lost") });
    }
    if hasher.finalize().to_hex().as_str() != f.hash {
        let _ = std::fs::remove_file(part);
        return Err(DlError { retry: true, msg: crate::t!("err.corrupt_retry") });
    }
    Ok(Fetched::Ok)
}

/// Отдаёт файл устройству `peer`, если он есть в предложении для него и не изменился.
pub(crate) async fn serve_file(inner: &Arc<Inner>, peer: &str, mut send: SendStream, mut recv: RecvStream) -> Result<()> {
    let req: FileReq = read_msg(&mut recv).await?;
    if inner.is_paused() {
        write_msg(&mut send, &FileResp::Busy).await?;
        send.finish()?;
        return Ok(());
    }
    let root = inner.root();
    let found = {
        let s = inner.st();
        s.outgoing
            .iter()
            .find(|o| o.offer.id == req.offer_id && o.to == peer)
            .and_then(|o| o.offer.files.get(req.index).cloned())
            .filter(|f| s.index.get(&f.path).is_some_and(|e| e.hash == f.hash))
            .map(|f| {
                let e = s.index.get(&f.path).cloned().unwrap();
                (f, e)
            })
    };
    let Some((f, entry)) = found else {
        write_msg(&mut send, &FileResp::Changed).await?;
        send.finish()?;
        return Ok(());
    };
    let path = native(&root, &f.path);
    let unchanged = std::fs::metadata(&path).is_ok_and(|m| m.len() == entry.size && mtime_ms(&m) == entry.mtime);
    if !unchanged || req.offset > f.size {
        write_msg(&mut send, &FileResp::Changed).await?;
        send.finish()?;
        return Ok(());
    }
    write_msg(&mut send, &FileResp::Ok).await?;
    let mut file = tokio::fs::File::open(&path).await?;
    file.seek(SeekFrom::Start(req.offset)).await?;
    let mut reader = file.take(f.size - req.offset);
    let mut buf = vec![0u8; BUF];
    let key = format!("out:{}", req.offer_id);
    let mut sent = req.offset;
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 || inner.is_paused() {
            break;
        }
        send.write_all(&buf[..n]).await?;
        inner.up_limit.take(n).await;
        sent += n as u64;
        inner.update_rate(&key, sent);
    }
    send.finish()?;
    // Ждём, пока получатель дочитает, иначе закрытие потока может оборвать хвост.
    let _ = tokio::time::timeout(Duration::from_secs(30), send.stopped()).await;
    lock(&inner.rates).remove(&key);
    Ok(())
}

/// Раскладывает скачанные файлы. Возвращает (сохранённые пути, конфликтные копии).
fn apply(inner: &Arc<Inner>, id: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let (offer, peer_name, root) = {
        let s = inner.st();
        let i = s.incoming.iter().find(|i| i.offer.id == id).ok_or_else(|| crate::t!("err.offer_not_found"))?;
        (i.offer.clone(), s.group.name_of(&i.offer.from), s.settings.folder.clone())
    };
    let staging = root.join(".obshaya").join("in").join(id);
    let mut saved = Vec::new();
    let mut conflicts = Vec::new();
    for (idx, f) in offer.files.iter().enumerate() {
        let part = staging.join(format!("{idx}.part"));
        let Some(mut dest) = safe_join(&root, &f.path) else { continue };
        if !part.exists() {
            // Уже разложен при прошлой попытке.
            if dest.exists() {
                saved.push(rel_path(&root, &dest).unwrap_or_default());
            }
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::t!("err.create_folder_reason", reason = e))?;
        }
        if dest.exists() {
            let local = local_hash(inner, &root, &dest).map_err(|e| crate::t!("err.read_file", name = f.path, reason = e))?;
            if local == f.hash {
                let _ = std::fs::remove_file(&part);
                saved.push(rel_path(&root, &dest).unwrap_or_default());
                continue;
            } else if f.prev_hash.as_deref() == Some(local.as_str()) {
                to_trash(&root, &dest).map_err(|e| busy_msg(&dest, e))?;
            } else {
                dest = unique_path(&dest, &crate::t!("conflict.suffix", name = peer_name));
                conflicts.push(rel_path(&root, &dest).unwrap_or_default());
            }
        }
        std::fs::rename(&part, &dest).map_err(|e| busy_msg(&dest, e))?;
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&dest) {
            let _ = file.set_modified(system_time(f.mtime));
        }
        if is_executable(&f.path) {
            mark_from_internet(&dest);
        }
        let meta = std::fs::metadata(&dest).map_err(|e| e.to_string())?;
        let rel = rel_path(&root, &dest).unwrap_or_default();
        let mut state = inner.st();
        let previous = state.file_access.get(&rel).cloned();
        let owner = if f.owner.is_empty() { offer.from.clone() } else { f.owner.clone() };
        let mut audience = if f.audience.is_empty() { vec![offer.from.clone(), inner.me.clone()] }
            else { f.audience.clone() };
        if !audience.contains(&inner.me) { audience.push(inner.me.clone()); }
        // Старые программы и получатели не меняют разрешения, установленные автором.
        state.file_access.insert(rel.clone(), if f.owner.is_empty() || offer.from != owner {
            previous.unwrap_or(crate::model::FileAccess { owner, audience })
        } else { crate::model::FileAccess { owner, audience } });
        state.index.insert(
            rel.clone(),
            crate::model::IndexEntry { size: meta.len(), mtime: mtime_ms(&meta), hash: f.hash.clone() },
        );
        saved.push(rel);
    }
    Ok((saved, conflicts))
}

fn busy_msg(path: &Path, e: impl std::fmt::Display) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    crate::t!("err.replace_busy", name = name, reason = e)
}

fn local_hash(inner: &Arc<Inner>, root: &Path, path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    if let Some(rel) = rel_path(root, path) {
        let s = inner.st();
        if let Some(e) = s.index.get(&rel)
            && e.size == meta.len()
            && e.mtime == mtime_ms(&meta)
        {
            return Ok(e.hash.clone());
        }
    }
    crate::scan::hash_file(path, |_| {})
}

/// Старая версия — в Корзину (в тестах — в служебную папку).
fn to_trash(root: &Path, path: &Path) -> Result<()> {
    if std::env::var_os("OBSHAYA_NO_TRASH").is_some() {
        let dir = root.join(".obshaya").join("old");
        std::fs::create_dir_all(&dir)?;
        let name = path.file_name().unwrap_or_default();
        let target: PathBuf = unique_path(&dir.join(name), "");
        std::fs::rename(path, target)?;
        return Ok(());
    }
    trash::delete(path).map_err(|e| anyhow!("{e}"))
}
