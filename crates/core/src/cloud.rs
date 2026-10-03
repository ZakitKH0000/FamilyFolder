//! Облако как запасной путь: когда получатель (или отправитель) не в сети.
//!
//! Три вида хранилища: Яндекс Диск (папка приложения), любой WebDAV-сервер (Nextcloud, Koofr,
//! pCloud, NAS…) и папка, которую синхронизирует облачный диск (OneDrive, Google Диск, Dropbox…).
//! Внутри:
//!   data/<ключ содержимого>/<номер куска>  — зашифрованные куски файлов по 32 МБ;
//!   mail/<получатель>/<предложение>.bin    — зашифрованные предложения;
//!   status/<отправитель>/<предложение>.bin — ответы получателя (доставлено/отклонено).
//! Всё удаляется, когда больше не нужно.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::config::CloudMode;
use crate::crypto::{content_key, open, seal};
use crate::engine::{CloudAuth, Inner, lock};
use crate::model::{CloudContent, CloudCreds, CloudUp, InState, Offer, OfferFile, OutState, Status};
use crate::scan::native;
use crate::t;
use crate::transfer::{DlError, Fetched, progress, resume_point};
use crate::util::{mtime_ms, now_ms, short_id};

pub(crate) const CHUNK: u64 = 32 * 1024 * 1024;
const API: &str = "https://cloud-api.yandex.net/v1/disk/resources";
const EXPIRE_MS: i64 = 30 * 24 * 3600 * 1000;
/// Своя папка на WebDAV-сервере и внутри папки облачного диска.
pub(crate) const DATA_DIR: &str = "Family Folder sync";

type Dirs = Mutex<HashSet<String>>;

pub(crate) enum Backend {
    Yandex { http: reqwest::Client, token: String },
    WebDav { http: reqwest::Client, root: String, user: String, pass: String },
    Local(PathBuf),
}

pub(crate) fn configured(inner: &Inner) -> bool {
    inner.st().cloud.as_ref().is_some_and(CloudCreds::usable)
}

fn backend(inner: &Inner) -> Option<Backend> {
    let c = inner.st().cloud.clone().filter(CloudCreds::usable)?;
    Some(match c.kind() {
        "folder" => Backend::Local(PathBuf::from(c.local_dir?)),
        "webdav" => Backend::WebDav { http: inner.http.clone(), root: webdav_root(&c.url), user: c.user, pass: c.password },
        _ => Backend::Yandex { http: inner.http.clone(), token: c.access_token },
    })
}

fn webdav_root(url: &str) -> String {
    format!("{}/{}", url.trim().trim_end_matches('/'), DATA_DIR.replace(' ', "%20"))
}

#[derive(Deserialize)]
struct Link {
    href: String,
}

impl Backend {
    fn auth(&self) -> String {
        match self {
            Backend::Yandex { token, .. } => format!("OAuth {token}"),
            _ => String::new(),
        }
    }

    fn dav(&self, method: &str, path: &str) -> reqwest::RequestBuilder {
        let Backend::WebDav { http, root, user, pass } = self else { unreachable!("WebDAV only") };
        let method = reqwest::Method::from_bytes(method.as_bytes()).expect("WebDAV method");
        let url = if path.is_empty() { format!("{root}/") } else { format!("{root}/{path}") };
        http.request(method, url).basic_auth(user, Some(pass))
    }

    async fn mkdirs(&self, dirs: &Dirs, path: &str) -> Result<()> {
        let parts: Vec<&str> = path.split('/').collect();
        match self {
            Backend::Local(_) => Ok(()),
            Backend::Yandex { http, .. } => {
                for i in 1..parts.len() {
                    let dir = parts[..i].join("/");
                    if lock(dirs).contains(&dir) {
                        continue;
                    }
                    let resp = http
                        .put(API)
                        .query(&[("path", format!("app:/{dir}"))])
                        .header("Authorization", self.auth())
                        .send()
                        .await?;
                    let code = resp.status().as_u16();
                    if code != 201 && code != 409 {
                        bail!(t!("cloud.err.mkdir", reason = api_error(resp).await));
                    }
                    lock(dirs).insert(dir);
                }
                Ok(())
            }
            Backend::WebDav { .. } => {
                for i in 0..parts.len() {
                    let dir = parts[..i].join("/");
                    if lock(dirs).contains(&dir) {
                        continue;
                    }
                    let target = if dir.is_empty() { String::new() } else { format!("{dir}/") };
                    let resp = self.dav("MKCOL", &target).send().await?;
                    let code = resp.status().as_u16();
                    if !(resp.status().is_success() || code == 405) {
                        bail!(t!("cloud.err.mkdir", reason = dav_error(code)));
                    }
                    lock(dirs).insert(dir);
                }
                Ok(())
            }
        }
    }

    async fn put(&self, dirs: &Dirs, path: &str, data: Vec<u8>) -> Result<()> {
        match self {
            Backend::Local(base) => {
                let p = local_path(base, path);
                if let Some(parent) = p.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let tmp = p.with_extension("uploading");
                tokio::fs::write(&tmp, &data).await?;
                tokio::fs::rename(&tmp, &p).await?;
                Ok(())
            }
            Backend::Yandex { http, .. } => {
                self.mkdirs(dirs, path).await?;
                let resp = http
                    .get(format!("{API}/upload"))
                    .query(&[("path", format!("app:/{path}")), ("overwrite", "true".into())])
                    .header("Authorization", self.auth())
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    bail!("{}", api_error(resp).await);
                }
                let link: Link = resp.json().await?;
                let resp = http.put(&link.href).body(data).send().await?;
                if !resp.status().is_success() {
                    bail!(t!("cloud.err.upload", reason = resp.status()));
                }
                Ok(())
            }
            Backend::WebDav { .. } => {
                self.mkdirs(dirs, path).await?;
                let resp = self.dav("PUT", path).body(data).send().await?;
                if !resp.status().is_success() {
                    // Папку могли удалить на сервере — в следующий раз создать заново.
                    lock(dirs).clear();
                    bail!(t!("cloud.err.upload", reason = dav_error(resp.status().as_u16())));
                }
                Ok(())
            }
        }
    }

    async fn get(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match self {
            Backend::Local(base) => match tokio::fs::read(local_path(base, path)).await {
                Ok(d) => Ok(Some(d)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            },
            Backend::Yandex { http, .. } => {
                let resp = http
                    .get(format!("{API}/download"))
                    .query(&[("path", format!("app:/{path}"))])
                    .header("Authorization", self.auth())
                    .send()
                    .await?;
                if resp.status().as_u16() == 404 {
                    return Ok(None);
                }
                if !resp.status().is_success() {
                    bail!("{}", api_error(resp).await);
                }
                let link: Link = resp.json().await?;
                let resp = http.get(&link.href).send().await?;
                if !resp.status().is_success() {
                    bail!(t!("cloud.err.download", reason = resp.status()));
                }
                Ok(Some(resp.bytes().await?.to_vec()))
            }
            Backend::WebDav { .. } => {
                let resp = self.dav("GET", path).send().await?;
                let code = resp.status().as_u16();
                if code == 404 {
                    return Ok(None);
                }
                if !resp.status().is_success() {
                    bail!(t!("cloud.err.download", reason = dav_error(code)));
                }
                Ok(Some(resp.bytes().await?.to_vec()))
            }
        }
    }

    async fn list(&self, dir: &str) -> Result<Vec<String>> {
        match self {
            Backend::Local(base) => {
                let mut out = Vec::new();
                if let Ok(mut rd) = tokio::fs::read_dir(local_path(base, dir)).await {
                    while let Some(e) = rd.next_entry().await? {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if !name.ends_with(".uploading") {
                            out.push(name);
                        }
                    }
                }
                Ok(out)
            }
            Backend::Yandex { http, .. } => {
                #[derive(Deserialize)]
                struct Item {
                    name: String,
                }
                #[derive(Deserialize)]
                struct Embedded {
                    items: Vec<Item>,
                }
                #[derive(Deserialize)]
                struct Resource {
                    #[serde(rename = "_embedded")]
                    embedded: Option<Embedded>,
                }
                let resp = http
                    .get(API)
                    .query(&[
                        ("path", format!("app:/{dir}")),
                        ("limit", "10000".into()),
                        ("fields", "_embedded.items.name".into()),
                    ])
                    .header("Authorization", self.auth())
                    .send()
                    .await?;
                if resp.status().as_u16() == 404 {
                    return Ok(vec![]);
                }
                if !resp.status().is_success() {
                    bail!("{}", api_error(resp).await);
                }
                let r: Resource = resp.json().await?;
                Ok(r.embedded.map(|e| e.items.into_iter().map(|i| i.name).collect()).unwrap_or_default())
            }
            Backend::WebDav { .. } => {
                let body = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/></d:prop></d:propfind>"#;
                let resp = self
                    .dav("PROPFIND", &format!("{dir}/"))
                    .header("Depth", "1")
                    .header("Content-Type", "application/xml; charset=utf-8")
                    .body(body)
                    .send()
                    .await?;
                let code = resp.status().as_u16();
                if code == 404 {
                    return Ok(vec![]);
                }
                if !resp.status().is_success() {
                    bail!(t!("cloud.err.list", reason = dav_error(code)));
                }
                Ok(dav_names(&resp.text().await?, dir))
            }
        }
    }

    /// Удаляет насовсем (мимо Корзины Диска, чтобы не занимать место).
    async fn delete(&self, path: &str) -> Result<()> {
        match self {
            Backend::Local(base) => {
                let p = local_path(base, path);
                if p.is_dir() {
                    let _ = tokio::fs::remove_dir_all(p).await;
                } else {
                    let _ = tokio::fs::remove_file(p).await;
                }
                Ok(())
            }
            Backend::Yandex { http, .. } => {
                let resp = http
                    .delete(API)
                    .query(&[("path", format!("app:/{path}")), ("permanently", "true".into())])
                    .header("Authorization", self.auth())
                    .send()
                    .await?;
                let code = resp.status().as_u16();
                if !(resp.status().is_success() || code == 404) {
                    bail!("{}", api_error(resp).await);
                }
                Ok(())
            }
            Backend::WebDav { .. } => {
                let resp = self.dav("DELETE", path).send().await?;
                let code = resp.status().as_u16();
                if !(resp.status().is_success() || code == 404) {
                    bail!(t!("cloud.err.delete", reason = dav_error(code)));
                }
                Ok(())
            }
        }
    }
}

fn local_path(base: &Path, path: &str) -> PathBuf {
    native(base, path)
}

async fn api_error(resp: reqwest::Response) -> String {
    let code = resp.status();
    #[derive(Deserialize)]
    struct E {
        message: Option<String>,
        description: Option<String>,
    }
    let text = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<E>(&text)
        .ok()
        .and_then(|e| e.message.or(e.description))
        .unwrap_or(text);
    match code.as_u16() {
        401 => t!("cloud.err.yandex_expired"),
        403 => t!("cloud.err.yandex_no_disk"),
        507 => t!("cloud.err.full"),
        _ => t!("cloud.err.yandex", reason = format!("{code}: {msg}")),
    }
}

fn dav_error(code: u16) -> String {
    match code {
        401 | 403 => t!("cloud.err.webdav_auth"),
        507 => t!("cloud.err.full"),
        _ => format!("HTTP {code}"),
    }
}

/// Имена внутри папки из ответа PROPFIND (сама папка в ответе тоже есть — её пропускаем).
fn dav_names(xml: &str, dir: &str) -> Vec<String> {
    let own = format!("{DATA_DIR}/{dir}");
    let own = own.trim_end_matches('/');
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let name = tag.split_whitespace().next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name);
        if tag.starts_with('/') || tag.ends_with('/') || !local.eq_ignore_ascii_case("href") {
            continue;
        }
        let Some(close) = rest.find('<') else { break };
        let href = percent_decode(&xml_unescape(rest[..close].trim()));
        let href = href.trim_end_matches('/');
        if href.ends_with(own) {
            continue;
        }
        if let Some(last) = href.rsplit('/').next().filter(|s| !s.is_empty()) {
            out.push(last.to_string());
        }
    }
    out
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn start(inner: &Arc<Inner>) {
    crate::engine::spawn(upload_loop(inner.clone()));
    crate::engine::spawn(poll_loop(inner.clone()));
}

fn set_error(inner: &Arc<Inner>, err: Option<String>) {
    let mut e = lock(&inner.cloud_error);
    if *e != err {
        *e = err;
        drop(e);
        inner.changed();
    }
}

// ---------- Отправитель: загрузка в облако ----------

async fn upload_loop(inner: Arc<Inner>) {
    let mut retry_after: Option<Instant> = None;
    loop {
        tokio::select! {
            _ = inner.cloud_kick.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(20)) => {}
        }
        if inner.closing.load(Ordering::Acquire) {
            break;
        }
        if inner.is_paused() {
            continue;
        }
        let Some(b) = backend(&inner) else { continue };
        loop {
            let Some(id) = next_upload(&inner, retry_after.is_some_and(|t| Instant::now() < t)) else { break };
            if inner.is_paused() {
                break;
            }
            match upload_offer(&inner, &b, &id).await {
                Ok(()) => {
                    retry_after = None;
                    set_error(&inner, None);
                }
                Err(e) => {
                    tracing::warn!("загрузка в облако: {e:#}");
                    {
                        let mut s = inner.st();
                        if let Some(o) = s.outgoing.iter_mut().find(|o| o.offer.id == id) {
                            o.cloud = CloudUp::Failed(format!("{e:#}"));
                        }
                    }
                    lock(&inner.uploads).remove(&id);
                    lock(&inner.forced_uploads).remove(&id);
                    set_error(&inner, Some(format!("{e:#}")));
                    retry_after = Some(Instant::now() + Duration::from_secs(300));
                    inner.changed();
                    break;
                }
            }
        }
    }
}

/// Какое предложение пора загрузить в облако.
fn next_upload(inner: &Arc<Inner>, backing_off: bool) -> Option<String> {
    let forced = lock(&inner.forced_uploads).clone();
    let s = inner.st();
    let mode = s.settings.cloud_mode;
    let now = now_ms();
    s.outgoing
        .iter()
        .filter(|o| !o.state.is_final() && matches!(o.state, OutState::Pending | OutState::Offered))
        .filter(|o| match &o.cloud {
            CloudUp::None => true,
            CloudUp::Failed(_) => !backing_off || forced.contains(&o.offer.id),
            _ => false,
        })
        .find(|o| {
            if forced.contains(&o.offer.id) {
                return true;
            }
            let age = now - o.offer.created_at;
            match mode {
                CloudMode::Never => false,
                CloudMode::Always => true,
                CloudMode::WhenNeeded => {
                    (!inner.is_online(&o.to) && age > 60_000) || age > 15 * 60_000
                }
            }
        })
        .map(|o| o.offer.id.clone())
}

async fn upload_offer(inner: &Arc<Inner>, b: &Backend, id: &str) -> Result<()> {
    let (mut offer, to) = {
        let mut s = inner.st();
        let o = s.outgoing.iter_mut().find(|o| o.offer.id == id).context(t!("err.offer_not_found"))?;
        o.cloud = CloudUp::Uploading;
        (o.offer.clone(), o.to.clone())
    };
    let total = offer.total_size();
    lock(&inner.uploads).insert(id.to_string(), (0, total));
    inner.changed();
    let key = inner.group_key();
    let mut base = 0u64;
    for f in &offer.files {
        let uploaded = inner.st().cloud_content.contains_key(&f.hash);
        if !uploaded {
            upload_content(inner, b, &key, f, id, base).await?;
            inner.st().cloud_content.insert(f.hash.clone(), CloudContent { size: f.size, uploaded_at: now_ms() });
            inner.save_soon();
        }
        base += f.size;
        if let Some(u) = lock(&inner.uploads).get_mut(id) {
            u.0 = base;
        }
    }
    offer.in_cloud = true;
    let path = format!("mail/{}/{}.bin", short_id(&to), offer.id);
    let data = seal(&key, &path, &serde_json::to_vec(&offer)?);
    b.put(&inner.cloud_dirs, &path, data).await?;
    {
        let mut s = inner.st();
        if let Some(o) = s.outgoing.iter_mut().find(|o| o.offer.id == id) {
            o.cloud = CloudUp::Uploaded;
            o.offer.in_cloud = true;
        }
    }
    lock(&inner.uploads).remove(id);
    lock(&inner.forced_uploads).remove(id);
    inner.save_soon();
    crate::sync::send_pending_offers(inner, &to);
    inner.changed();
    Ok(())
}

async fn upload_content(inner: &Arc<Inner>, b: &Backend, key: &[u8; 32], f: &OfferFile, id: &str, base: u64) -> Result<()> {
    let root = inner.root();
    let path = native(&root, &f.path);
    let entry = inner.st().index.get(&f.path).cloned();
    let unchanged = entry.as_ref().is_some_and(|e| e.hash == f.hash)
        && std::fs::metadata(&path).is_ok_and(|m| Some(m.len()) == entry.as_ref().map(|e| e.size) && Some(mtime_ms(&m)) == entry.as_ref().map(|e| e.mtime));
    if !unchanged {
        bail!(t!("cloud.err.changed", name = f.path));
    }
    let ck = content_key(key, &f.hash);
    let chunks = f.size.div_ceil(CHUNK).max(1);
    let existing: HashSet<String> = b.list(&format!("data/{ck}")).await?.into_iter().collect();
    for c in 0..chunks {
        let len = (f.size - c * CHUNK).min(CHUNK);
        if !existing.contains(&c.to_string()) {
            let data = {
                let path = path.clone();
                tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
                    use std::io::{Read, Seek};
                    let mut file = std::fs::File::open(&path)?;
                    file.seek(std::io::SeekFrom::Start(c * CHUNK))?;
                    let mut buf = vec![0u8; len as usize];
                    file.read_exact(&mut buf)?;
                    Ok(buf)
                })
                .await??
            };
            let obj = format!("data/{ck}/{c}");
            let sealed = seal(key, &obj, &data);
            inner.up_limit.take(sealed.len()).await;
            b.put(&inner.cloud_dirs, &obj, sealed).await?;
        }
        let done = base + c * CHUNK + len;
        if let Some(u) = lock(&inner.uploads).get_mut(id) {
            u.0 = done;
        }
        inner.update_rate(&format!("cloud:{id}"), done);
        inner.changed();
    }
    Ok(())
}

// ---------- Получатель: почтовый ящик и скачивание ----------

#[derive(Serialize, Deserialize)]
struct StatusNote {
    offer_id: String,
    to: String,
    status: Status,
}

async fn poll_loop(inner: Arc<Inner>) {
    let mut first = true;
    loop {
        if !first {
            tokio::select! {
                _ = inner.poll_kick.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            }
        }
        first = false;
        if inner.closing.load(Ordering::Acquire) {
            break;
        }
        if inner.is_paused() {
            continue;
        }
        let Some(b) = backend(&inner) else { continue };
        let result = async {
            poll_mail(&inner, &b).await?;
            report_statuses(&inner, &b).await?;
            poll_statuses(&inner, &b).await?;
            cleanup(&inner, &b).await
        }
        .await;
        match result {
            Ok(()) => {
                if lock(&inner.uploads).is_empty() {
                    set_error(&inner, None);
                }
            }
            Err(e) => {
                tracing::warn!("облако: {e:#}");
                set_error(&inner, Some(format!("{e:#}")));
            }
        }
        // Не чаще раза в 10 секунд, даже если будят часто.
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn poll_mail(inner: &Arc<Inner>, b: &Backend) -> Result<()> {
    let dir = format!("mail/{}", short_id(&inner.me));
    let key = inner.group_key();
    for name in b.list(&dir).await? {
        let Some(offer_id) = name.strip_suffix(".bin") else { continue };
        let known = inner.st().incoming.iter().any(|i| i.offer.id == offer_id);
        if known {
            continue;
        }
        let path = format!("{dir}/{name}");
        let Some(data) = b.get(&path).await? else { continue };
        let offer: Offer = match open(&key, &path, &data).and_then(|d| Ok(serde_json::from_slice(&d)?)) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!("предложение из облака не читается: {e:#}");
                continue;
            }
        };
        if !inner.st().group.is_member(&offer.from) {
            continue;
        }
        let from = offer.from.clone();
        crate::sync::handle_offers(inner, &from, vec![offer]);
    }
    Ok(())
}

/// Получатель сообщает через облако о завершённых предложениях из облака и убирает их из ящика.
async fn report_statuses(inner: &Arc<Inner>, b: &Backend) -> Result<()> {
    let todo: Vec<(String, String, Status)> = {
        let s = inner.st();
        s.incoming
            .iter()
            .filter(|i| i.offer.in_cloud && i.state.is_final() && i.state != InState::Superseded)
            .filter_map(|i| Status::from_in(i.state, i.done_bytes).map(|st| (i.offer.id.clone(), i.offer.from.clone(), st)))
            .collect()
    };
    let key = inner.group_key();
    let me = short_id(&inner.me).to_string();
    for (id, from, status) in todo {
        let mail = format!("mail/{me}/{id}.bin");
        if b.get(&mail).await?.is_none() {
            // Уже сообщили раньше.
            continue;
        }
        let path = format!("status/{}/{id}.bin", short_id(&from));
        let note = StatusNote { offer_id: id.clone(), to: inner.me.clone(), status };
        b.put(&inner.cloud_dirs, &path, seal(&key, &path, &serde_json::to_vec(&note)?)).await?;
        b.delete(&mail).await?;
    }
    Ok(())
}

async fn poll_statuses(inner: &Arc<Inner>, b: &Backend) -> Result<()> {
    let dir = format!("status/{}", short_id(&inner.me));
    let key = inner.group_key();
    for name in b.list(&dir).await? {
        let path = format!("{dir}/{name}");
        if let Some(data) = b.get(&path).await?
            && let Ok(note) = open(&key, &path, &data).and_then(|d| Ok(serde_json::from_slice::<StatusNote>(&d)?))
        {
            crate::sync::handle_statuses(inner, &note.to, vec![(note.offer_id, note.status)]);
        }
        b.delete(&path).await?;
    }
    Ok(())
}

/// Удаляет из облака то, что уже никому не нужно, и всё старше 30 дней.
async fn cleanup(inner: &Arc<Inner>, b: &Backend) -> Result<()> {
    let key = inner.group_key();
    let now = now_ms();
    let (stale_mail, needed) = {
        let s = inner.st();
        let stale: Vec<(String, String)> = s
            .outgoing
            .iter()
            .filter(|o| o.cloud == CloudUp::Uploaded && (o.state.is_final() || now - o.offer.created_at > EXPIRE_MS))
            .map(|o| (o.offer.id.clone(), o.to.clone()))
            .collect();
        let needed: HashSet<String> = s
            .outgoing
            .iter()
            .filter(|o| !o.state.is_final() && now - o.offer.created_at < EXPIRE_MS)
            .filter(|o| matches!(o.cloud, CloudUp::Uploaded | CloudUp::Uploading))
            .flat_map(|o| o.offer.files.iter().map(|f| f.hash.clone()))
            .collect();
        (stale, needed)
    };
    for (id, to) in stale_mail {
        b.delete(&format!("mail/{}/{id}.bin", short_id(&to))).await?;
        let mut s = inner.st();
        if let Some(o) = s.outgoing.iter_mut().find(|o| o.offer.id == id) {
            o.cloud = CloudUp::None;
        }
    }
    let uploading: HashSet<String> = lock(&inner.uploads).keys().cloned().collect();
    if !uploading.is_empty() {
        return Ok(());
    }
    let contents: Vec<String> = inner.st().cloud_content.keys().cloned().collect();
    for hash in contents {
        if !needed.contains(&hash) {
            b.delete(&format!("data/{}", content_key(&key, &hash))).await?;
            inner.st().cloud_content.remove(&hash);
            inner.save_soon();
        }
    }
    Ok(())
}

/// Скачивание одного файла предложения из облака (по кускам, с докачкой).
pub(crate) async fn fetch(
    inner: &Arc<Inner>,
    offer: &Offer,
    _idx: usize,
    f: &OfferFile,
    part: &Path,
    base: u64,
    _total: u64,
) -> Result<Fetched, DlError> {
    let b = backend(inner).ok_or_else(|| DlError { retry: true, msg: t!("cloud.err.not_connected") })?;
    let key = inner.group_key();
    let ck = content_key(&key, &f.hash);
    let (offset, mut hasher) = resume_point(part, f.size, CHUNK).await?;
    let mut file = tokio::fs::OpenOptions::new().append(true).open(part).await?;
    let chunks = f.size.div_ceil(CHUNK);
    let mut got = offset;
    for c in (offset / CHUNK)..chunks {
        if lock(&inner.cancelled).contains(&offer.id) {
            return Err(DlError { retry: false, msg: t!("err.cancelled") });
        }
        if inner.is_paused() {
            return Err(DlError { retry: true, msg: t!("in.paused") });
        }
        let obj = format!("data/{ck}/{c}");
        // Облачный диск на компьютере мог ещё не докачать кусок — тогда подождать.
        let synced = matches!(b, Backend::Local(_));
        let data = b.get(&obj).await?.ok_or_else(|| DlError {
            retry: synced,
            msg: if synced { t!("cloud.err.not_synced") } else { t!("cloud.err.missing_part") },
        })?;
        let plain = open(&key, &obj, &data).map_err(|e| DlError { retry: synced, msg: format!("{e:#}") })?;
        let expected = (f.size - c * CHUNK).min(CHUNK);
        if plain.len() as u64 != expected {
            return Err(anyhow!(t!("cloud.err.partial")).into());
        }
        inner.down_limit.take(data.len()).await;
        file.write_all(&plain).await?;
        hasher.update(&plain);
        got += expected;
        progress(inner, &offer.id, base + got);
    }
    file.flush().await?;
    drop(file);
    if hasher.finalize().to_hex().as_str() != f.hash {
        let _ = std::fs::remove_file(part);
        return Err(DlError { retry: false, msg: t!("cloud.err.corrupt") });
    }
    Ok(Fetched::Ok)
}

// ---------- Подключение Яндекс Диска ----------

#[derive(Deserialize)]
struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_url: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default = "default_expires")]
    expires_in: u64,
}

fn default_interval() -> u64 {
    5
}

fn default_expires() -> u64 {
    300
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Вход через код подтверждения: пользователь открывает страницу Яндекса и вводит код.
pub(crate) async fn begin_yandex_auth(inner: &Arc<Inner>, client_id: &str, client_secret: &str) -> Result<()> {
    let client_id = client_id.trim().to_string();
    let client_secret = client_secret.trim().to_string();
    anyhow::ensure!(!client_id.is_empty() && !client_secret.is_empty(), t!("cloud.err.need_client"));
    let device_name = {
        let mut s = inner.st();
        s.cloud_client = Some((client_id.clone(), client_secret.clone()));
        s.settings.device_name.clone()
    };
    inner.save_soon();
    let resp = inner
        .http
        .post("https://oauth.yandex.ru/device/code")
        .form(&[("client_id", client_id.as_str()), ("device_name", device_name.as_str())])
        .send()
        .await
        .context(t!("cloud.err.yandex_unreachable"))?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        bail!(t!("cloud.err.yandex_client", reason = text));
    }
    let code: DeviceCode = resp.json().await?;
    *lock(&inner.cloud_auth) = Some(CloudAuth { user_code: code.user_code.clone(), url: code.verification_url.clone() });
    *lock(&inner.cloud_error) = None;
    inner.changed();

    let inner = inner.clone();
    crate::engine::spawn(async move {
        let deadline = Instant::now() + Duration::from_secs(code.expires_in);
        let mut interval = code.interval.max(2);
        let result: Result<()> = async {
            loop {
                tokio::time::sleep(Duration::from_secs(interval)).await;
                if Instant::now() > deadline {
                    bail!(t!("cloud.err.auth_timeout"));
                }
                if lock(&inner.cloud_auth).is_none() {
                    return Ok(()); // отменили
                }
                let resp = inner
                    .http
                    .post("https://oauth.yandex.ru/token")
                    .form(&[
                        ("grant_type", "device_code"),
                        ("code", code.device_code.as_str()),
                        ("client_id", client_id.as_str()),
                        ("client_secret", client_secret.as_str()),
                    ])
                    .send()
                    .await?;
                let t: TokenResp = resp.json().await?;
                if let Some(token) = t.access_token {
                    check_access(&inner.http, &token).await?;
                    let login = disk_login(&inner.http, &token).await.unwrap_or_default();
                    inner.st().cloud = Some(CloudCreds {
                        client_id: client_id.clone(),
                        client_secret: client_secret.clone(),
                        access_token: token,
                        refresh_token: t.refresh_token.unwrap_or_default(),
                        expires_at: now_ms() + t.expires_in.unwrap_or(0) * 1000,
                        login,
                        provider: "yandex".into(),
                        ..Default::default()
                    });
                    lock(&inner.cloud_dirs).clear();
                    return Ok(());
                }
                match t.error.as_deref() {
                    Some("authorization_pending") => {}
                    Some("slow_down") => interval += 5,
                    _ => bail!(t!(
                        "cloud.err.yandex_denied",
                        reason = t.error_description.or(t.error).unwrap_or_default()
                    )),
                }
            }
        }
        .await;
        *lock(&inner.cloud_auth) = None;
        if let Err(e) = result {
            *lock(&inner.cloud_error) = Some(format!("{e:#}"));
        }
        inner.save_soon();
        inner.broadcast_hello();
        inner.kick_cloud();
        inner.changed();
    });
    Ok(())
}

/// Есть ли у полученного ключа доступ к папке приложения (без этого Диск бесполезен).
async fn check_access(http: &reqwest::Client, token: &str) -> Result<()> {
    let resp = http
        .get(API)
        .query(&[("path", "app:/"), ("limit", "1")])
        .header("Authorization", format!("OAuth {token}"))
        .send()
        .await
        .context(t!("cloud.err.yandex_unreachable"))?;
    match resp.status().as_u16() {
        200 | 404 => Ok(()),
        _ => bail!("{}", api_error(resp).await),
    }
}

async fn disk_login(http: &reqwest::Client, token: &str) -> Result<String> {
    #[derive(Deserialize)]
    struct User {
        login: Option<String>,
        display_name: Option<String>,
    }
    #[derive(Deserialize)]
    struct Disk {
        user: Option<User>,
    }
    let d: Disk = http
        .get("https://cloud-api.yandex.net/v1/disk")
        .header("Authorization", format!("OAuth {token}"))
        .send()
        .await?
        .json()
        .await?;
    Ok(d.user.and_then(|u| u.display_name.or(u.login)).unwrap_or_default())
}

/// Папка, которую синхронизирует облачный диск, или любая папка для проверки.
/// У каждого компьютера своя — другим устройствам не передаётся.
pub(crate) fn use_local(inner: &Arc<Inner>, dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    inner.st().cloud = Some(CloudCreds {
        provider: "folder".into(),
        login: dir.display().to_string(),
        local_dir: Some(dir.to_string_lossy().into_owned()),
        ..Default::default()
    });
    *lock(&inner.cloud_error) = None;
    inner.save_soon();
    inner.kick_cloud();
    inner.changed();
}

/// Подключение WebDAV: проверяет вход, создаёт свою папку и пробный файл.
pub(crate) async fn connect_webdav(inner: &Arc<Inner>, url: &str, user: &str, password: &str) -> Result<()> {
    let url = url.trim().trim_end_matches('/').to_string();
    let user = user.trim().to_string();
    anyhow::ensure!(url.starts_with("https://") || url.starts_with("http://"), t!("cloud.err.webdav_url"));
    let b = Backend::WebDav { http: inner.http.clone(), root: webdav_root(&url), user: user.clone(), pass: password.into() };
    let dirs = Dirs::default();
    b.put(&dirs, "check.txt", b"ok".to_vec()).await?;
    anyhow::ensure!(b.get("check.txt").await?.as_deref() == Some(b"ok".as_slice()), t!("cloud.err.webdav_check"));
    let _ = b.delete("check.txt").await;
    let host = reqwest::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    lock(&inner.cloud_dirs).clear();
    {
        let mut s = inner.st();
        s.cloud = Some(CloudCreds {
            provider: "webdav".into(),
            login: if user.is_empty() { host } else { format!("{user} · {host}") },
            url,
            user,
            password: password.into(),
            // Более новое подключение побеждает на других устройствах (как у Яндекса).
            expires_at: now_ms() + 365 * 24 * 3600 * 1000,
            ..Default::default()
        });
    }
    *lock(&inner.cloud_error) = None;
    inner.save_soon();
    inner.broadcast_hello();
    inner.kick_cloud();
    inner.changed();
    Ok(())
}

/// Облачные диски, которые синхронизируют папку на этом компьютере: (название, папка для данных).
pub(crate) fn detect_folders() -> Vec<(String, String)> {
    let mut roots: Vec<(String, PathBuf)> = Vec::new();
    let mut add = |name: &str, p: PathBuf| {
        if p.is_dir() && !roots.iter().any(|(_, r)| r == &p) {
            roots.push((name.to_string(), p));
        }
    };
    for var in ["OneDriveConsumer", "OneDrive", "OneDriveCommercial"] {
        if let Some(p) = std::env::var_os(var) {
            add("OneDrive", PathBuf::from(p));
        }
    }
    // Google Диск для компьютера: диск с папкой «Мой диск» (название зависит от языка).
    const MY_DRIVE: [&str; 9] =
        ["My Drive", "Мой диск", "Мій диск", "Meine Ablage", "Mi unidad", "Mon Drive", "Meu Drive", "Drive'ım", "我的云端硬盘"];
    'google: for letter in "GDEFHIJKLMNOPQRSTUVWXYZ".chars() {
        for name in MY_DRIVE {
            let p = PathBuf::from(format!("{letter}:\\{name}"));
            if p.is_dir() {
                add("Google Drive", p);
                break 'google;
            }
        }
    }
    for base in [std::env::var_os("LOCALAPPDATA"), std::env::var_os("APPDATA")].into_iter().flatten() {
        if let Ok(text) = std::fs::read_to_string(PathBuf::from(base).join("Dropbox").join("info.json"))
            && let Ok(info) = serde_json::from_str::<serde_json::Value>(&text)
        {
            for kind in ["personal", "business"] {
                if let Some(p) = info.get(kind).and_then(|k| k.get("path")).and_then(|p| p.as_str()) {
                    add("Dropbox", PathBuf::from(p));
                }
            }
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        add("iCloud Drive", home.join("iCloudDrive"));
        add(&t!("cloud.yandex"), home.join("YandexDisk"));
        add("MEGA", home.join("MEGA"));
    }
    roots
        .into_iter()
        .map(|(name, root)| (name, root.join(DATA_DIR).to_string_lossy().into_owned()))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
    use tokio::net::TcpListener;

    use super::*;

    type Files = Arc<Mutex<HashMap<String, Option<Vec<u8>>>>>;

    fn encode(path: &str) -> String {
        path.replace(' ', "%20")
    }

    /// Крошечный WebDAV-сервер в памяти: MKCOL, PUT, GET, DELETE, PROPFIND с Depth: 1.
    async fn serve(files: Files) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let good = format!("basic {}", data_encoding::BASE64.encode(b"user:pass"));
        tokio::spawn(async move {
            loop {
                let (sock, _) = listener.accept().await.unwrap();
                let (files, good) = (files.clone(), good.clone());
                tokio::spawn(async move {
                    let (r, mut w) = sock.into_split();
                    let mut r = BufReader::new(r);
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        let mut parts = line.split_whitespace();
                        let method = parts.next().unwrap_or("").to_string();
                        let key = percent_decode(parts.next().unwrap_or("")).trim_end_matches('/').to_string();
                        let (mut len, mut auth) = (0usize, false);
                        loop {
                            let mut h = String::new();
                            r.read_line(&mut h).await.unwrap();
                            let h = h.trim_end().to_ascii_lowercase();
                            if h.is_empty() {
                                break;
                            }
                            if let Some(v) = h.strip_prefix("content-length:") {
                                len = v.trim().parse().unwrap();
                            }
                            if h.strip_prefix("authorization: ") == Some(good.to_ascii_lowercase().as_str()) {
                                auth = true;
                            }
                        }
                        let mut body = vec![0; len];
                        r.read_exact(&mut body).await.unwrap();
                        let parent = key.rsplit_once('/').map(|p| p.0.to_string()).unwrap_or_default();
                        let (code, out): (u16, Vec<u8>) = if !auth {
                            (401, vec![])
                        } else {
                            let mut f = files.lock().unwrap();
                            match method.as_str() {
                                "MKCOL" if f.contains_key(&key) => (405, vec![]),
                                "MKCOL" | "PUT" if !matches!(f.get(&parent), Some(None)) => (409, vec![]),
                                "MKCOL" => {
                                    f.insert(key, None);
                                    (201, vec![])
                                }
                                "PUT" => {
                                    f.insert(key, Some(body));
                                    (201, vec![])
                                }
                                "GET" => match f.get(&key) {
                                    Some(Some(d)) => (200, d.clone()),
                                    _ => (404, vec![]),
                                },
                                "DELETE" => {
                                    let before = f.len();
                                    f.retain(|k, _| k != &key && !k.starts_with(&format!("{key}/")));
                                    (if f.len() < before { 204 } else { 404 }, vec![])
                                }
                                "PROPFIND" if f.contains_key(&key) => {
                                    let mut xml = String::from(r#"<?xml version="1.0"?><D:multistatus xmlns:D="DAV:">"#);
                                    for k in f.keys().filter(|k| *k == &key || k.rsplit_once('/').map(|p| p.0) == Some(key.as_str())) {
                                        let slash = if matches!(f.get(k), Some(None)) { "/" } else { "" };
                                        xml += &format!("<D:response><D:href>{}{slash}</D:href></D:response>", encode(k));
                                    }
                                    xml += "</D:multistatus>";
                                    (207, xml.into_bytes())
                                }
                                _ => (404, vec![]),
                            }
                        };
                        let head = format!("HTTP/1.1 {code} X\r\nContent-Length: {}\r\n\r\n", out.len());
                        if w.write_all(head.as_bytes()).await.is_err() || w.write_all(&out).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        format!("http://{addr}/dav")
    }

    fn dav(url: &str, pass: &str) -> Backend {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Backend::WebDav { http: reqwest::Client::new(), root: webdav_root(url), user: "user".into(), pass: pass.into() }
    }

    #[tokio::test]
    async fn webdav_backend() {
        let files: Files = Arc::new(Mutex::new(HashMap::from([("/dav".to_string(), None)])));
        let url = serve(files.clone()).await;
        let b = dav(&url, "pass");
        let dirs = Dirs::default();
        b.put(&dirs, "data/abc/0", b"hello".to_vec()).await.unwrap();
        b.put(&dirs, "data/abc/1", b"world".to_vec()).await.unwrap();
        assert!(files.lock().unwrap().contains_key("/dav/Family Folder sync/data/abc/1"));
        assert_eq!(b.get("data/abc/0").await.unwrap().as_deref(), Some(&b"hello"[..]));
        assert_eq!(b.get("data/abc/9").await.unwrap(), None);
        let mut names = b.list("data/abc").await.unwrap();
        names.sort();
        assert_eq!(names, ["0", "1"]);
        assert_eq!(b.list("data").await.unwrap(), ["abc"]);
        assert!(b.list("mail/none").await.unwrap().is_empty());
        b.delete("data/abc").await.unwrap();
        assert!(b.list("data/abc").await.unwrap().is_empty());
        b.delete("data/abc").await.unwrap();

        let err = dav(&url, "wrong").put(&Dirs::default(), "x", vec![]).await.unwrap_err();
        assert!(format!("{err:#}").contains(&t!("cloud.err.webdav_auth")), "{err:#}");
    }

    #[test]
    fn propfind_names() {
        let xml = r#"<d:multistatus xmlns:d="DAV:"><d:response><d:href>/remote.php/dav/files/u/Family%20Folder%20sync/mail/ab/</d:href></d:response>
            <d:response><d:href>/remote.php/dav/files/u/Family%20Folder%20sync/mail/ab/x%2B1.bin</d:href></d:response>
            <response xmlns="DAV:"><href>https://h/Family%20Folder%20sync/mail/ab/y.bin</href></response></d:multistatus>"#;
        assert_eq!(dav_names(xml, "mail/ab"), ["x+1.bin", "y.bin"]);
    }
}
