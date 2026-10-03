//! Где лежат данные программы и пользовательские настройки.

use std::path::PathBuf;

use anyhow::{Context, Result};
use iroh::SecretKey;
use serde::{Deserialize, Serialize};

/// Папка данных: `%APPDATA%\ObshayaPapka` или `OBSHAYA_DATA_DIR` (для второго экземпляра в тестах).
pub fn default_data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("OBSHAYA_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = std::env::var_os("APPDATA").context("APPDATA is not set")?;
    Ok(PathBuf::from(base).join("ObshayaPapka"))
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CloudMode {
    /// Только если получатель не в сети или долго не забирает файл.
    #[default]
    WhenNeeded,
    Always,
    Never,
}

/// Что принимать без вопроса «Получить?».
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AutoAccept {
    /// Всегда спрашивать.
    #[default]
    Off,
    Photos,
    All,
}

/// Как сообщать о новых файлах и событиях.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NotifyVia {
    /// Шторка сверху экрана (во время игр и фильмов на весь экран — уведомление Windows).
    #[default]
    Island,
    Windows,
    Both,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub device_name: String,
    pub folder: PathBuf,
    pub onboarded: bool,
    /// Язык окна: код ("ru", "en", …) или пусто — как в Windows.
    pub language: String,
    /// Что принимать без вопроса. None — настройки до версии 1.1 (переводятся при запуске).
    pub auto_accept: Option<AutoAccept>,
    /// Предел размера для автоприёма (МБ); 0 — без предела.
    pub auto_accept_mb: u64,
    /// Принимать без вопроса и программы (.exe и т. п.).
    pub auto_accept_exe: bool,
    /// Ограничение скорости в КБ/с для каждого направления; 0 — без ограничения.
    pub speed_limit_kbps: u64,
    pub cloud_mode: CloudMode,
    pub dock_panel: bool,
    pub autostart: bool,
    pub notify_delivered: bool,
    /// Ставить новую версию самому, когда ничего не передаётся (иначе — по кнопке).
    pub auto_update: bool,
    /// Шторка сверху экрана: выезжает у верхнего края и принимает перетащенные файлы.
    pub island: bool,
    pub notify_via: NotifyVia,
    /// Какую экскурсию с Папычем уже показали (`TOUR_VERSION`); меньше — показать после запуска.
    pub tour_seen: u32,
    /// Подсказки Папыча в облачке.
    pub tips: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: computer_name(),
            folder: default_folder(),
            onboarded: false,
            language: String::new(),
            // Не задано: при запуске станет «всегда спрашивать» (или «только фото» для настроек до 1.1).
            auto_accept: None,
            auto_accept_mb: 0,
            auto_accept_exe: false,
            speed_limit_kbps: 0,
            cloud_mode: CloudMode::WhenNeeded,
            dock_panel: true,
            autostart: true,
            notify_delivered: true,
            auto_update: true,
            island: true,
            notify_via: NotifyVia::Island,
            tour_seen: 0,
            tips: true,
        }
    }
}

/// Сколько места на диске оставлять свободным при автоприёме (иначе — спросить).
pub const AUTO_ACCEPT_RESERVE: u64 = 5 * 1024 * 1024 * 1024;

pub fn default_folder() -> PathBuf {
    if let Some(dir) = std::env::var_os("OBSHAYA_FOLDER") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"));
    home.join(crate::t!("folder.default"))
}

fn computer_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| crate::t!("device.default"))
}

pub fn load_or_create_key(path: &std::path::Path) -> Result<SecretKey> {
    if let Ok(bytes) = std::fs::read(path)
        && let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice())
    {
        return Ok(SecretKey::from_bytes(&arr));
    }
    let key = SecretKey::generate();
    crate::util::write_atomic(path, &key.to_bytes())?;
    Ok(key)
}
