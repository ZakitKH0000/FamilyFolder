//! Обновление программы внутри семьи, без своего сервера.
//!
//! Устройство, у которого есть установщик (его кладёт сам установщик — см. installer-hooks.nsh),
//! сообщает о нём в приветствии: версия, размер, BLAKE3 и подпись разработчика. Устройство со
//! старой версией скачивает установщик напрямую (`STREAM_UPDATE`, с докачкой) в `update\` своей
//! папки данных и сообщает окну (`Event::UpdateReady`). Подпись проверяет окно: двигатель ключа
//! не знает. После проверки скачанный установщик сам становится источником для остальных.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use iroh::endpoint::{RecvStream, SendStream};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::engine::{Event, Inner, lock};
use crate::proto::{APP_VERSION, STREAM_UPDATE, UpdateInfo, UpdateReq, read_msg, write_msg};
use crate::util::version_newer;

/// Установщик, который можно отдать другим.
#[derive(Clone)]
pub(crate) struct UpdatePkg {
    pub info: UpdateInfo,
    pub path: PathBuf,
}

pub(crate) fn dir(inner: &Inner) -> PathBuf {
    inner.data_dir.join("update")
}

/// Самая новая версия, которая здесь уже есть: установленная или скачанная.
fn have_version(inner: &Inner) -> String {
    let mut v = APP_VERSION.to_string();
    if let Some((ready, _)) = &*lock(&inner.update_ready)
        && version_newer(ready, &v)
    {
        v = ready.clone();
    }
    v
}

/// Пришло приветствие с установщиком — скачать, если он новее того, что есть.
pub(crate) fn offered(inner: &Arc<Inner>, peer: &str, info: UpdateInfo) {
    if inner.family_updates_off.load(std::sync::atomic::Ordering::Relaxed)
        || !version_newer(&info.version, &have_version(inner))
        || lock(&inner.update_rejected).contains(&info.hash)
        || !lock(&inner.update_downloading).insert(info.hash.clone())
    {
        return;
    }
    let (inner, peer) = (inner.clone(), peer.to_string());
    crate::engine::spawn(async move {
        let hash = info.hash.clone();
        match download(&inner, &peer, &info).await {
            Ok(path) => {
                tracing::info!("скачано обновление {} от {peer}", info.version);
                inner.emit(Event::UpdateReady { version: info.version.clone(), path, sig: info.sig.clone(), hash: hash.clone() });
            }
            Err(e) => tracing::warn!("обновление {}: {e:#}", info.version),
        }
        lock(&inner.update_downloading).remove(&hash);
    });
}

async fn download(inner: &Arc<Inner>, peer: &str, info: &UpdateInfo) -> Result<PathBuf> {
    let dir = dir(inner);
    tokio::fs::create_dir_all(&dir).await?;
    let file_name = format!("setup-{}.exe", crate::util::sanitize_name(&info.version));
    let path = dir.join(&file_name);
    let part = dir.join(format!("{file_name}.part"));
    let (offset, mut hasher) = crate::transfer::resume_point(&part, info.size, 1).await?;
    let Some(conn) = inner.conn(peer) else { bail!("устройство не в сети") };
    let (mut send, mut recv) = conn.open_bi().await?;
    send.write_u8(STREAM_UPDATE).await?;
    write_msg(&mut send, &UpdateReq { hash: info.hash.clone(), offset }).await?;
    let ok: bool = read_msg(&mut recv).await?;
    if !ok {
        bail!("у устройства больше нет этого установщика");
    }
    let mut file = tokio::fs::OpenOptions::new().append(true).open(&part).await?;
    let mut buf = vec![0u8; 1 << 20];
    let mut got = offset;
    while got < info.size {
        let Some(n) = recv.read(&mut buf).await? else { break };
        let n = n.min((info.size - got) as usize);
        file.write_all(&buf[..n]).await?;
        hasher.update(&buf[..n]);
        got += n as u64;
        inner.down_limit.take(n).await;
    }
    file.flush().await?;
    drop(file);
    if got < info.size {
        bail!("связь прервалась");
    }
    if hasher.finalize().to_hex().as_str() != info.hash {
        let _ = tokio::fs::remove_file(&part).await;
        bail!("установщик повредился при передаче");
    }
    tokio::fs::rename(&part, &path).await?;
    tokio::fs::write(dir.join(format!("{file_name}.sig")), &info.sig).await?;
    Ok(path)
}

/// Отдаёт свой установщик, если это тот самый (по сумме).
pub(crate) async fn serve(inner: &Arc<Inner>, mut send: SendStream, mut recv: RecvStream) -> Result<()> {
    let req: UpdateReq = read_msg(&mut recv).await?;
    let pkg = lock(&inner.update_pkg).clone().filter(|p| p.info.hash == req.hash && req.offset <= p.info.size);
    let Some(pkg) = pkg.filter(|_| !inner.is_paused()) else {
        write_msg(&mut send, &false).await?;
        send.finish()?;
        return Ok(());
    };
    write_msg(&mut send, &true).await?;
    let mut file = tokio::fs::File::open(&pkg.path).await?;
    file.seek(std::io::SeekFrom::Start(req.offset)).await?;
    let mut reader = file.take(pkg.info.size - req.offset);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        send.write_all(&buf[..n]).await?;
        inner.up_limit.take(n).await;
    }
    send.finish()?;
    let _ = tokio::time::timeout(Duration::from_secs(30), send.stopped()).await;
    Ok(())
}

/// Окно проверило подпись установщика: его можно отдавать другим (и ставить, если он новее).
pub(crate) async fn set_package(inner: &Arc<Inner>, version: &str, path: PathBuf, sig: &str) -> Result<()> {
    let p = path.clone();
    let (size, hash) = tokio::task::spawn_blocking(move || -> Result<(u64, String)> {
        let size = std::fs::metadata(&p)?.len();
        let hash = crate::scan::hash_file(&p, |_| {})?;
        Ok((size, hash))
    })
    .await??;
    let info = UpdateInfo { version: version.to_string(), size, hash, sig: sig.trim().to_string() };
    let newer = version_newer(version, APP_VERSION);
    {
        let mut pkg = lock(&inner.update_pkg);
        if pkg.as_ref().is_some_and(|p| version_newer(&p.info.version, version)) {
            return Ok(());
        }
        *pkg = Some(UpdatePkg { info, path: path.clone() });
    }
    tracing::info!("установщик {version} можно отдавать семье");
    if newer {
        *lock(&inner.update_ready) = Some((version.to_string(), path));
    }
    inner.broadcast_hello();
    inner.changed();
    Ok(())
}
