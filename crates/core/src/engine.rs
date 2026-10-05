//! «Двигатель»: общее состояние, фоновые задачи и команды для окна.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use iroh::Endpoint;
use iroh::endpoint::{Connection, presets};
use tokio::sync::{Notify, mpsc};

use crate::config::{AutoAccept, Settings, load_or_create_key};
use crate::model::{Group, InState, Member, Status};
use crate::proto::{Msg, PAIR_ALPN, SYNC_ALPN};
use crate::store::{self, State};
use crate::util::{fmt_size, now_ms, random_bytes};
use crate::{cloud, pairing, scan, sync, transfer, view};

/// События для окна и уведомлений.
#[derive(Debug, Clone)]
pub enum Event {
    /// Что-то изменилось — окно запрашивает новое состояние.
    Changed,
    NewOffer {
        id: String,
        from: String,
        title: String,
        detail: String,
        is_update: bool,
        has_exe: bool,
    },
    Received {
        id: String,
        title: String,
        path: PathBuf,
        conflicts: usize,
    },
    Delivered {
        title: String,
        to: String,
    },
    /// В семью добавилось устройство.
    Joined {
        id: String,
        name: String,
        added_by: String,
        files: usize,
    },
    /// Пришло сообщение (текст, ссылка).
    Note {
        id: String,
        from: String,
        text: String,
    },
    /// Скачан установщик новой версии — окно проверяет подпись.
    UpdateReady {
        version: String,
        path: PathBuf,
        sig: String,
        hash: String,
    },
}

pub(crate) struct PeerConn {
    pub conn: Connection,
    pub serial: u64,
    pub initiator: String,
    pub ctrl: mpsc::UnboundedSender<Msg>,
}

pub(crate) struct Dirty {
    pub last: Instant,
    pub quiet: Duration,
}

#[derive(Default)]
pub(crate) struct Rate {
    last: Option<(Instant, u64)>,
    pub bps: f64,
}

impl Rate {
    pub fn update(&mut self, done: u64) {
        let now = Instant::now();
        match self.last {
            None => self.last = Some((now, done)),
            Some((t, b)) => {
                let dt = (now - t).as_secs_f64();
                if dt >= 0.5 {
                    let inst = done.saturating_sub(b) as f64 / dt;
                    self.bps = if self.bps == 0.0 { inst } else { 0.6 * self.bps + 0.4 * inst };
                    self.last = Some((now, done));
                }
            }
        }
    }
}

/// Равномерно растягивает передачу, чтобы не превышать заданную скорость.
pub(crate) struct Limiter {
    kbps: AtomicU64,
    next: tokio::sync::Mutex<Option<tokio::time::Instant>>,
}

impl Limiter {
    fn new(kbps: u64) -> Self {
        Self { kbps: AtomicU64::new(kbps), next: tokio::sync::Mutex::new(None) }
    }

    pub fn set(&self, kbps: u64) {
        self.kbps.store(kbps, Ordering::Relaxed);
    }

    pub async fn take(&self, bytes: usize) {
        let kbps = self.kbps.load(Ordering::Relaxed);
        if kbps == 0 {
            return;
        }
        let cost = Duration::from_secs_f64(bytes as f64 / (kbps as f64 * 1024.0));
        let start = {
            let mut next = self.next.lock().await;
            let now = tokio::time::Instant::now();
            let start = next.filter(|n| *n > now).unwrap_or(now);
            *next = Some(start + cost);
            start
        };
        tokio::time::sleep_until(start + cost).await;
    }
}

pub(crate) struct CloudAuth {
    pub user_code: String,
    pub url: String,
}

pub(crate) struct Inner {
    pub data_dir: PathBuf,
    pub me: String,
    pub endpoint: Endpoint,
    state: Mutex<State>,
    save_flag: AtomicBool,
    save_lock: Mutex<()>,
    save_notify: Notify,
    changed_flag: AtomicBool,
    events: mpsc::UnboundedSender<Event>,
    pub conns: Mutex<HashMap<String, PeerConn>>,
    pub dialing: Mutex<HashSet<String>>,
    pub dial_backoff: Mutex<HashMap<String, (Instant, Duration)>>,
    pub dial_now: Notify,
    pub dirty: Mutex<HashMap<String, Dirty>>,
    pub scanning: Mutex<HashSet<String>>,
    pub preparing: Mutex<BTreeMap<String, (u64, u64)>>,
    pub downloads: Mutex<HashSet<String>>,
    pub voice_downloads: Mutex<HashSet<String>>,
    pub voice_serving: AtomicU64,
    pub chat_cloud_busy: AtomicBool,
    pub cancelled: Mutex<HashSet<String>>,
    pub uploads: Mutex<HashMap<String, (u64, u64)>>,
    pub forced_uploads: Mutex<HashSet<String>>,
    pub rates: Mutex<HashMap<String, Rate>>,
    pub status_sent: Mutex<HashMap<String, Instant>>,
    pub up_limit: Limiter,
    pub down_limit: Limiter,
    pub kick: Notify,
    pub cloud_kick: Notify,
    pub poll_kick: Notify,
    pub serial: AtomicU64,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
    pub cloud_auth: Mutex<Option<CloudAuth>>,
    pub cloud_error: Mutex<Option<String>>,
    pub cloud_dirs: Mutex<HashSet<String>>,
    /// Версия программы на других устройствах (из приветствия).
    pub peer_versions: Mutex<HashMap<String, String>>,
    /// Пауза до этого времени (мс), 0 — нет.
    pub paused_until: AtomicI64,
    /// Устройства, которые сейчас на паузе (у них ничего не скачиваем).
    pub peer_paused: Mutex<HashSet<String>>,
    pub update_pkg: Mutex<Option<crate::update::UpdatePkg>>,
    /// Проверенный установщик новее установленной версии: (версия, путь).
    pub update_ready: Mutex<Option<(String, PathBuf)>>,
    pub update_downloading: Mutex<HashSet<String>>,
    /// Установщики с неверной подписью — больше не скачивать.
    pub update_rejected: Mutex<HashSet<String>>,
    /// Установщики от семьи не нужны: программу обновляет Microsoft Store.
    pub family_updates_off: AtomicBool,
    /// Кому предложить только что положенное в папку (бросили на устройство в шторке):
    /// элемент → (устройства, до какого времени ждать появления).
    pub targets: Mutex<HashMap<String, (Vec<String>, Instant)>>,
    pub http: reqwest::Client,
    pub closing: AtomicBool,
}

static RUNTIME: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();

/// Запуск фоновой задачи. Работает и из потоков вне tokio (команды окна, уведомления).
pub(crate) fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            h.spawn(fut);
        }
        Err(_) => match RUNTIME.get() {
            Some(h) => {
                h.spawn(fut);
            }
            None => tracing::error!("фоновая задача без среды выполнения"),
        },
    }
}

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    pub fn st(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    pub fn save_soon(&self) {
        self.save_flag.store(true, Ordering::Release);
        self.save_notify.notify_one();
    }

    pub fn save_now(&self) {
        if let Err(e) = self.save_checked() {
            tracing::error!("не удалось сохранить состояние: {e:#}");
        }
    }

    pub fn save_checked(&self) -> Result<()> {
        let _saving = lock(&self.save_lock);
        let data = {
            let mut s = self.st();
            s.prune();
            serde_json::to_vec(&*s)?
        };
        crate::util::write_atomic(&self.data_dir.join("state.json"), &data)
    }

    /// Сообщает окну об изменениях не чаще ~6 раз в секунду.
    pub fn changed(self: &Arc<Self>) {
        if !self.changed_flag.swap(true, Ordering::AcqRel) {
            let inner = self.clone();
            crate::engine::spawn(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                inner.changed_flag.store(false, Ordering::Release);
                let _ = inner.events.send(Event::Changed);
            });
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_until.load(Ordering::Relaxed) > now_ms()
    }

    pub fn emit(&self, ev: Event) {
        let _ = self.events.send(ev);
    }

    pub fn root(&self) -> PathBuf {
        self.st().settings.folder.clone()
    }

    pub fn staging(&self) -> PathBuf {
        self.root().join(".obshaya")
    }

    pub fn conn(&self, peer: &str) -> Option<Connection> {
        lock(&self.conns)
            .get(peer)
            .filter(|c| c.conn.close_reason().is_none())
            .map(|c| c.conn.clone())
    }

    pub fn is_online(&self, peer: &str) -> bool {
        self.conn(peer).is_some()
    }

    pub fn send(&self, peer: &str, msg: Msg) -> bool {
        lock(&self.conns)
            .get(peer)
            .is_some_and(|c| c.ctrl.send(msg).is_ok())
    }

    pub fn group_key(&self) -> [u8; 32] {
        self.st().group.key_bytes()
    }

    pub fn update_rate(&self, id: &str, done: u64) -> f64 {
        let mut rates = lock(&self.rates);
        let r = rates.entry(id.to_string()).or_default();
        r.update(done);
        r.bps
    }

    /// Отправляет статус входящего предложения его отправителю (не чаще раза в 2 с для прогресса).
    pub fn send_status(&self, offer_id: &str) {
        let found = {
            let s = self.st();
            s.incoming
                .iter()
                .find(|i| i.offer.id == offer_id)
                .and_then(|i| Status::from_in(i.state, i.done_bytes).map(|st| (i.offer.from.clone(), st)))
        };
        let Some((peer, status)) = found else { return };
        if matches!(status, Status::Downloading(_)) {
            let mut sent = lock(&self.status_sent);
            if sent.get(offer_id).is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
                return;
            }
            sent.insert(offer_id.to_string(), Instant::now());
        }
        self.send(&peer, Msg::Statuses { items: vec![(offer_id.to_string(), status)] });
    }

    pub fn hello(&self) -> Msg {
        let s = self.st();
        Msg::Hello {
            name: s.settings.device_name.clone(),
            members: s.group.members.clone(),
            removed: s.group.removed.clone(),
            version: crate::proto::APP_VERSION.to_string(),
            cloud: s.cloud.clone().filter(|c| !c.is_local()),
            update: lock(&self.update_pkg).as_ref().map(|p| p.info.clone()),
            paused: self.is_paused(),
        }
    }

    /// Будит загрузку в облако и проверку почтового ящика.
    pub fn kick_cloud(&self) {
        self.cloud_kick.notify_one();
        self.poll_kick.notify_one();
    }

    pub fn broadcast_hello(&self) {
        let peers: Vec<String> = lock(&self.conns).keys().cloned().collect();
        for p in peers {
            self.send(&p, self.hello());
        }
    }
}

/// Ручка для окна. Дешёво клонируется.
#[derive(Clone)]
pub struct Engine(pub(crate) Arc<Inner>);

impl Engine {
    pub async fn start(data_dir: PathBuf) -> Result<(Engine, mpsc::UnboundedReceiver<Event>)> {
        Self::start_in(data_dir, None).await
    }

    /// Запуск с заданной общей папкой (для тестов и второго экземпляра).
    pub async fn start_in(
        data_dir: PathBuf,
        folder: Option<PathBuf>,
    ) -> Result<(Engine, mpsc::UnboundedReceiver<Event>)> {
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("cannot create {}", data_dir.display()))?;
        let _ = RUNTIME.set(tokio::runtime::Handle::current());
        let _ = rustls::crypto::ring::default_provider().install_default();

        let key = load_or_create_key(&data_dir.join("device.key"))?;
        let me = key.public().to_string();
        let mut state = store::load(&data_dir.join("state.json"));
        if !state.settings.language.is_empty() && !crate::i18n::LANGS.iter().any(|(code,_)|*code==state.settings.language) {
            state.settings.language.clear();
        }
        crate::i18n::set_lang(&state.settings.language);
        if state.settings.auto_accept.is_none() {
            // Настройки до 1.1: автоприём был только для фото, 0 — выключен.
            let mb = state.settings.auto_accept_mb;
            state.settings.auto_accept = Some(if mb > 0 { AutoAccept::Photos } else { AutoAccept::Off });
            if mb == 0 {
                state.settings.auto_accept_mb = 1024;
            }
        }
        if state.group.id.is_empty() {
            state.group = Group {
                id: crate::util::hex(&random_bytes::<16>()),
                key: crate::util::hex(&random_bytes::<32>()),
                members: vec![],
                removed: vec![],
            };
        }
        if let Some(folder) = folder {
            state.settings.folder = folder;
        }
        if state.cloud_client.is_none()
            && let Some(c) = state.cloud.as_ref().filter(|c| !c.client_id.is_empty())
        {
            state.cloud_client = Some((c.client_id.clone(), c.client_secret.clone()));
        }
        let my_name = state.settings.device_name.clone();
        match state.group.members.iter_mut().find(|m| m.id == me) {
            Some(m) => m.name = my_name,
            None => state.group.members.push(Member { id: me.clone(), name: my_name, added_by: String::new() }),
        }
        crate::sharing::migrate(&mut state, &me);
        // Загрузки, прерванные выключением, продолжатся сами.
        for i in &mut state.incoming {
            if i.state == InState::Downloading {
                i.state = InState::Queued;
            }
        }

        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(key)
            .alpns(vec![SYNC_ALPN.to_vec(), PAIR_ALPN.to_vec()])
            .bind()
            .await
            .context("network start failed")?;

        let (tx, rx) = mpsc::unbounded_channel();
        let limit = state.settings.speed_limit_kbps;
        let paused_until = state.paused_until;
        let inner = Arc::new(Inner {
            data_dir,
            me,
            endpoint,
            state: Mutex::new(state),
            save_flag: AtomicBool::new(false),
            save_notify: Notify::new(),
            changed_flag: AtomicBool::new(false),
            events: tx,
            conns: Mutex::default(),
            dialing: Mutex::default(),
            dial_backoff: Mutex::default(),
            dial_now: Notify::new(),
            dirty: Mutex::default(),
            scanning: Mutex::default(),
            preparing: Mutex::default(),
            downloads: Mutex::default(),
            voice_downloads: Mutex::default(),
            voice_serving: AtomicU64::new(0),
            chat_cloud_busy: AtomicBool::new(false),
            save_lock: Mutex::default(),
            cancelled: Mutex::default(),
            uploads: Mutex::default(),
            forced_uploads: Mutex::default(),
            rates: Mutex::default(),
            status_sent: Mutex::default(),
            up_limit: Limiter::new(limit),
            down_limit: Limiter::new(limit),
            kick: Notify::new(),
            cloud_kick: Notify::new(),
            poll_kick: Notify::new(),
            serial: AtomicU64::new(1),
            watcher: Mutex::new(None),
            cloud_auth: Mutex::new(None),
            cloud_error: Mutex::new(None),
            cloud_dirs: Mutex::default(),
            peer_versions: Mutex::default(),
            paused_until: AtomicI64::new(paused_until),
            peer_paused: Mutex::default(),
            update_pkg: Mutex::default(),
            update_ready: Mutex::default(),
            update_downloading: Mutex::default(),
            update_rejected: Mutex::default(),
            family_updates_off: AtomicBool::new(false),
            targets: Mutex::default(),
            http: reqwest::Client::builder()
                .user_agent(concat!("ObshayaPapka/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(20))
                .build()?,
            closing: AtomicBool::new(false),
        });
        inner.save_now();

        crate::engine::spawn(saver(inner.clone()));
        crate::engine::spawn(sync::accept_loop(inner.clone()));
        crate::engine::spawn(sync::dial_loop(inner.clone()));
        scan::start(&inner);
        crate::engine::spawn(transfer::scheduler(inner.clone()));
        cloud::start(&inner);
        crate::engine::spawn(crate::voice::scheduler(inner.clone()));
        Ok((Engine(inner), rx))
    }

    pub fn snapshot(&self) -> view::UiState {
        view::build(&self.0)
    }

    pub fn device_id(&self) -> String {
        self.0.me.clone()
    }

    pub fn folder(&self) -> PathBuf {
        self.0.root()
    }

    pub fn settings(&self) -> Settings {
        self.0.st().settings.clone()
    }

    /// Сохранение настроек. Смена папки перезапускает слежение.
    pub fn set_settings(&self, mut new: Settings) -> Result<()> {
        if !new.language.is_empty() && !crate::i18n::LANGS.iter().any(|(code,_)|*code==new.language) { new.language.clear(); }
        let inner = &self.0;
        let (folder_changed, name_changed) = {
            let mut s = inner.st();
            if s.settings.language != new.language {
                crate::i18n::set_lang(&new.language);
            }
            let folder_changed = s.settings.folder != new.folder;
            let name_changed = s.settings.device_name != new.device_name;
            let me = inner.me.clone();
            if name_changed
                && let Some(m) = s.group.members.iter_mut().find(|m| m.id == me)
            {
                m.name = new.device_name.clone();
            }
            s.settings = new.clone();
            (folder_changed, name_changed)
        };
        inner.up_limit.set(new.speed_limit_kbps);
        inner.down_limit.set(new.speed_limit_kbps);
        if folder_changed {
            std::fs::create_dir_all(&new.folder)
                .with_context(|| crate::t!("err.create_folder", path = new.folder.display()))?;
            scan::restart_watcher(inner);
            let inner2 = inner.clone();
            crate::engine::spawn(async move { scan::scan_all(&inner2).await });
        }
        if name_changed {
            inner.broadcast_hello();
        }
        inner.save_soon();
        inner.changed();
        Ok(())
    }

    /// «Получить». Проверяет место на диске.
    pub fn accept(&self, id: &str) -> Result<()> {
        let inner = &self.0;
        let (need, root) = {
            let s = inner.st();
            let inc = s.incoming.iter().find(|i| i.offer.id == id).context(crate::t!("err.offer_not_found"))?;
            (inc.offer.total_size().saturating_sub(inc.done_bytes), s.settings.folder.clone())
        };
        if let Some(free) = crate::util::free_space(&root)
            && need + 200 * 1024 * 1024 > free
        {
            bail!(crate::t!("err.no_space", need = fmt_size(need), free = fmt_size(free)));
        }
        {
            let mut s = inner.st();
            if let Some(inc) = s.incoming.iter_mut().find(|i| i.offer.id == id)
                && matches!(inc.state, InState::New | InState::Failed | InState::Declined)
            {
                inc.state = InState::Queued;
                inc.message = None;
                inc.updated_at = now_ms();
            }
        }
        lock(&inner.cancelled).remove(id);
        inner.save_soon();
        inner.send_status(id);
        inner.kick.notify_one();
        inner.changed();
        Ok(())
    }

    /// Разрешить или запретить передачу прежних собственных файлов новому участнику.
    pub fn share_history(&self, peer: &str, allow: bool) -> Result<()> {
        crate::sharing::answer(&self.0, peer, allow)
    }

    /// «Отклонить» или отмена идущей загрузки.
    pub fn decline(&self, id: &str) {
        if !crate::voice::valid_id(id) { return; }
        let inner = &self.0;
        {
            let mut s = inner.st();
            if let Some(inc) = s.incoming.iter_mut().find(|i| i.offer.id == id)
                && !inc.state.is_final()
            {
                inc.state = InState::Declined;
                inc.updated_at = now_ms();
            }
        }
        lock(&inner.cancelled).insert(id.to_string());
        let _ = std::fs::remove_dir_all(inner.staging().join("in").join(id));
        inner.save_soon();
        inner.send_status(id);
        inner.changed();
    }

    /// Убрать запись из истории.
    pub fn dismiss(&self, id: &str) {
        let inner = &self.0;
        {
            let mut s = inner.st();
            s.incoming.retain(|i| !(i.offer.id == id && i.state.is_final()));
            s.outgoing.retain(|o| !(o.offer.id == id && o.state.is_final()));
            s.notes.retain(|n| n.id != id);
        }
        if let Ok(path) = crate::voice::path(inner, id) {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(path.with_extension("part"));
        }
        inner.save_soon();
        inner.changed();
    }

    pub fn upload_now(&self, id: &str) {
        lock(&self.0.forced_uploads).insert(id.to_string());
        self.0.kick_cloud();
        self.0.changed();
    }

    pub fn uploads_in_progress(&self) -> bool {
        !lock(&self.0.uploads).is_empty()
    }

    pub fn create_invite(&self) -> String {
        pairing::create_invite(&self.0)
    }

    /// Присоединение к семье по коду. Возвращает имя пригласившего устройства.
    pub async fn join(&self, code: &str) -> Result<String> {
        pairing::join(&self.0, code).await
    }

    pub fn remove_device(&self, id: &str) {
        let inner = &self.0;
        if id == inner.me {
            return;
        }
        {
            let mut s = inner.st();
            if !s.group.removed.iter().any(|r| r == id) {
                s.group.removed.push(id.to_string());
            }
            for o in s.outgoing.iter_mut().filter(|o| o.to == id && !o.state.is_final()) {
                o.state = crate::model::OutState::Superseded;
            }
        }
        inner.broadcast_hello();
        if let Some(c) = lock(&inner.conns).remove(id) {
            c.conn.close(0u32.into(), b"removed");
        }
        inner.save_soon();
        inner.changed();
    }

    pub async fn cloud_connect_yandex(&self, client_id: &str, client_secret: &str) -> Result<()> {
        cloud::begin_yandex_auth(&self.0, client_id, client_secret).await
    }

    /// Папка, которую синхронизирует облачный диск (или любая папка — для проверки).
    pub fn cloud_use_local(&self, dir: PathBuf) {
        cloud::use_local(&self.0, dir);
    }

    pub async fn cloud_connect_webdav(&self, url: &str, user: &str, password: &str) -> Result<()> {
        cloud::connect_webdav(&self.0, url, user, password).await
    }

    /// Облачные диски, найденные на этом компьютере: (название, папка для данных).
    pub fn cloud_folders(&self) -> Vec<(String, String)> {
        cloud::detect_folders()
    }

    pub fn cloud_disconnect(&self) {
        let inner = &self.0;
        {
            let mut s = inner.st();
            s.cloud_dropped = s.cloud.take().filter(|c| !c.is_local()).map(|c| c.identity()).filter(|t| !t.is_empty());
        }
        *lock(&inner.cloud_auth) = None;
        *lock(&inner.cloud_error) = None;
        inner.save_soon();
        inner.changed();
    }

    /// Сообщение семье (текст, ссылка): `peers` пусто — всем, иначе только им.
    pub fn send_note(&self, text: &str, peers: &[String]) -> Result<()> {
        crate::notes::send(&self.0, text, peers)
    }

    pub fn send_chat(&self, text: &str, peer: Option<&str>) -> Result<()> {
        crate::notes::send_chat(&self.0, text, peer, None, None)
    }

    pub fn send_voice(&self, data: &[u8], mime: &str, duration_ms: u64, peer: Option<&str>) -> Result<()> {
        crate::voice::send(&self.0, data, mime, duration_ms, peer, &[], None)
    }

    pub fn send_chat_reply(&self, text: &str, peer: Option<&str>, reply_to: Option<&str>) -> Result<()> {
        crate::notes::send_chat(&self.0, text, peer, None, reply_to)
    }

    pub fn send_voice_reply(&self, data: &[u8], mime: &str, duration_ms: u64, peer: Option<&str>, waveform: &[u8], reply_to: Option<&str>) -> Result<()> {
        crate::voice::send(&self.0, data, mime, duration_ms, peer, waveform, reply_to)
    }

    pub fn voice_bytes(&self, id: &str) -> Result<Vec<u8>> {
        crate::voice::bytes(&self.0, id)
    }

    /// Человек увидел полученные сообщения.
    pub fn notes_seen(&self, ids: &[String]) {
        let inner = &self.0;
        let changed = {
            let mut s = inner.st();
            let mut changed = false;
            for n in s.notes.iter_mut().filter(|n| !n.seen && ids.contains(&n.id)) {
                n.seen = true;
                changed = true;
            }
            changed
        };
        if changed {
            inner.save_soon();
            inner.changed();
        }
    }

    /// Текст сообщения (для кнопки «Копировать» в уведомлении).
    pub fn note_text(&self, id: &str) -> Option<String> {
        self.0.st().notes.iter().find(|n| n.id == id).map(|n| n.text.clone())
    }

    /// Элементы, которые сейчас положат в папку, предложить только этим устройствам
    /// (а не всей семье). Действует на первое предложение, ждёт до 15 минут.
    pub fn target(&self, items: Vec<String>, peers: Vec<String>) {
        let until = Instant::now() + Duration::from_secs(15 * 60);
        let mut t = lock(&self.0.targets);
        t.retain(|_, (_, u)| *u > Instant::now());
        for item in items {
            t.insert(item, (peers.clone(), until));
        }
    }

    /// Пауза: ничего не скачивать, не отдавать и не загружать в облако.
    /// `minutes` — на сколько; None — пока не нажмут «Продолжить».
    pub fn pause(&self, minutes: Option<u64>) {
        let until = minutes.map(|m| now_ms() + m as i64 * 60_000).unwrap_or(i64::MAX);
        self.set_pause(until);
    }

    pub fn resume(&self) {
        self.set_pause(0);
    }

    fn set_pause(&self, until: i64) {
        let inner = &self.0;
        inner.paused_until.store(until, Ordering::Relaxed);
        inner.st().paused_until = until;
        inner.save_soon();
        inner.broadcast_hello();
        if until == 0 {
            transfer::resumed(inner);
        }
        inner.changed();
    }

    pub fn is_paused(&self) -> bool {
        self.0.is_paused()
    }

    /// Идёт ли передача (тогда обновление подождёт).
    pub fn busy(&self) -> bool {
        let inner = &self.0;
        if !lock(&inner.downloads).is_empty() || !lock(&inner.uploads).is_empty()
            || !lock(&inner.voice_downloads).is_empty() || inner.voice_serving.load(Ordering::Relaxed) > 0 || inner.chat_cloud_busy.load(Ordering::Relaxed) {
            return true;
        }
        let s = inner.st();
        s.outgoing.iter().any(|o| o.state == crate::model::OutState::Downloading)
    }

    /// Установщик с проверенной подписью: отдавать другим устройствам; если он новее — готов к установке.
    pub async fn set_update_package(&self, version: &str, path: PathBuf, sig: &str) -> Result<()> {
        crate::update::set_package(&self.0, version, path, sig).await
    }

    /// Программа из Microsoft Store: её обновляет Store, установщики от семьи не скачивать.
    pub fn disable_family_updates(&self) {
        self.0.family_updates_off.store(true, Ordering::Relaxed);
    }

    /// Подпись не сошлась — этот установщик больше не скачивать.
    pub fn reject_update(&self, hash: &str) {
        lock(&self.0.update_rejected).insert(hash.to_string());
    }

    /// Готовое к установке обновление: (версия, установщик).
    pub fn update_ready(&self) -> Option<(String, PathBuf)> {
        lock(&self.0.update_ready).clone()
    }

    pub fn data_dir(&self) -> PathBuf {
        self.0.data_dir.clone()
    }

    pub async fn shutdown(&self) {
        let inner = &self.0;
        inner.closing.store(true, Ordering::Release);
        *lock(&inner.watcher) = None;
        inner.save_now();
        inner.endpoint.close().await;
    }
}

async fn saver(inner: Arc<Inner>) {
    loop {
        inner.save_notify.notified().await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        if inner.save_flag.swap(false, Ordering::AcqRel) {
            let inner2 = inner.clone();
            let _ = tokio::task::spawn_blocking(move || inner2.save_now()).await;
        }
        if inner.closing.load(Ordering::Acquire) {
            break;
        }
    }
}
