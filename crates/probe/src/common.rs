//! Общие части отправителя и получателя: сеть, служебные сообщения, вывод прогресса.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use iroh::endpoint::{Connection, presets};
use iroh::{Endpoint, SecretKey};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const ALPN: &[u8] = b"obshaya-papka/probe/1";
pub const PROTOCOL_VERSION: u8 = 1;
pub const CHUNK: usize = 1 << 20;

/// Описание файла, которое отправитель передаёт получателю до начала передачи.
#[derive(Serialize, Deserialize, Debug)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    /// BLAKE3 всего файла, hex.
    pub hash: String,
}

/// Ключ отправителя хранится в %APPDATA%, чтобы его код не менялся между запусками
/// (нужно для докачки после перезапуска). Получатель берёт новый ключ каждый раз,
/// поэтому обе роли можно запускать на одном компьютере.
fn load_or_create_key() -> Result<SecretKey> {
    let base = std::env::var_os("APPDATA").context("не найдена папка APPDATA")?;
    let dir = PathBuf::from(base).join("obshaya-papka");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("probe.key");
    if let Ok(bytes) = std::fs::read(&path)
        && let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice())
    {
        return Ok(SecretKey::from_bytes(&arr));
    }
    let key = SecretKey::generate();
    std::fs::write(&path, key.to_bytes())?;
    Ok(key)
}

pub async fn bind(persistent_key: bool) -> Result<Endpoint> {
    let key = if persistent_key {
        load_or_create_key()?
    } else {
        SecretKey::generate()
    };
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await?;
    if tokio::time::timeout(Duration::from_secs(20), endpoint.online())
        .await
        .is_err()
    {
        println!("  (сервер-посредник долго не отвечает — продолжаю без него)");
    }
    Ok(endpoint)
}

/// Как сейчас идут данные: напрямую между компьютерами или через ретранслятор.
pub fn route(conn: &Connection) -> &'static str {
    match conn.paths().iter().find(|p| p.is_selected()) {
        Some(p) if p.is_ip() => "напрямую",
        Some(p) if p.is_relay() => "через ретранслятор",
        _ => "устанавливается",
    }
}

pub async fn write_msg<T: Serialize>(w: &mut (impl AsyncWrite + Unpin), msg: &T) -> Result<()> {
    let data = serde_json::to_vec(msg)?;
    w.write_u32(data.len() as u32).await?;
    w.write_all(&data).await?;
    Ok(())
}

pub async fn read_msg<T: DeserializeOwned>(r: &mut (impl AsyncRead + Unpin)) -> Result<T> {
    let len = r.read_u32().await?;
    anyhow::ensure!(len <= 64 * 1024, "слишком большое служебное сообщение");
    let mut buf = vec![0; len as usize];
    r.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf)?)
}

/// Дочитывает первые `len` байт файла в `hasher` (для докачки и для подсчёта суммы).
pub async fn hash_prefix(path: &Path, len: u64, hasher: &mut blake3::Hasher, label: &str) -> Result<()> {
    let file = tokio::fs::File::open(path).await?;
    let mut reader = file.take(len);
    let mut buf = vec![0u8; CHUNK];
    let mut done = 0;
    let mut progress = Progress::new(label, len, 0);
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        progress.update(done, || "");
    }
    progress.finish();
    anyhow::ensure!(done == len, "файл изменился во время чтения");
    Ok(())
}

pub struct Progress {
    label: String,
    total: u64,
    start_done: u64,
    start: Instant,
    last_print: Option<Instant>,
}

impl Progress {
    pub fn new(label: &str, total: u64, already: u64) -> Self {
        Self {
            label: label.to_string(),
            total,
            start_done: already,
            start: Instant::now(),
            last_print: None,
        }
    }

    pub fn update(&mut self, done: u64, note: impl FnOnce() -> &'static str) {
        let now = Instant::now();
        if let Some(last) = self.last_print
            && now - last < Duration::from_millis(250)
            && done < self.total
        {
            return;
        }
        self.last_print = Some(now);

        let frac = if self.total == 0 { 1.0 } else { done as f64 / self.total as f64 };
        let filled = (frac * 10.0).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(10 - filled.min(10));
        let secs = self.start.elapsed().as_secs_f64().max(0.001);
        let speed = (done - self.start_done) as f64 / secs;
        let eta = if speed > 0.0 && done < self.total {
            format!("  ~{}", fmt_eta((self.total - done) as f64 / speed))
        } else {
            String::new()
        };
        let note = note();
        let note = if note.is_empty() { String::new() } else { format!("  {note}") };
        let line = format!(
            "{} [{bar}] {:>3}%  {} из {}  {}/с{eta}{note}",
            self.label,
            (frac * 100.0) as u32,
            fmt_size(done),
            fmt_size(self.total),
            fmt_size(speed as u64),
        );
        print!("\r{line:<100}");
        let _ = std::io::stdout().flush();
    }

    pub fn finish(&self) {
        println!();
    }
}

pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let num = if i == 0 { bytes.to_string() } else { format!("{v:.1}").replace('.', ",") };
    format!("{num} {}", UNITS[i])
}

fn fmt_eta(secs: f64) -> String {
    let s = secs.round() as u64;
    match s {
        0..60 => format!("{s} с"),
        60..3600 => format!("{} мин", s / 60),
        _ => format!("{} ч {} мин", s / 3600, (s % 3600) / 60),
    }
}

/// Читает строку из консоли, не блокируя сетевые задачи.
pub async fn prompt(text: &str) -> Result<String> {
    print!("{text}");
    let _ = std::io::stdout().flush();
    let line = tokio::task::spawn_blocking(|| {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).map(|_| s)
    })
    .await??;
    Ok(line.trim().to_string())
}

/// Запущены ли мы в окне консоли (а не с перенаправленным выводом, как в автотесте).
pub fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Кладёт текст в буфер обмена Windows через встроенный clip.exe.
pub fn copy_to_clipboard(text: &str) -> bool {
    use std::process::{Command, Stdio};
    if !interactive() {
        return false;
    }
    let Ok(mut child) = Command::new("clip").stdin(Stdio::piped()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}
