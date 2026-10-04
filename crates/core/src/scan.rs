//! Слежение за общей папкой: что появилось или изменилось → предложения устройствам семьи.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::engine::{Dirty, Inner, lock};
use crate::model::{IndexEntry, Offer, OfferFile, OutState, Outgoing, CloudUp};
use crate::sync::send_pending_offers;
use crate::util::{mtime_ms, now_ms, rel_path, set_hidden, top_item};

/// Новый файл предлагается через 3 с тишины, изменённый — через 20 с
/// (программы вроде Word сохраняют по нескольку раз подряд).
const QUIET_NEW: Duration = Duration::from_secs(3);
fn quiet_changed() -> Duration {
    match std::env::var("OBSHAYA_QUIET_CHANGED_MS") {
        Ok(v) => Duration::from_millis(v.parse().unwrap_or(20_000)),
        Err(_) => Duration::from_secs(20),
    }
}

pub(crate) fn start(inner: &Arc<Inner>) {
    restart_watcher(inner);
    crate::engine::spawn(processor(inner.clone()));
    let inner = inner.clone();
    crate::engine::spawn(async move {
        scan_all(&inner).await;
        // Подстраховка на случай пропущенных событий.
        loop {
            tokio::time::sleep(Duration::from_secs(600)).await;
            if inner.closing.load(Ordering::Acquire) {
                break;
            }
            scan_all(&inner).await;
        }
    });
}

pub(crate) fn restart_watcher(inner: &Arc<Inner>) {
    let root = inner.root();
    if let Err(e) = std::fs::create_dir_all(&root) {
        tracing::error!("не удалось создать общую папку {}: {e}", root.display());
        return;
    }
    let staging = root.join(".obshaya");
    let _ = std::fs::create_dir_all(&staging);
    set_hidden(&staging);

    let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    });
    let mut watcher = match watcher {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("слежение за папкой не запустилось: {e}");
            return;
        }
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        tracing::error!("слежение за папкой не запустилось: {e}");
        return;
    }
    *lock(&inner.watcher) = Some(watcher);

    let inner = inner.clone();
    crate::engine::spawn(async move {
        while let Some(path) = rx.recv().await {
            let Some(rel) = rel_path(&root, &path) else { continue };
            if rel.split('/').any(is_ignored) {
                continue;
            }
            mark_dirty(&inner, top_item(&rel), None);
        }
    });
}

pub(crate) fn is_ignored(name: &str) -> bool {
    let n = name.to_lowercase();
    n == ".obshaya"
        || n == "desktop.ini"
        || n == "thumbs.db"
        || n == ".ds_store"
        || n.starts_with("~$")
        || n.starts_with(".~lock")
        || n.ends_with(".tmp")
        || n.ends_with(".part")
        || n.ends_with(".crdownload")
        || n.ends_with(".partial")
}

fn mark_dirty(inner: &Arc<Inner>, item: &str, quiet: Option<Duration>) {
    let quiet = quiet.unwrap_or_else(|| {
        let s = inner.st();
        let prefix = format!("{item}/");
        let known = s.index.contains_key(item) || s.index.range(prefix.clone()..).next().is_some_and(|(k, _)| k.starts_with(&prefix));
        if known { quiet_changed() } else { QUIET_NEW }
    });
    lock(&inner.dirty).insert(item.to_string(), Dirty { last: Instant::now(), quiet });
}

async fn processor(inner: Arc<Inner>) {
    let busy = Arc::new(tokio::sync::Semaphore::new(2));
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if inner.closing.load(Ordering::Acquire) {
            break;
        }
        let ready: Vec<String> = {
            let mut dirty = lock(&inner.dirty);
            let scanning = lock(&inner.scanning);
            let ready: Vec<String> = dirty
                .iter()
                .filter(|(k, d)| d.last.elapsed() >= d.quiet && !scanning.contains(*k))
                .map(|(k, _)| k.clone())
                .collect();
            for k in &ready {
                dirty.remove(k);
            }
            ready
        };
        for item in ready {
            lock(&inner.scanning).insert(item.clone());
            let inner = inner.clone();
            let busy = busy.clone();
            crate::engine::spawn(async move {
                let _permit = busy.acquire().await;
                if let Err(e) = scan_item(&inner, &item, true).await {
                    tracing::warn!("не удалось обработать «{item}»: {e:#}");
                }
                lock(&inner.scanning).remove(&item);
            });
        }
    }
}

/// Полный проход по папке (при запуске и раз в 10 минут). При смене папки — без предложений.
pub(crate) async fn scan_all(inner: &Arc<Inner>) {
    let root = inner.root();
    let baseline = {
        let mut s = inner.st();
        let root_str = root.to_string_lossy().into_owned();
        if s.index_root != root_str {
            if !s.index_root.is_empty() {
                s.file_access.clear();
                s.history_shares.clear();
            }
            s.index.clear();
            s.index_root = root_str;
            true
        } else {
            false
        }
    };
    let mut items: HashSet<String> = HashSet::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !is_ignored(&name) {
                items.insert(name);
            }
        }
    }
    items.extend(inner.st().index.keys().map(|k| top_item(k).to_string()));
    for item in items {
        if lock(&inner.scanning).contains(&item) {
            continue;
        }
        lock(&inner.scanning).insert(item.clone());
        if let Err(e) = scan_item(inner, &item, !baseline).await {
            tracing::warn!("не удалось обработать «{item}»: {e:#}");
        }
        lock(&inner.scanning).remove(&item);
    }
    inner.save_soon();
    inner.changed();
}

/// Проверяет один элемент верхнего уровня и предлагает изменения.
pub(crate) async fn scan_item(inner: &Arc<Inner>, item: &str, make_offers: bool) -> Result<()> {
    let root = inner.root();
    let item_path = root.join(item);
    let listing = {
        let root = root.clone();
        let item_path = item_path.clone();
        tokio::task::spawn_blocking(move || walk(&root, &item_path)).await?
    };
    let is_folder = item_path.is_dir();
    let present: HashSet<&str> = listing.iter().map(|(r, _, _)| r.as_str()).collect();

    let candidates: Vec<(String, u64, i64)> = {
        let mut s = inner.st();
        let prefix = format!("{item}/");
        s.index
            .retain(|k, _| !((k == item || k.starts_with(&prefix)) && !present.contains(k.as_str())));
        listing
            .iter()
            .filter(|(rel, size, mtime)| {
                s.index.get(rel).is_none_or(|e| e.size != *size || e.mtime != *mtime)
            })
            .cloned()
            .collect()
    };
    if candidates.is_empty() {
        return Ok(());
    }
    for (rel, _, _) in &candidates {
        if is_busy(&native(&root, rel)) {
            mark_dirty(inner, item, Some(Duration::from_secs(5)));
            return Ok(());
        }
    }

    let total: u64 = candidates.iter().map(|c| c.1).sum();
    lock(&inner.preparing).insert(item.to_string(), (0, total));
    inner.changed();
    let mut changes = Vec::new();
    let mut base = 0u64;
    let result: Result<()> = async {
        for (rel, size, mtime) in &candidates {
            let path = native(&root, rel);
            let hash = {
                let inner = inner.clone();
                let item = item.to_string();
                let path = path.clone();
                tokio::task::spawn_blocking(move || {
                    hash_file(&path, |done| {
                        if let Some(p) = lock(&inner.preparing).get_mut(&item) {
                            p.0 = base + done;
                        }
                        inner.changed();
                    })
                })
                .await??
            };
            base += size;
            let meta = std::fs::metadata(&path)?;
            if meta.len() != *size || mtime_ms(&meta) != *mtime {
                anyhow::bail!(crate::t!("err.file_busy"));
            }
            let prev = {
                let mut s = inner.st();
                let prev = s.index.get(rel).map(|e| e.hash.clone());
                let audience = s.group.active().map(|m| m.id.clone()).collect();
                let access = s.file_access.entry(rel.clone()).or_insert_with(|| crate::model::FileAccess {
                    owner: inner.me.clone(), audience,
                }).clone();
                s.index.insert(rel.clone(), IndexEntry { size: *size, mtime: *mtime, hash: hash.clone() });
                (prev, access)
            };
            if prev.0.as_deref() != Some(hash.as_str()) {
                changes.push(OfferFile { path: rel.clone(), size: *size, mtime: *mtime, hash,
                    prev_hash: prev.0, owner: prev.1.owner, audience: prev.1.audience });
            }
        }
        Ok(())
    }
    .await;
    lock(&inner.preparing).remove(item);
    inner.save_soon();
    inner.changed();
    if let Err(e) = result {
        tracing::info!("«{item}» ещё меняется, проверю позже: {e:#}");
        mark_dirty(inner, item, Some(Duration::from_secs(5)));
        return Ok(());
    }
    if make_offers && !changes.is_empty() {
        create_offers(inner, item, is_folder, changes, None);
    }
    Ok(())
}

/// Создаёт предложения всем (или одному) устройствам. Если прошлое предложение
/// по этому же элементу ещё не забрали — новое заменяет его, объединяя файлы.
pub(crate) fn create_offers(
    inner: &Arc<Inner>,
    item: &str,
    is_folder: bool,
    changes: Vec<OfferFile>,
    only: Option<&str>,
) {
    // Бросили в шторке на конкретное устройство — предложить только ему.
    let targeted = if only.is_none() {
        lock(&inner.targets).remove(item).filter(|(_, until)| *until > Instant::now()).map(|(p, _)| p)
    } else {
        None
    };
    let peers: Vec<String> = {
        let s = inner.st();
        s.group
            .active()
            .filter(|m| m.id != inner.me && only.is_none_or(|o| o == m.id))
            .filter(|m| targeted.as_ref().is_none_or(|t| t.contains(&m.id)))
            .map(|m| m.id.clone())
            .collect()
    };
    if peers.is_empty() {
        return;
    }
    let batch = uuid::Uuid::new_v4().to_string();
    {
        let mut s = inner.st();
        for peer in &peers {
            let mut files: Vec<OfferFile> = changes.iter().filter_map(|f| {
                let access = s.file_access.get_mut(&f.path)?;
                // Перетаскивание — явное действие автора. Новая копия идёт выбранным людям.
                if let Some(targets) = &targeted {
                    if access.owner == inner.me {
                        if f.prev_hash.is_none() { access.audience = vec![inner.me.clone()]; }
                        for id in targets {
                            if !access.audience.contains(id) { access.audience.push(id.clone()); }
                        }
                    }
                }
                if !access.audience.contains(peer) { return None; }
                let mut f = f.clone();
                f.owner = access.owner.clone();
                f.audience = access.audience.clone();
                Some(f)
            }).collect();
            if files.is_empty() { continue; }
            let mut supersedes = Vec::new();
            for o in s.outgoing.iter_mut().filter(|o| {
                &o.to == peer
                    && o.offer.item == item
                    && matches!(o.state, OutState::Pending | OutState::Offered)
            }) {
                for old in &o.offer.files {
                    match files.iter_mut().find(|f| f.path == old.path) {
                        Some(f) => f.prev_hash = old.prev_hash.clone(),
                        None => files.push(old.clone()),
                    }
                }
                o.state = OutState::Superseded;
                o.updated_at = now_ms();
                supersedes.push(o.offer.id.clone());
            }
            files.retain(|f| f.prev_hash.as_ref() != Some(&f.hash));
            if files.is_empty() {
                continue;
            }
            files.sort_by(|a, b| a.path.cmp(&b.path));
            s.outgoing.push(Outgoing {
                offer: Offer {
                    id: uuid::Uuid::new_v4().to_string(),
                    from: inner.me.clone(),
                    item: item.to_string(),
                    is_folder,
                    files,
                    created_at: now_ms(),
                    supersedes,
                    in_cloud: false,
                },
                to: peer.clone(),
                state: OutState::Pending,
                progress: 0,
                cloud: CloudUp::None,
                updated_at: now_ms(),
                batch: batch.clone(),
            });
        }
    }
    inner.save_soon();
    for peer in &peers {
        send_pending_offers(inner, peer);
    }
    inner.kick_cloud();
    inner.changed();
}

pub(crate) fn native(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/') {
        p.push(part);
    }
    p
}

/// Все файлы элемента: (путь, размер, время изменения).
pub(crate) fn walk(root: &Path, item: &Path) -> Vec<(String, u64, i64)> {
    let mut out = Vec::new();
    let mut stack = vec![item.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    if !is_ignored(&e.file_name().to_string_lossy()) {
                        stack.push(e.path());
                    }
                }
            }
        } else if let Some(rel) = rel_path(root, &p) {
            out.push((rel, meta.len(), mtime_ms(&meta)));
        }
    }
    out
}

/// Файл сейчас кто-то записывает (копирование ещё идёт)?
fn is_busy(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    match std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path) {
        Ok(_) => false,
        Err(e) => e.raw_os_error() == Some(32),
    }
}

pub(crate) fn hash_file(path: &Path, mut progress: impl FnMut(u64)) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut last = Instant::now();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        if last.elapsed() > Duration::from_millis(200) {
            progress(done);
            last = Instant::now();
        }
    }
    Ok(hasher.finalize().to_hex().to_string())
}
