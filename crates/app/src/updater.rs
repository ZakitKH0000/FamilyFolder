//! Обновление внутри семьи: проверка подписи разработчика и установка.
//!
//! Двигатель приносит установщик от другого устройства (`Event::UpdateReady`), здесь проверяется
//! подпись (minisign, ключ в `updater.pub`; подписывает scripts/build-release.ps1). Свой установщик
//! (`update\installer.exe`, его кладёт установщик программы) тоже проверяется и отдаётся другим.
//! Установка — тот же установщик в тихом режиме: он закрывает программу и запускает её снова.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use data_encoding::BASE64;
use obshaya_core::{Engine, t, version_newer};
use tauri::{AppHandle, Manager};

use crate::{AppState, notify};

const PUBKEY: &str = include_str!("../updater.pub");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Подпись разработчика сходится с файлом.
pub fn verify(file: &Path, sig_b64: &str) -> bool {
    let check = || -> anyhow::Result<()> {
        let pk = minisign_verify::PublicKey::decode(&String::from_utf8(BASE64.decode(PUBKEY.trim().as_bytes())?)?)?;
        let sig = minisign_verify::Signature::decode(&String::from_utf8(BASE64.decode(sig_b64.trim().as_bytes())?)?)?;
        pk.verify(&std::fs::read(file)?, &sig, true)?;
        Ok(())
    };
    match check() {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("подпись установщика {} не сошлась: {e:#}", file.display());
            false
        }
    }
}

fn sig_of(file: &Path) -> Option<String> {
    let mut p = file.as_os_str().to_owned();
    p.push(".sig");
    std::fs::read_to_string(PathBuf::from(p)).ok()
}

fn remove_with_sig(file: &Path) {
    let _ = std::fs::remove_file(file);
    let mut p = file.as_os_str().to_owned();
    p.push(".sig");
    let _ = std::fs::remove_file(PathBuf::from(p));
}

/// При запуске: свой установщик — источник для других; скачанные раньше — готовы к установке.
pub fn startup(app: &AppHandle) {
    let engine = app.state::<AppState>().engine.clone();
    let dir = engine.data_dir().join("update");
    tauri::async_runtime::spawn(async move {
        let own = dir.join("installer.exe");
        if let Some(sig) = sig_of(&own).filter(|s| verify(&own, s))
            && let Err(e) = engine.set_update_package(APP_VERSION, own, &sig).await
        {
            tracing::warn!("свой установщик: {e:#}");
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { return };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(version) = name.strip_prefix("setup-").and_then(|n| n.strip_suffix(".exe")) else { continue };
            match sig_of(&path) {
                Some(sig) if version_newer(version, APP_VERSION) && verify(&path, &sig) => {
                    let _ = engine.set_update_package(version, path, &sig).await;
                }
                _ => remove_with_sig(&path),
            }
        }
    });
}

/// Двигатель скачал установщик — проверить подпись.
pub fn downloaded(app: &AppHandle, version: String, path: PathBuf, sig: String, hash: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let engine = app.state::<AppState>().engine.clone();
        if !verify(&path, &sig) {
            remove_with_sig(&path);
            engine.reject_update(&hash);
            return;
        }
        if let Err(e) = engine.set_update_package(&version, path, &sig).await {
            tracing::warn!("обновление {version}: {e:#}");
            return;
        }
        tracing::info!("обновление {version} готово к установке");
        if !engine.settings().auto_update {
            notify::update_ready(&app, &version);
        }
    });
}

/// Установить готовое обновление: установщик закроет программу и запустит новую версию.
pub fn install(app: &AppHandle) -> Result<(), String> {
    let engine = app.state::<AppState>().engine.clone();
    let (version, path) = engine.update_ready().ok_or_else(|| t!("update.none"))?;
    tracing::info!("ставлю обновление {version}");
    std::process::Command::new(&path)
        .args(["/S", "/UPDATE", "/R", "/ARGS", "--updated"])
        .creation_flags(0x0000_0008) // DETACHED_PROCESS: живёт после выхода программы
        .spawn()
        .map_err(|e| t!("update.failed", reason = e))?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        engine.shutdown().await;
        app.exit(0);
    });
    Ok(())
}

/// Автообновление: ставить, когда ничего не передаётся и окно закрыто.
pub fn auto_loop(app: &AppHandle) {
    let app = app.clone();
    let started = Instant::now();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            let engine: Engine = app.state::<AppState>().engine.clone();
            let window_open = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false));
            if engine.settings().auto_update
                && engine.update_ready().is_some()
                && started.elapsed() > Duration::from_secs(120)
                && !engine.busy()
                && !engine.is_paused()
                && !window_open
            {
                if let Err(e) = install(&app) {
                    tracing::warn!("автообновление: {e}");
                }
                return;
            }
        }
    });
}
