//! Адресная облачная очередь: DH ключей устройств, подпись автора, отдельный ACK.
use std::{sync::{Arc, atomic::Ordering}, time::Duration};
use anyhow::{Result, Context, ensure};
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer};
use serde::{Serialize, Deserialize};
use crate::{cloud::{self, Backend}, engine::{Inner, Event}, notes::{self, ChatMsg}, crypto, voice, util::now_ms};

const MAX_META: usize = 160_000;
const MAX_PACKET: usize = voice::MAX_SIZE as usize + MAX_META + 128;
#[derive(Serialize, Deserialize)]
struct Mail { from: String, to: String, note: Option<ChatMsg>, ack: Option<String> }

fn public(peer: &str) -> Result<VerifyingKey> {
    let id: iroh::PublicKey = peer.parse()?;
    ensure!(id.to_string() == peer, "noncanonical peer");
    let p = VerifyingKey::from_bytes(id.as_bytes())?;
    ensure!(!p.is_weak(), "weak peer key");
    Ok(p)
}
fn pair_key(secret: &SigningKey, peer: &str) -> Result<[u8; 32]> {
    let shared = (public(peer)?.to_montgomery() * secret.to_scalar()).to_bytes();
    ensure!(shared != [0; 32], "weak shared key");
    Ok(blake3::derive_key("Family Folder chat cloud v1", &shared))
}
fn signed(path: &str, raw: &[u8]) -> Vec<u8> {
    let mut data = b"Family Folder chat cloud v1\0".to_vec();
    data.extend_from_slice(path.as_bytes()); data.push(0); data.extend_from_slice(raw); data
}
fn pack(secret: &SigningKey, path: &str, m: &Mail, audio: &[u8]) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(m)?;
    ensure!(json.len() <= MAX_META && audio.len() <= voice::MAX_SIZE as usize, "mail too large");
    let mut raw = (json.len() as u32).to_le_bytes().to_vec();
    raw.extend(json); raw.extend_from_slice(audio);
    let sig = secret.sign(&signed(path, &raw));
    let mut bytes = sig.to_bytes().to_vec(); bytes.extend(raw);
    Ok(crypto::seal(&pair_key(secret, &m.to)?, path, &bytes))
}
fn unpack(secret: &SigningKey, path: &str, from: &str, data: &[u8]) -> Result<(Mail, Vec<u8>)> {
    ensure!(data.len() <= MAX_PACKET, "mail too large");
    let bytes = crypto::open(&pair_key(secret, from)?, path, data)?;
    ensure!(bytes.len() >= 68, "short mail");
    public(from)?.verify_strict(&signed(path, &bytes[64..]), &Signature::from_slice(&bytes[..64])?)?;
    let size = u32::from_le_bytes(bytes[64..68].try_into()?) as usize;
    ensure!(size <= MAX_META && size <= bytes.len() - 68, "invalid metadata");
    let m: Mail = serde_json::from_slice(&bytes[68..68 + size])?;
    ensure!(m.from == from && m.to == iroh::SecretKey::from_bytes(&secret.to_bytes()).public().to_string(), "wrong recipient");
    ensure!(m.note.is_some() != m.ack.is_some(), "invalid mail kind");
    Ok((m, bytes[68 + size..].to_vec()))
}
fn secret(inner: &Inner) -> SigningKey { SigningKey::from_bytes(&inner.endpoint.secret_key().to_bytes()) }
fn root(inner: &Inner) -> String { format!("chat-v1/{}", crypto::content_key(&inner.group_key(), "mail")) }
fn path(inner: &Inner, kind: &str, to: &str, from: &str, id: &str) -> String {
    format!("{}/{kind}/{to}/{from}_{id}.bin", root(inner))
}
pub(crate) fn start(inner: Arc<Inner>) {
    crate::engine::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if inner.closing.load(Ordering::Acquire) { break; }
            if inner.is_paused() { continue; }
            let Some(b) = cloud::backend(&inner) else { continue };
            struct Busy<'a>(&'a std::sync::atomic::AtomicBool);
            impl Drop for Busy<'_> { fn drop(&mut self) {self.0.store(false,Ordering::Relaxed);} }
            inner.chat_cloud_busy.store(true,Ordering::Relaxed);
            let _busy=Busy(&inner.chat_cloud_busy);
            if let Err(e) = tokio::time::timeout(Duration::from_secs(90), cycle(&inner, &b)).await {
                tracing::debug!("chat cloud timeout: {e}");
            }
        }
    });
}
async fn cycle(inner: &Arc<Inner>, b: &Backend) {
    // Сначала входящие и ACK: повреждённый объект не блокирует остальные письма.
    for kind in ["mail", "ack"] {
        let dir = format!("{}/{kind}/{}", root(inner), inner.me);
        let Ok(names) = b.list(&dir).await else { continue };
        for name in names.into_iter().take(500) {
            if inner.is_paused() || inner.closing.load(Ordering::Relaxed) { return; }
            let Some((from, id)) = name.strip_suffix(".bin").and_then(|s| s.split_once('_')) else { continue };
            if !voice::valid_id(id) || public(from).is_err() || !inner.st().group.is_member(from) { continue; }
            let p = format!("{dir}/{name}");
            if let Err(e) = receive(inner, b, kind, from, id, &p).await {
                tracing::debug!("chat cloud object: {e:#}");
            }
        }
    }
    if inner.st().settings.cloud_mode == crate::config::CloudMode::Never { return; }
    let waiting: Vec<_> = inner.st().notes.iter().filter(|n| n.from == inner.me && n.group.is_some())
        .filter(|n| !n.pending.is_empty()).cloned().take(100).collect();
    for n in waiting {
        for to in &n.pending {
            if inner.is_paused() || inner.closing.load(Ordering::Relaxed) { return; }
            if n.cloud_ready.contains(to) || !inner.st().group.is_member(to) { continue; }
            if inner.is_online(to) && now_ms() - n.created_at < 30_000
                && inner.st().settings.cloud_mode != crate::config::CloudMode::Always { continue; }
            let p = path(inner, "mail", to, &inner.me, &n.id);
            let result: Result<()> = async {
                let audio = if n.voice.is_some() { tokio::fs::read(voice::path(inner, &n.id)?).await? } else { vec![] };
                let m = Mail { from: inner.me.clone(), to: to.clone(), ack: None, note: Some(ChatMsg {
                    id: n.id.clone(), text: n.text.clone(), created_at: n.created_at, group: n.group.unwrap(),
                    voice: n.voice.clone(), reply_to: n.reply_to.clone() }) };
                b.put(&inner.cloud_dirs, &p, pack(&secret(inner), &p, &m, &audio)?).await?;
                // Маркер готовности только после завершения PUT всего текста и аудио.
                Ok(())
            }.await;
            {
                let mut s = inner.st();
                if let Some(note) = s.notes.iter_mut().find(|x| x.id == n.id) {
                    match result {
                        Ok(()) => { if !note.cloud_ready.contains(to) { note.cloud_ready.push(to.clone()); } note.cloud_error = None; }
                        Err(e) => note.cloud_error = Some(format!("{e:#}")),
                    }
                }
            }
            let _ = inner.save_checked(); inner.changed();
        }
        // ACK удаляет только свою копию; после доставки всем у отправителя остаётся история.
    }
}
async fn receive(inner: &Arc<Inner>, b: &Backend, kind: &str, from: &str, id: &str, p: &str) -> Result<()> {
    let receipt=inner.st().chat_receipts.get(id).cloned();
    if kind == "mail" && let Some(author)=receipt { ensure!(author==from,"conflicting receipt author"); return Ok(()); }
    if kind == "mail" && inner.st().notes.iter().any(|n|n.id==id && n.from==from && n.cloud_ready.iter().any(|p|p==from)
        && (n.voice.is_none() || voice::path(inner,id).is_ok_and(|p|p.is_file()))) { return Ok(()); }
    let bytes = b.get_bounded(p, if kind == "ack" { MAX_META } else { MAX_PACKET }).await?.context("missing mail")?;
    let (m, audio) = unpack(&secret(inner), p, from, &bytes)?;
    ensure!(!inner.is_paused() && inner.st().group.is_member(from), "peer removed or paused");
    if kind == "ack" {
        ensure!(m.ack.as_deref() == Some(id) && audio.is_empty(), "wrong ack");
        notes::acked(inner, from, id); inner.save_checked()?;
        // Сначала удалить письмо: даже после падения его повторная обработка безопасна.
        b.delete(&path(inner, "mail", from, &inner.me, id)).await?;
        b.delete(p).await?; return Ok(());
    }
    ensure!(inner.st().chat_receipts.len()<100_000, "cloud receipt limit reached");
    let note = m.note.context("missing chat")?;
    ensure!(note.id == id && note.text.chars().count() <= notes::MAX_LEN
        && note.reply_to.as_ref().is_none_or(|r| voice::valid_id(r))
        && (!note.text.trim().is_empty() || note.voice.is_some()), "invalid chat");
    if let Some(v) = &note.voice {
        ensure!(v.valid() && v.size == audio.len() as u64 && blake3::hash(&audio).to_hex().as_str() == v.hash, "invalid voice");
    } else { ensure!(audio.is_empty(), "unexpected voice"); }
    let existing = inner.st().notes.iter().find(|n| n.id == id).cloned();
    if let Some(n) = &existing {
        ensure!(n.from == from && n.text == note.text && n.group == Some(note.group)
            && n.voice == note.voice && n.reply_to == note.reply_to, "conflicting message");
    }
    let had_audio = voice::path(inner, id)?.is_file();
    // received_chat сохраняет метаданные; подтверждаем только после долговечной записи и аудио.
    notes::received_chat(inner, from, note.clone());
    ensure!(inner.st().notes.iter().any(|n| n.id == id && n.from == from), "chat rejected");
    inner.save_checked()?;
    if note.voice.is_some() && !had_audio {
        let dest = voice::path(inner, id)?;
        std::fs::create_dir_all(dest.parent().unwrap())?;
        crate::util::write_atomic(&dest, &audio)?;
        inner.emit(Event::Note { id: id.into(), from: inner.st().group.name_of(from), text: crate::t!("chat.voice") });
        inner.changed();
    }
    let ack_path = path(inner, "ack", from, &inner.me, id);
    let ack = Mail { from: inner.me.clone(), to: from.into(), note: None, ack: Some(id.into()) };
    b.put(&inner.cloud_dirs, &ack_path, pack(&secret(inner), &ack_path, &ack, &[])?).await?;
    if let Some(n) = inner.st().notes.iter_mut().find(|n|n.id==id) { if !n.cloud_ready.iter().any(|p|p==from) {n.cloud_ready.push(from.into());} }
    inner.st().chat_receipts.insert(id.into(),from.into());
    inner.save_checked()?;
    // Письмо хранится до возвращения автора и обработки ACK: потерянный PUT ACK повторится.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_and_signed_mail() {
        let a = SigningKey::from_bytes(&[1;32]); let b = SigningKey::from_bytes(&[2;32]); let c = SigningKey::from_bytes(&[3;32]);
        let id = |s: &SigningKey| iroh::SecretKey::from_bytes(&s.to_bytes()).public().to_string();
        let m = Mail {from:id(&a), to:id(&b), note:None, ack:Some(uuid::Uuid::new_v4().to_string())};
        let packet = pack(&a, "test/mail", &m, &[]).unwrap();
        assert!(unpack(&b, "test/mail", &id(&a), &packet).is_ok());
        assert!(unpack(&c, "test/mail", &id(&a), &packet).is_err());
        assert!(unpack(&b, "other/mail", &id(&a), &packet).is_err());
        assert!(unpack(&b, "test/mail", &id(&c), &packet).is_err());
        let mut changed = packet; changed[45] ^= 1;
        assert!(unpack(&b, "test/mail", &id(&a), &changed).is_err());
    }
}
