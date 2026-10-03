//! Снимок состояния для окна (сериализуется в JSON).

use std::sync::Arc;

use serde::Serialize;

use crate::config::Settings;
use crate::engine::{Inner, lock};
use crate::model::{CloudUp, InState, OutState};
use crate::scan::native;
use crate::util::{is_executable, kind_of};

#[derive(Serialize, Clone)]
pub struct UiState {
    pub me: String,
    pub my_name: String,
    pub folder: String,
    pub onboarded: bool,
    pub peers: Vec<PeerView>,
    pub incoming: Vec<ItemView>,
    pub outgoing: Vec<ItemView>,
    pub preparing: Vec<PrepView>,
    pub settings: Settings,
    pub cloud: CloudView,
    pub version: &'static str,
    pub lang: &'static str,
    /// Пауза до этого времени (мс); 0 — нет паузы, i64::MAX — пока не продолжат.
    pub paused_until: i64,
    /// Скачанная и проверенная новая версия, готовая к установке.
    pub update: Option<String>,
    /// Сообщения (новые сначала).
    pub notes: Vec<NoteView>,
}

#[derive(Serialize, Clone)]
pub struct NoteView {
    pub id: String,
    /// Отправлено отсюда.
    pub outgoing: bool,
    /// Кто написал (для полученных).
    pub peer_id: String,
    pub peer: String,
    pub text: String,
    pub created_at: i64,
    pub seen: bool,
    /// Отправленное: кому и дошло ли.
    pub to: Vec<NoteTo>,
}

#[derive(Serialize, Clone)]
pub struct NoteTo {
    pub id: String,
    pub name: String,
    pub delivered: bool,
    /// У получателя старая версия программы — получит после обновления.
    pub needs_update: bool,
}

#[derive(Serialize, Clone)]
pub struct PeerView {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub route: String,
    /// Когда последний раз был на связи (мс), 0 — неизвестно.
    pub last_seen: i64,
    pub version: String,
    /// На устройстве программа старее этой — стоит обновить.
    pub outdated: bool,
    pub paused: bool,
}

#[derive(Serialize, Clone)]
pub struct PrepView {
    pub item: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Serialize, Clone)]
pub struct ItemView {
    pub id: String,
    pub peer: String,
    pub peer_id: String,
    /// Отправленные: общий номер для одного элемента, предложенного нескольким устройствам.
    pub batch: String,
    pub item: String,
    pub kind: &'static str,
    pub is_folder: bool,
    pub is_update: bool,
    pub files: usize,
    pub total: u64,
    pub done: u64,
    pub speed: f64,
    pub state: String,
    pub message: Option<String>,
    pub source: Option<String>,
    pub conflicts: Vec<String>,
    pub path: Option<String>,
    pub cloud: String,
    pub cloud_done: u64,
    pub has_exe: bool,
    pub auto: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Clone)]
pub struct CloudView {
    pub connected: bool,
    /// "yandex", "webdav" или "folder".
    pub provider: String,
    pub local: bool,
    pub url: String,
    pub user: String,
    pub login: String,
    pub uploading: bool,
    pub auth_code: Option<String>,
    pub auth_url: Option<String>,
    pub error: Option<String>,
    pub client_id: String,
    pub client_secret: String,
}

pub(crate) fn route_text(inner: &Inner, peer: &str) -> String {
    let Some(conn) = inner.conn(peer) else { return String::new() };
    let route = match conn.paths().iter().find(|p| p.is_selected()) {
        Some(p) if p.is_ip() => "route.direct",
        Some(p) if p.is_relay() => "route.relay",
        _ => "route.connecting",
    };
    crate::t!(route)
}

pub(crate) fn build(inner: &Arc<Inner>) -> UiState {
    let uploads = lock(&inner.uploads).clone();
    let preparing: Vec<PrepView> = lock(&inner.preparing)
        .iter()
        .map(|(k, (d, t))| PrepView { item: k.clone(), done: *d, total: *t })
        .collect();
    let rates: std::collections::HashMap<String, f64> =
        lock(&inner.rates).iter().map(|(k, r)| (k.clone(), r.bps)).collect();
    let peer_ids: Vec<(String, String, i64)> = {
        let s = inner.st();
        s.group
            .active()
            .filter(|m| m.id != inner.me)
            .map(|m| (m.id.clone(), m.name.clone(), s.last_seen.get(&m.id).copied().unwrap_or(0)))
            .collect()
    };
    let versions = lock(&inner.peer_versions).clone();
    let peer_paused = lock(&inner.peer_paused).clone();
    let peers: Vec<PeerView> = peer_ids
        .into_iter()
        .map(|(id, name, last_seen)| {
            let version = versions.get(&id).cloned().unwrap_or_default();
            PeerView {
                online: inner.is_online(&id),
                route: route_text(inner, &id),
                last_seen,
                paused: peer_paused.contains(&id),
                outdated: !version.is_empty() && crate::util::version_newer(crate::proto::APP_VERSION, &version),
                version,
                id,
                name,
            }
        })
        .collect();
    let cloud_error = lock(&inner.cloud_error).clone();
    let (auth_code, auth_url) = match &*lock(&inner.cloud_auth) {
        Some(a) => (Some(a.user_code.clone()), Some(a.url.clone())),
        None => (None, None),
    };

    let s = inner.st();
    let root = s.settings.folder.clone();
    let mut incoming: Vec<ItemView> = s
        .incoming
        .iter()
        .filter(|i| i.state != InState::Superseded)
        .map(|i| ItemView {
            id: i.offer.id.clone(),
            peer: s.group.name_of(&i.offer.from),
            peer_id: i.offer.from.clone(),
            batch: String::new(),
            item: i.offer.item.clone(),
            kind: kind_of(&i.offer.item, i.offer.is_folder),
            is_folder: i.offer.is_folder,
            is_update: i.offer.is_update(),
            files: i.offer.files.len(),
            total: i.offer.total_size(),
            done: i.done_bytes,
            speed: rates.get(&i.offer.id).copied().unwrap_or(0.0),
            state: format!("{:?}", i.state).to_lowercase(),
            message: i.message.clone(),
            source: i.source.clone(),
            conflicts: i.conflicts.clone(),
            path: i.saved.first().map(|r| native(&root, r).to_string_lossy().into_owned()),
            cloud: if i.offer.in_cloud { "uploaded".into() } else { "none".into() },
            cloud_done: 0,
            has_exe: i.offer.files.iter().any(|f| is_executable(&f.path)),
            auto: i.auto,
            created_at: i.offer.created_at,
            updated_at: i.updated_at,
        })
        .collect();
    let mut outgoing: Vec<ItemView> = s
        .outgoing
        .iter()
        .filter(|o| o.state != OutState::Superseded)
        .map(|o| {
            let up = uploads.get(&o.offer.id);
            ItemView {
                id: o.offer.id.clone(),
                peer: s.group.name_of(&o.to),
                peer_id: o.to.clone(),
                batch: if o.batch.is_empty() { o.offer.id.clone() } else { o.batch.clone() },
                item: o.offer.item.clone(),
                kind: kind_of(&o.offer.item, o.offer.is_folder),
                is_folder: o.offer.is_folder,
                is_update: o.offer.is_update(),
                files: o.offer.files.len(),
                total: o.offer.total_size(),
                done: o.progress,
                speed: if up.is_some() {
                    rates.get(&format!("cloud:{}", o.offer.id)).copied().unwrap_or(0.0)
                } else {
                    rates.get(&format!("out:{}", o.offer.id)).copied().unwrap_or(0.0)
                },
                state: format!("{:?}", o.state).to_lowercase(),
                message: match &o.cloud {
                    CloudUp::Failed(e) => Some(crate::t!("cloud.failed", reason = e)),
                    _ => None,
                },
                source: None,
                conflicts: vec![],
                path: Some(native(&root, &o.offer.item).to_string_lossy().into_owned()),
                cloud: match &o.cloud {
                    CloudUp::None => "none",
                    CloudUp::Uploading => "uploading",
                    CloudUp::Uploaded => "uploaded",
                    CloudUp::Failed(_) => "failed",
                }
                .into(),
                cloud_done: up.map(|u| u.0).unwrap_or(0),
                has_exe: o.offer.files.iter().any(|f| is_executable(&f.path)),
                auto: false,
                created_at: o.offer.created_at,
                updated_at: o.updated_at,
            }
        })
        .collect();
    incoming.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    outgoing.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let cloud = CloudView {
        connected: s.cloud.as_ref().is_some_and(|c| c.usable()),
        provider: s.cloud.as_ref().map(|c| c.kind().to_string()).unwrap_or_default(),
        local: s.cloud.as_ref().is_some_and(|c| c.is_local()),
        url: s.cloud.as_ref().map(|c| c.url.clone()).unwrap_or_default(),
        user: s.cloud.as_ref().map(|c| c.user.clone()).unwrap_or_default(),
        login: s.cloud.as_ref().map(|c| c.login.clone()).unwrap_or_default(),
        uploading: !uploads.is_empty(),
        auth_code,
        auth_url,
        error: cloud_error,
        client_id: s.cloud_client.as_ref().map(|c| c.0.clone()).unwrap_or_default(),
        client_secret: s.cloud_client.as_ref().map(|c| c.1.clone()).unwrap_or_default(),
    };
    let name_of = |id: &str| s.group.members.iter().find(|m| m.id == id).map(|m| m.name.clone()).unwrap_or_default();
    let mut notes: Vec<NoteView> = s
        .notes
        .iter()
        .map(|n| {
            let outgoing = n.from == inner.me;
            let to = n
                .delivered
                .iter()
                .map(|p| (p, true))
                .chain(n.pending.iter().map(|p| (p, false)))
                .map(|(p, delivered)| NoteTo {
                    id: p.clone(),
                    name: name_of(p),
                    delivered,
                    needs_update: !delivered && lock(&inner.peer_versions).get(p).is_some_and(|v| crate::util::version_newer(crate::notes::SINCE, v)),
                })
                .collect();
            NoteView {
                id: n.id.clone(),
                outgoing,
                peer_id: n.from.clone(),
                peer: if outgoing { String::new() } else { name_of(&n.from) },
                text: n.text.clone(),
                created_at: n.created_at,
                seen: n.seen,
                to,
            }
        })
        .collect();
    notes.reverse();
    UiState {
        me: inner.me.clone(),
        my_name: s.settings.device_name.clone(),
        folder: root.to_string_lossy().into_owned(),
        onboarded: s.settings.onboarded,
        peers,
        incoming,
        outgoing,
        preparing,
        settings: s.settings.clone(),
        cloud,
        version: crate::proto::APP_VERSION,
        lang: crate::i18n::lang(),
        paused_until: if inner.is_paused() { inner.paused_until.load(std::sync::atomic::Ordering::Relaxed) } else { 0 },
        update: lock(&inner.update_ready).as_ref().map(|u| u.0.clone()),
        notes,
    }
}

