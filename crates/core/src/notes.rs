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
const KEEP: usize = 300;

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
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NoteMsg {
    pub id: String,
    pub text: String,
    pub created_at: i64,
}

/// Понимает ли устройство сообщения (по версии из приветствия).
pub(crate) fn supports(inner: &Inner, peer: &str) -> bool {
    lock(&inner.peer_versions).get(peer).is_some_and(|v| !version_newer(SINCE, v))
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_LEN).collect()
}

fn trim(notes: &mut Vec<Note>) {
    if notes.len() > KEEP {
        let extra = notes.len() - KEEP;
        notes.drain(..extra);
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
        });
        trim(&mut s.notes);
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
    let waiting: Vec<NoteMsg> = {
        let s = inner.st();
        s.notes
            .iter()
            .filter(|n| n.from == inner.me && n.pending.iter().any(|p| p == peer))
            .map(|n| NoteMsg { id: n.id.clone(), text: n.text.clone(), created_at: n.created_at })
            .collect()
    };
    for note in waiting {
        inner.send(peer, Msg::Note { note });
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
        });
        trim(&mut s.notes);
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
