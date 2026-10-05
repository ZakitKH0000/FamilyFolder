//! Голос чата: отдельный ограниченный QUIC-поток, никогда не путь в общей папке.
use std::path::PathBuf;
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use iroh::endpoint::{SendStream, RecvStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncSeekExt};
use crate::engine::{Inner, Event, lock};
use crate::proto::{Msg, STREAM_VOICE, read_bounded_msg, write_msg};

pub const MAX_SIZE: u64 = 8 * 1024 * 1024;
pub const MAX_DURATION: u64 = 300_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Voice { pub size: u64, pub hash: String, pub duration_ms: u64, pub mime: String, #[serde(default)] pub waveform: Vec<u8> }
impl Voice {
    pub fn valid(&self) -> bool {
        self.waveform.len() <= 64 && self.size > 0 && self.size <= MAX_SIZE && self.duration_ms >= 300 && self.duration_ms <= MAX_DURATION
            && self.hash.len() == 64 && self.hash.bytes().all(|c| c.is_ascii_hexdigit())
            && matches!(self.mime.as_str(), "audio/webm" | "audio/ogg" | "audio/mp4")
    }
}
#[derive(Serialize, Deserialize)]
struct Request { id: String, offset: u64 }
pub(crate) fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.to_string() == id)
}
pub(crate) fn path(inner: &Inner, id: &str) -> Result<PathBuf> {
    path_in(&inner.data_dir, id)
}
pub(crate) fn path_in(data_dir: &std::path::Path, id: &str) -> Result<PathBuf> {
    anyhow::ensure!(valid_id(id), "invalid voice id");
    Ok(data_dir.join("chat").join("voice").join(format!("{id}.audio")))
}
pub(crate) fn send(inner: &Arc<Inner>, data: &[u8], mime: &str, duration_ms: u64, peer: Option<&str>, waveform: &[u8], reply_to: Option<&str>) -> Result<()> {
    let meta = Voice { size: data.len() as u64, hash: blake3::hash(data).to_hex().to_string(), duration_ms, mime: mime.into(), waveform: waveform.to_vec() };
    anyhow::ensure!(meta.valid(), crate::t!("chat.voice_limit"));
    crate::notes::send_chat(inner, "", peer, Some((meta, data)), reply_to)
}
pub(crate) fn bytes(inner: &Inner, id: &str) -> Result<Vec<u8>> {
    let meta = inner.st().notes.iter().find(|n| n.id == id).and_then(|n| n.voice.clone()).context("voice not found")?;
    anyhow::ensure!(meta.valid(), "invalid voice metadata");
    let path = path(inner, id)?;
    anyhow::ensure!(std::fs::metadata(&path)?.len() == meta.size, "invalid voice size");
    let data = std::fs::read(path)?;
    anyhow::ensure!(blake3::hash(&data).to_hex().as_str() == meta.hash, "invalid voice hash");
    Ok(data)
}
pub(crate) async fn serve(inner: &Arc<Inner>, peer: &str, mut send: SendStream, mut recv: RecvStream) -> Result<()> {
    struct Serving<'a>(&'a Inner);
    impl Drop for Serving<'_> {
        fn drop(&mut self) { self.0.voice_serving.fetch_sub(1, Ordering::Relaxed); }
    }
    inner.voice_serving.fetch_add(1, Ordering::Relaxed);
    let _serving = Serving(inner);
    let req: Request = tokio::time::timeout(Duration::from_secs(15), read_bounded_msg(&mut recv, 1024)).await??;
    let meta = {
        let s = inner.st();
        s.notes.iter().find(|n| n.id == req.id && n.from == inner.me && s.group.is_member(peer)
            && (n.pending.iter().any(|p| p == peer) || n.delivered.iter().any(|p| p == peer)))
            .and_then(|n| n.voice.clone()).filter(|v| v.valid() && req.offset <= v.size)
    };
    let Some(meta) = meta.filter(|_| !inner.is_paused()) else {
        write_msg(&mut send, &false).await?; send.finish()?; return Ok(());
    };
    let mut file = tokio::fs::File::open(path(inner, &req.id)?).await?;
    anyhow::ensure!(file.metadata().await?.len() == meta.size, "voice changed");
    write_msg(&mut send, &true).await?;
    file.seek(std::io::SeekFrom::Start(req.offset)).await?;
    let mut remaining = meta.size - req.offset;
    let mut buf = vec![0; 64 * 1024];
    while remaining > 0 {
        anyhow::ensure!(!inner.is_paused() && inner.st().group.is_member(peer), "voice paused or removed");
        let limit = remaining.min(buf.len() as u64) as usize;
        let n = file.read(&mut buf[..limit]).await?;
        anyhow::ensure!(n > 0, "voice truncated");
        tokio::time::timeout(Duration::from_secs(30), send.write_all(&buf[..n])).await??;
        inner.up_limit.take(n).await;
        remaining -= n as u64;
    }
    send.finish()?;
    let _ = tokio::time::timeout(Duration::from_secs(30), send.stopped()).await;
    Ok(())
}
async fn fetch(inner: &Arc<Inner>, id: &str, peer: &str, meta: &Voice) -> Result<()> {
    let path = path(inner, id)?;
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    anyhow::ensure!(crate::util::free_space(&inner.data_dir).is_none_or(|free| free > meta.size + 200 * 1024 * 1024), "not enough space for voice");
    let part = path.with_extension("part");
    let (offset, mut hash) = crate::transfer::resume_point(&part, meta.size, 1).await?;
    let conn = inner.conn(peer).context("peer offline")?;
    let (mut send, mut recv) = conn.open_bi().await?;
    send.write_u8(STREAM_VOICE).await?;
    write_msg(&mut send, &Request { id: id.into(), offset }).await?;
    send.finish()?;
    let ok: bool = tokio::time::timeout(Duration::from_secs(15), read_bounded_msg(&mut recv, 32)).await??;
    anyhow::ensure!(ok, "voice unavailable");
    let mut file = tokio::fs::OpenOptions::new().append(true).open(&part).await?;
    let mut got = offset;
    let mut buf = vec![0; 64 * 1024];
    while got < meta.size {
        anyhow::ensure!(!inner.is_paused() && wanted(inner, id, peer, meta)
            && !inner.closing.load(Ordering::Relaxed), "voice paused or removed");
        let limit = (meta.size - got).min(buf.len() as u64) as usize;
        let n = tokio::time::timeout(Duration::from_secs(30), recv.read(&mut buf[..limit])).await??.context("voice interrupted")?;
        file.write_all(&buf[..n]).await?;
        hash.update(&buf[..n]); got += n as u64;
        inner.down_limit.take(n).await;
    }
    file.flush().await?; file.sync_all().await?; drop(file);
    if hash.finalize().to_hex().as_str() != meta.hash {
        let _ = tokio::fs::remove_file(part).await;
        anyhow::bail!("voice hash mismatch");
    }
    anyhow::ensure!(wanted(inner, id, peer, meta), "voice removed");
    inner.save_checked()?;
    tokio::fs::rename(part, path).await?;
    inner.send(peer, Msg::NoteAck { id: id.into() });
    let name = inner.st().group.name_of(peer);
    inner.emit(Event::Note { id: id.into(), from: name, text: crate::t!("chat.voice") });
    inner.changed();
    Ok(())
}
fn wanted(inner: &Inner, id: &str, peer: &str, meta: &Voice) -> bool {
    let s = inner.st();
    s.group.is_member(peer) && s.notes.iter().any(|n| n.id == id && n.voice.as_ref() == Some(meta))
}
pub(crate) async fn scheduler(inner: Arc<Inner>) {
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        if inner.closing.load(Ordering::Acquire) { break; }
        if inner.is_paused() { continue; }
        let waiting: Vec<_> = {
            let s = inner.st();
            s.notes.iter().filter(|n| n.from != inner.me && s.group.is_member(&n.from))
                .filter_map(|n| n.voice.as_ref().filter(|v| v.valid()).map(|v| (n.id.clone(), n.from.clone(), v.clone())))
                .filter(|(id, _, _)| path(&inner, id).is_ok_and(|p| !p.is_file())).collect()
        };
        for (id, peer, meta) in waiting {
            if !inner.is_online(&peer) { continue; }
            let mut running = lock(&inner.voice_downloads);
            if running.len() >= 2 { break; }
            if !running.insert(id.clone()) { continue; }
            drop(running);
            let inner = inner.clone();
            crate::engine::spawn(async move {
                if let Err(e) = fetch(&inner, &id, &peer, &meta).await {
                    tracing::debug!("голосовое сообщение: {e:#}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                lock(&inner.voice_downloads).remove(&id);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_ids_and_metadata_are_bounded() {
        assert!(!valid_id("../../outside"));
        assert!(!valid_id("C:\\outside"));
        assert!(valid_id(&uuid::Uuid::new_v4().to_string()));
        let mut v = Voice { size: 10, hash: "a".repeat(64), duration_ms: 1000, mime: "audio/webm".into(), waveform: vec![] };
        assert!(v.valid()); v.size = MAX_SIZE + 1; assert!(!v.valid());
        v.size = 10; v.duration_ms = MAX_DURATION + 1; assert!(!v.valid());
        v.duration_ms = 1000; v.waveform = vec![255; 65]; assert!(!v.valid());
        v.waveform = vec![255; 48]; assert!(v.valid());
        v.mime = "text/html".into(); assert!(!v.valid());
    }
}
