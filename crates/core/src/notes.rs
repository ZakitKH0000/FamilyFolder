//! Сообщения внутри семьи: текст, ссылка, адрес — чтобы у себя скопировать или открыть.
//!
//! Идут напрямую по управляющему каналу (`Msg::Note`); получатель подтверждает (`Msg::NoteAck`),
//! пока подтверждения нет — сообщение ждёт и уходит снова при следующей связи.
//! Устройства до 1.4.0 таких сообщений не знают (связь бы порвалась) — им отправим после обновления.

use std::sync::Arc;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::engine::{Event, Inner, lock};
use crate::proto::Msg;
use crate::util::{now_ms, version_newer};

/// Самое длинное сообщение (в символах).
pub const MAX_LEN: usize = 20_000;
/// С какой версии программа понимает сообщения.
pub const SINCE: &str = "1.4.0";
/// Сколько сообщений хранить (старые уходят).
const KEEP: usize = 2000;
pub const CHAT_SINCE: &str = "1.5.0";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Note {
    pub id: String,
    /// Кто написал (код устройства).
    pub from: String,
    pub text: String,
    pub created_at: i64,
    /// Отправленное: кому ещё не доставлено и кому уже.
    #[serde(default)]
    pub pending: Vec<String>,
    #[serde(default)]
    pub delivered: Vec<String>,
    /// Полученное: человек его уже видел.
    #[serde(default)]
    pub seen: bool,
    /// None — сообщение из версии до появления отдельных разговоров.
    #[serde(default)]
    pub group: Option<bool>,
    #[serde(default)]
    pub voice: Option<crate::voice::Voice>,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub cloud_ready: Vec<String>,
    #[serde(default)]
    pub cloud_error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoteMsg {
    pub id: String,
    pub text: String,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMsg {
    pub id: String,
    pub text: String,
    pub created_at: i64,
    pub group: bool,
    pub voice: Option<crate::voice::Voice>,
    #[serde(default)]
    pub reply_to: Option<String>,
}

/// Понимает ли устройство сообщения (по версии из приветствия).
pub(crate) fn supports(inner: &Inner, peer: &str) -> bool {
    lock(&inner.peer_versions).get(peer).is_some_and(|v| !version_newer(SINCE, v))
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_LEN).collect()
}

fn trim(notes: &mut Vec<Note>, data_dir: &std::path::Path) {
    if notes.len() > KEEP {
        let mut extra = notes.len() - KEEP;
        notes.retain(|n| {
            if extra > 0 && n.pending.is_empty() && (n.seen || n.from.is_empty()) {
                extra -= 1;
                if n.voice.is_some() {
                    if let Ok(path) = crate::voice::path_in(data_dir, &n.id) {
                        let _ = std::fs::remove_file(&path);
                        let _ = std::fs::remove_file(path.with_extension("part"));
                    }
                }
                false
            } else { true }
        });
    }
}

/// Написать семье: `peers` пусто — всем, иначе только им.
pub(crate) fn send(inner: &Arc<Inner>, text: &str, peers: &[String]) -> Result<()> {
    let text = clip(text.trim());
    if text.is_empty() {
        bail!(crate::t!("note.empty"));
    }
    let targets: Vec<String> = {
        let s = inner.st();
        s.group
            .active()
            .filter(|m| m.id != inner.me && (peers.is_empty() || peers.contains(&m.id)))
            .map(|m| m.id.clone())
            .collect()
    };
    if targets.is_empty() {
        bail!(crate::t!("peer.none"));
    }
    {
        let mut s = inner.st();
        s.notes.push(Note {
            id: uuid::Uuid::new_v4().to_string(),
            from: inner.me.clone(),
            text,
            created_at: now_ms(),
            pending: targets.clone(),
            delivered: Vec::new(),
            seen: true,
            group: None,
            voice: None,
            reply_to: None, cloud_ready: vec![], cloud_error: None,
        });
        trim(&mut s.notes, &inner.data_dir);
    }
    inner.save_soon();
    for peer in &targets {
        flush(inner, peer);
    }
    inner.changed();
    Ok(())
}

/// Отправить устройству всё, что для него ждёт (после его приветствия — тогда известна версия).
pub(crate) fn flush(inner: &Arc<Inner>, peer: &str) {
    if !supports(inner, peer) {
        return;
    }
    let waiting: Vec<Note> = {
        let s = inner.st();
        s.notes
            .iter()
            .filter(|n| n.from == inner.me && n.pending.iter().any(|p| p == peer))
            .cloned()
            .collect()
    };
    for note in waiting {
        if let Some(group) = note.group {
            if lock(&inner.peer_versions).get(peer).is_some_and(|v| !version_newer(CHAT_SINCE, v)) {
                inner.send(peer, Msg::Chat { note: ChatMsg { id: note.id, text: note.text,
                    created_at: note.created_at, group, voice: note.voice, reply_to: note.reply_to } });
            }
        } else {
            inner.send(peer, Msg::Note { note: NoteMsg { id: note.id, text: note.text, created_at: note.created_at } });
        }
    }
}

pub(crate) fn received(inner: &Arc<Inner>, peer: &str, m: NoteMsg) {
    // Подтверждаем всегда, даже повтор, — иначе отправитель будет слать снова.
    inner.send(peer, Msg::NoteAck { id: m.id.clone() });
    let from = {
        let mut s = inner.st();
        let Some(name) = s.group.active().find(|x| x.id == peer).map(|x| x.name.clone()) else { return };
        if s.notes.iter().any(|n| n.id == m.id) {
            return;
        }
        s.notes.push(Note {
            id: m.id.clone(),
            from: peer.to_string(),
            text: clip(&m.text),
            created_at: m.created_at.min(now_ms()),
            pending: Vec::new(),
            delivered: Vec::new(),
            seen: false,
            group: None,
            voice: None,
            reply_to: None, cloud_ready: vec![], cloud_error: None,
        });
        trim(&mut s.notes, &inner.data_dir);
        name
    };
    inner.save_soon();
    inner.emit(Event::Note { id: m.id, from, text: clip(&m.text) });
    inner.changed();
}

pub(crate) fn acked(inner: &Arc<Inner>, peer: &str, id: &str) {
    let changed = {
        let mut s = inner.st();
        match s.notes.iter_mut().find(|n| n.id == id && n.from == inner.me) {
            Some(n) if n.pending.iter().any(|p| p == peer) => {
                n.pending.retain(|p| p != peer);
                n.delivered.push(peer.to_string());
                if n.pending.is_empty() { n.cloud_error=None; }
                true
            }
            _ => false,
        }
    };
    if changed {
        inner.save_soon();
        inner.changed();
    }
}

pub(crate) fn send_chat(inner: &Arc<Inner>, text: &str, peer: Option<&str>,
    voice: Option<(crate::voice::Voice, &[u8])>, reply_to: Option<&str>) -> Result<()> {
    let text = clip(text.trim());
    anyhow::ensure!(!text.is_empty() || voice.is_some(), crate::t!("note.empty"));
    let targets: Vec<String> = {
        let s = inner.st();
        if let Some(peer) = peer {
            anyhow::ensure!(peer != inner.me && s.group.is_member(peer), crate::t!("share.gone"));
        }
        if let Some(id) = reply_to {
            anyhow::ensure!(crate::voice::valid_id(id) && s.notes.iter().any(|n| n.id == id && {
                let group = n.group.unwrap_or(n.from != inner.me || n.pending.len() + n.delivered.len() != 1);
                match peer {
                    None => group,
                    Some(p) => !group && (n.from == p || n.from == inner.me && n.pending.iter().chain(&n.delivered).any(|to| to == p)),
                }
            }), crate::t!("chat.reply_missing"));
        }
        anyhow::ensure!(s.notes.len() < 10_000, crate::t!("chat.full"));
        s.group.active().filter(|m| m.id != inner.me && peer.is_none_or(|p| p == m.id))
            .map(|m| m.id.clone()).collect()
    };
    anyhow::ensure!(!targets.is_empty(), crate::t!("peer.none"));
    let id = uuid::Uuid::new_v4().to_string();
    let voice = match voice {
        Some((meta, data)) => {
            let path = crate::voice::path(inner, &id)?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            crate::util::write_atomic(&path, data)?;
            Some(meta)
        }
        None => None,
    };
    {
        let mut s = inner.st();
        s.notes.push(Note { id: id.clone(), from: inner.me.clone(), text, created_at: now_ms(),
            pending: targets.clone(), delivered: vec![], seen: true, group: Some(peer.is_none()), voice, reply_to: reply_to.map(str::to_owned), cloud_ready: vec![], cloud_error: None });
        trim(&mut s.notes, &inner.data_dir);
    }
    if let Err(e) = inner.save_checked() {
        inner.st().notes.retain(|n| n.id != id);
        if let Ok(path) = crate::voice::path(inner, &id) { let _ = std::fs::remove_file(path); }
        return Err(e);
    }
    for peer in targets { flush(inner, &peer); }
    inner.changed();
    Ok(())
}

pub(crate) fn received_chat(inner: &Arc<Inner>, peer: &str, m: ChatMsg) {
    if !inner.st().group.is_member(peer) || !crate::voice::valid_id(&m.id)
        || m.text.chars().count() > MAX_LEN
        || (m.text.trim().is_empty() && m.voice.is_none())
        || m.reply_to.as_ref().is_some_and(|id| !crate::voice::valid_id(id))
        || m.voice.as_ref().is_some_and(|v| !v.valid()) { return; }
    let receipt=inner.st().chat_receipts.get(&m.id).cloned();
    if let Some(author)=receipt {
        if author==peer { inner.send(peer, Msg::NoteAck { id: m.id }); }
        return;
    }
    let (fresh, ready, name) = {
        let mut s = inner.st();
        let name = s.group.name_of(peer);
        if let Some(old) = s.notes.iter().find(|n| n.id == m.id) {
            if old.from != peer || old.group != Some(m.group) || old.text != m.text || old.voice != m.voice || old.reply_to != m.reply_to { return; }
            (false, old.voice.is_none() || crate::voice::path(inner, &m.id).is_ok_and(|p| p.is_file()), name)
        } else {
            if s.notes.len() >= 10_000 { return; }
            let ready = m.voice.is_none();
            s.notes.push(Note { id: m.id.clone(), from: peer.into(), text: m.text.clone(),
                created_at: m.created_at.min(now_ms()), pending: vec![], delivered: vec![],
                seen: false, group: Some(m.group), voice: m.voice.clone(), reply_to: m.reply_to.clone(), cloud_ready: vec![], cloud_error: None });
            trim(&mut s.notes, &inner.data_dir);
            (true, ready, name)
        }
    };
    if inner.save_checked().is_err() { inner.save_soon(); return; }
    if ready { inner.send(peer, Msg::NoteAck { id: m.id.clone() }); }
    if fresh && m.voice.is_none() {
        inner.emit(Event::Note { id: m.id, from: name, text: m.text });
    }
    inner.changed();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_keeps_unread_and_pending_but_removes_old_audio() {
        let dir = tempfile::tempdir().unwrap();
        let note = || Note { id: uuid::Uuid::new_v4().to_string(), from: "peer".into(), text: "hi".into(),
            created_at: 0, pending: vec![], delivered: vec![], seen: false, group: Some(false), voice: None, reply_to: None, cloud_ready: vec![], cloud_error: None };
        let mut old = note(); old.seen = true;
        old.voice = Some(crate::voice::Voice { size: 3, hash: blake3::hash(b"abc").to_hex().to_string(),
            duration_ms: 1000, mime: "audio/webm".into(), waveform: vec![] });
        let audio = crate::voice::path_in(dir.path(), &old.id).unwrap();
        std::fs::create_dir_all(audio.parent().unwrap()).unwrap();
        std::fs::write(&audio, b"abc").unwrap();
        std::fs::write(audio.with_extension("part"), b"a").unwrap();
        let mut pending = note(); pending.seen = true; pending.pending.push("offline".into());
        let pending_id = pending.id.clone();
        let unread = note(); let unread_id = unread.id.clone();
        let mut notes = vec![old, pending, unread];
        notes.extend((0..KEEP).map(|_| note()));
        trim(&mut notes, dir.path());
        assert!(!audio.exists() && !audio.with_extension("part").exists());
        assert!(notes.iter().any(|n| n.id == pending_id));
        assert!(notes.iter().any(|n| n.id == unread_id));
        assert_eq!(notes.len(), KEEP + 2);
    }
}
