//! Соединения с устройствами семьи и обмен служебными сообщениями.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::Result;
use iroh::EndpointId;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::config::{AUTO_ACCEPT_RESERVE, AutoAccept};
use crate::engine::{Event, Inner, PeerConn, lock};
use crate::model::{InState, Incoming, Offer, OutState, Status};
use crate::proto::{Msg, PAIR_ALPN, STREAM_CONTROL, STREAM_FILE, STREAM_UPDATE, SYNC_ALPN, read_msg, write_msg};
use crate::util::{fmt_size, now_ms};
use crate::{pairing, transfer};

pub(crate) async fn accept_loop(inner: Arc<Inner>) {
    while let Some(incoming) = inner.endpoint.accept().await {
        let inner = inner.clone();
        crate::engine::spawn(async move {
            let conn = match incoming.await {
                Ok(c) => c,
                Err(e) => {
                    tracing::debug!("входящее соединение не удалось: {e:#}");
                    return;
                }
            };
            let alpn = conn.alpn().to_vec();
            if alpn == PAIR_ALPN {
                if let Err(e) = pairing::serve(&inner, conn).await {
                    tracing::warn!("приглашение не удалось: {e:#}");
                }
            } else if alpn == SYNC_ALPN {
                let peer = conn.remote_id().to_string();
                if !inner.st().group.is_member(&peer) {
                    tracing::info!("отклонено соединение от чужого устройства {peer}");
                    conn.close(1u32.into(), b"not a member");
                    return;
                }
                register(&inner, conn, peer.clone(), peer).await;
            }
        });
    }
}

/// Раз в несколько секунд подключается к тем устройствам семьи, с которыми нет связи.
pub(crate) async fn dial_loop(inner: Arc<Inner>) {
    loop {
        tokio::select! {
            _ = inner.dial_now.notified() => {
                lock(&inner.dial_backoff).clear();
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
        if inner.closing.load(Ordering::Acquire) {
            break;
        }
        let peers: Vec<String> = {
            let s = inner.st();
            s.group.active().filter(|m| m.id != inner.me).map(|m| m.id.clone()).collect()
        };
        for peer in peers {
            if inner.is_online(&peer) || lock(&inner.dialing).contains(&peer) {
                continue;
            }
            if lock(&inner.dial_backoff)
                .get(&peer)
                .is_some_and(|(at, _)| Instant::now() < *at)
            {
                continue;
            }
            lock(&inner.dialing).insert(peer.clone());
            crate::engine::spawn(dial(inner.clone(), peer));
        }
    }
}

async fn dial(inner: Arc<Inner>, peer: String) {
    let result: Result<Connection> = async {
        let id: EndpointId = peer.parse()?;
        Ok(tokio::time::timeout(Duration::from_secs(30), inner.endpoint.connect(id, SYNC_ALPN)).await??)
    }
    .await;
    lock(&inner.dialing).remove(&peer);
    match result {
        Ok(conn) => {
            lock(&inner.dial_backoff).remove(&peer);
            register(&inner, conn, peer, inner.me.clone()).await;
        }
        Err(e) => {
            tracing::debug!("нет связи с {peer}: {e:#}");
            let mut b = lock(&inner.dial_backoff);
            let delay = b
                .get(&peer)
                .map(|(_, d)| (*d * 2).min(Duration::from_secs(60)))
                .unwrap_or(Duration::from_secs(10));
            b.insert(peer, (Instant::now() + delay, delay));
        }
    }
}

/// Запоминает соединение. Если их два (оба позвонили одновременно), оставляет то,
/// которое начал компьютер с меньшим кодом — обе стороны выбирают одинаково.
async fn register(inner: &Arc<Inner>, conn: Connection, peer: String, initiator: String) {
    let serial = inner.serial.fetch_add(1, Ordering::Relaxed);
    let preferred = if inner.me < peer { inner.me.clone() } else { peer.clone() };
    let (tx, rx) = mpsc::unbounded_channel();
    {
        let mut conns = lock(&inner.conns);
        if let Some(old) = conns.get(&peer)
            && old.conn.close_reason().is_none()
        {
            if old.initiator == preferred && initiator != preferred {
                conn.close(0u32.into(), b"duplicate");
                return;
            }
            old.conn.close(0u32.into(), b"replaced");
        }
        conns.insert(
            peer.clone(),
            PeerConn { conn: conn.clone(), serial, initiator: initiator.clone(), ctrl: tx },
        );
    }
    tracing::info!("связь с {peer} установлена");
    inner.st().last_seen.insert(peer.clone(), now_ms());

    let mut ctrl_rx = Some(rx);
    if initiator == inner.me {
        match conn.open_bi().await {
            Ok((mut send, recv)) => {
                if send.write_u8(STREAM_CONTROL).await.is_ok() {
                    crate::engine::spawn(control_writer(send, ctrl_rx.take().unwrap()));
                    crate::engine::spawn(control_reader(inner.clone(), peer.clone(), recv));
                }
            }
            Err(e) => {
                tracing::warn!("не удалось открыть служебный канал: {e:#}");
                unregister(inner, &peer, serial);
                return;
            }
        }
    }
    on_connected(inner, &peer);
    crate::engine::spawn(accept_streams(inner.clone(), conn.clone(), peer.clone(), ctrl_rx));

    let inner2 = inner.clone();
    crate::engine::spawn(async move {
        conn.closed().await;
        unregister(&inner2, &peer, serial);
    });
    inner.changed();
    inner.kick.notify_one();
}

fn unregister(inner: &Arc<Inner>, peer: &str, serial: u64) {
    let mut conns = lock(&inner.conns);
    if conns.get(peer).is_some_and(|c| c.serial == serial) {
        conns.remove(peer);
        tracing::info!("связь с {peer} потеряна");
        drop(conns);
        inner.st().last_seen.insert(peer.to_string(), now_ms());
        inner.save_soon();
    } else {
        drop(conns);
    }
    inner.changed();
}

async fn accept_streams(
    inner: Arc<Inner>,
    conn: Connection,
    peer: String,
    mut ctrl_rx: Option<mpsc::UnboundedReceiver<Msg>>,
) {
    loop {
        let (send, mut recv) = match conn.accept_bi().await {
            Ok(s) => s,
            Err(_) => break,
        };
        let Ok(tag) = recv.read_u8().await else { continue };
        match tag {
            STREAM_CONTROL => {
                if let Some(rx) = ctrl_rx.take() {
                    crate::engine::spawn(control_writer(send, rx));
                    crate::engine::spawn(control_reader(inner.clone(), peer.clone(), recv));
                }
            }
            STREAM_UPDATE => {
                let inner = inner.clone();
                crate::engine::spawn(async move {
                    if let Err(e) = crate::update::serve(&inner, send, recv).await {
                        tracing::info!("отдача обновления прервалась: {e:#}");
                    }
                });
            }
            STREAM_FILE => {
                let inner = inner.clone();
                let peer = peer.clone();
                crate::engine::spawn(async move {
                    if let Err(e) = transfer::serve_file(&inner, &peer, send, recv).await {
                        tracing::info!("отдача файла прервалась: {e:#}");
                    }
                });
            }
            _ => {}
        }
    }
}

async fn control_writer(mut send: SendStream, mut rx: mpsc::UnboundedReceiver<Msg>) {
    loop {
        let msg = tokio::select! {
            m = rx.recv() => match m { Some(m) => m, None => break },
            _ = tokio::time::sleep(Duration::from_secs(15)) => Msg::Ping,
        };
        if write_msg(&mut send, &msg).await.is_err() {
            break;
        }
    }
}

async fn control_reader(inner: Arc<Inner>, peer: String, mut recv: RecvStream) {
    loop {
        match read_msg::<Msg>(&mut recv).await {
            Ok(msg) => handle_msg(&inner, &peer, msg),
            Err(_) => break,
        }
    }
}

fn on_connected(inner: &Arc<Inner>, peer: &str) {
    inner.send(peer, inner.hello());
    send_pending_offers(inner, peer);
    let items: Vec<(String, Status)> = {
        let s = inner.st();
        let since = now_ms() - 14 * 24 * 3600 * 1000;
        s.incoming
            .iter()
            .filter(|i| i.offer.from == peer && i.updated_at > since)
            .filter_map(|i| Status::from_in(i.state, i.done_bytes).map(|st| (i.offer.id.clone(), st)))
            .collect()
    };
    if !items.is_empty() {
        inner.send(peer, Msg::Statuses { items });
    }
}

pub(crate) fn send_pending_offers(inner: &Arc<Inner>, peer: &str) {
    let offers: Vec<Offer> = {
        let s = inner.st();
        s.outgoing
            .iter()
            .filter(|o| o.to == peer && !o.state.is_final())
            .map(|o| o.offer.clone())
            .collect()
    };
    if !offers.is_empty() {
        inner.send(peer, Msg::Offers { offers });
    }
}

fn handle_msg(inner: &Arc<Inner>, peer: &str, msg: Msg) {
    match msg {
        Msg::Hello { name, members, removed, cloud, version, update, paused } => {
            if members.iter().any(|m| m.id != inner.me && !inner.st().group.is_member(&m.id)) {
                crate::sharing::seed_existing(inner);
            }
            lock(&inner.peer_versions).insert(peer.to_string(), version);
            // Теперь известна версия — можно отдать ждущие сообщения.
            crate::notes::flush(inner, peer);
            let changed_pause = if paused {
                lock(&inner.peer_paused).insert(peer.to_string())
            } else {
                lock(&inner.peer_paused).remove(peer)
            };
            if changed_pause {
                // Устройство встало на паузу или продолжило — пересмотреть загрузки.
                inner.kick.notify_one();
            }
            if let Some(info) = update {
                crate::update::offered(inner, peer, info);
            }
            let (changed, new_members) = {
                let mut s = inner.st();
                let before: Vec<String> = s.group.active().map(|m| m.id.clone()).collect();
                let mut changed = s.group.merge(&members, &removed);
                if let Some(m) = s.group.members.iter_mut().find(|m| m.id == peer)
                    && m.name != name
                    && !name.is_empty()
                {
                    m.name = name;
                    changed = true;
                }
                // Облако общее для семьи: берём подключение, если своего нет или пришло более новое
                // (кто-то переподключил Диск), но не то, от которого здесь отказались.
                // Своя папка облачного диска — выбор этого компьютера, её не заменяем.
                if let Some(theirs) = cloud.filter(|c| c.usable() && !c.is_local())
                    && s.cloud_dropped.as_deref() != Some(theirs.identity().as_str())
                    && s.cloud.as_ref().is_none_or(|mine| {
                        !mine.is_local() && mine.identity() != theirs.identity() && theirs.expires_at > mine.expires_at
                    })
                {
                    s.cloud = Some(theirs);
                    changed = true;
                }
                let new_members: Vec<(String, String)> = s
                    .group
                    .active()
                    .filter(|m| !before.contains(&m.id) && m.id != inner.me)
                    .map(|m| (m.id.clone(), m.name.clone()))
                    .collect();
                (changed, new_members)
            };
            if changed {
                inner.save_soon();
                inner.broadcast_hello();
                inner.dial_now.notify_one();
                inner.kick_cloud();
            }
            for (m, _) in new_members {
                let added_by = inner.st().group.members.iter().find(|p| p.id == m)
                    .map(|p| p.added_by.clone()).filter(|p| !p.is_empty()).unwrap_or_else(|| peer.into());
                crate::sharing::queue(inner, &m, &added_by);
            }
            inner.changed();
        }
        Msg::Offers { offers } => handle_offers(inner, peer, offers),
        Msg::Statuses { items } => handle_statuses(inner, peer, items),
        Msg::Ping => {}
        Msg::Note { note } => crate::notes::received(inner, peer, note),
        Msg::NoteAck { id } => crate::notes::acked(inner, peer, &id),
    }
}

/// Новые предложения от `peer`. Используется и для предложений из облака.
pub(crate) fn handle_offers(inner: &Arc<Inner>, peer: &str, offers: Vec<Offer>) {
    let mut events = Vec::new();
    let mut statuses = Vec::new();
    let mut superseded = Vec::new();
    {
        let mut s = inner.st();
        let from_name = s.group.name_of(peer);
        let auto_mode = s.settings.auto_accept.unwrap_or_default();
        let auto_mb = s.settings.auto_accept_mb;
        let auto_exe = s.settings.auto_accept_exe;
        let free = crate::util::free_space(&s.settings.folder);
        for offer in offers {
            if offer.from != peer {
                continue;
            }
            if let Some(inc) = s.incoming.iter_mut().find(|i| i.offer.id == offer.id) {
                if offer.in_cloud && !inc.offer.in_cloud {
                    inc.offer.in_cloud = true;
                }
                if let Some(st) = Status::from_in(inc.state, inc.done_bytes) {
                    statuses.push((offer.id.clone(), st));
                }
                continue;
            }
            let mut queued = false;
            for sid in &offer.supersedes {
                if let Some(old) = s.incoming.iter_mut().find(|i| &i.offer.id == sid) {
                    match old.state {
                        InState::New => old.state = InState::Superseded,
                        InState::Queued | InState::Failed => {
                            old.state = InState::Superseded;
                            queued = true;
                        }
                        _ => continue,
                    }
                    old.updated_at = now_ms();
                    superseded.push(sid.clone());
                }
            }
            let has_exe = offer.files.iter().any(|f| crate::util::is_executable(&f.path));
            let size = offer.total_size();
            let auto = match auto_mode {
                AutoAccept::Off => false,
                AutoAccept::Photos => offer.files.iter().all(|f| crate::util::is_image(&f.path)),
                AutoAccept::All => true,
            } && (auto_mb == 0 || size <= auto_mb * 1024 * 1024)
                && (auto_exe || !has_exe)
                && free.is_none_or(|f| size + AUTO_ACCEPT_RESERVE <= f);
            let state = if queued || auto { InState::Queued } else { InState::New };
            statuses.push((offer.id.clone(), Status::from_in(state, 0).unwrap()));
            if state == InState::New {
                let n = offer.files.len();
                let detail = if offer.is_folder {
                    format!("{} · {}", files_word(n), fmt_size(offer.total_size()))
                } else {
                    fmt_size(offer.total_size())
                };
                events.push(Event::NewOffer {
                    id: offer.id.clone(),
                    from: from_name.clone(),
                    title: offer.item.clone(),
                    detail,
                    is_update: offer.is_update(),
                    has_exe,
                });
            }
            s.incoming.push(Incoming {
                offer,
                state,
                done_files: vec![],
                done_bytes: 0,
                source: None,
                message: None,
                conflicts: vec![],
                saved: vec![],
                auto,
                updated_at: now_ms(),
            });
        }
    }
    for id in superseded {
        let _ = std::fs::remove_dir_all(inner.staging().join("in").join(&id));
    }
    inner.save_soon();
    for ev in events {
        inner.emit(ev);
    }
    if !statuses.is_empty() {
        inner.send(peer, Msg::Statuses { items: statuses });
    }
    inner.kick.notify_one();
    inner.changed();
}

pub(crate) fn handle_statuses(inner: &Arc<Inner>, peer: &str, items: Vec<(String, Status)>) {
    let mut events = Vec::new();
    {
        let mut s = inner.st();
        let notify = s.settings.notify_delivered;
        let to_name = s.group.name_of(peer);
        for (id, status) in items {
            let Some(o) = s.outgoing.iter_mut().find(|o| o.offer.id == id && o.to == peer) else {
                continue;
            };
            let before = o.state;
            if before.is_final() && !(before == OutState::Superseded && status == Status::Delivered) {
                continue;
            }
            o.state = match status {
                Status::Received if before == OutState::Pending => OutState::Offered,
                Status::Received => before,
                Status::Accepted => {
                    if before == OutState::Downloading { before } else { OutState::Accepted }
                }
                Status::Downloading(done) => {
                    o.progress = done;
                    OutState::Downloading
                }
                Status::Delivered => {
                    o.progress = o.offer.total_size();
                    OutState::Delivered
                }
                Status::Declined => OutState::Declined,
                Status::Unavailable => OutState::Unavailable,
            };
            if o.state != before {
                o.updated_at = now_ms();
            }
            if o.state == OutState::Delivered && before != OutState::Delivered && notify {
                events.push(Event::Delivered { title: o.offer.item.clone(), to: to_name.clone() });
            }
        }
    }
    inner.save_soon();
    for ev in events {
        inner.emit(ev);
    }
    inner.kick_cloud();
    inner.changed();
}

pub(crate) fn files_word(n: usize) -> String {
    crate::i18n::plural("files", n as u64)
}
