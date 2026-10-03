//! Значок у часов: меню, подсказка с состоянием, точка при новых файлах.

use std::sync::Mutex;

use obshaya_core::{UiState, t};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::{AppState, panel, shell};

static LAST: Mutex<(bool, String)> = Mutex::new((false, String::new()));

fn icon(badge: bool) -> Image<'static> {
    let bytes: &'static [u8] = if badge {
        include_bytes!("../icons/tray-badge@2x.png")
    } else {
        include_bytes!("../icons/64x64.png")
    };
    Image::from_bytes(bytes).expect("built-in icon")
}

/// Пункт «Пауза / Продолжить» меняет текст вместе с состоянием.
static PAUSE_ITEM: Mutex<Option<MenuItem<tauri::Wry>>> = Mutex::new(None);

fn menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", t!("tray.open"), true, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", t!("tray.folder"), true, None::<&str>)?;
    let paused = app.try_state::<AppState>().is_some_and(|s| s.engine.is_paused());
    let pause = MenuItem::with_id(app, "pause", if paused { t!("tray.resume") } else { t!("tray.pause") }, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", t!("tray.quit"), true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &folder, &pause, &sep, &quit])?;
    *PAUSE_ITEM.lock().unwrap_or_else(|e| e.into_inner()) = Some(pause);
    Ok(menu)
}

/// Язык сменился — меню и подсказка на новом языке.
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
    LAST.lock().unwrap_or_else(|e| e.into_inner()).1.clear();
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id("main")
        .icon(icon(false))
        .tooltip(t!("app.name"))
        .menu(&menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => panel::show_floating(app),
            "folder" => shell::reveal(&app.state::<AppState>().engine.folder()),
            "pause" => {
                let engine = &app.state::<AppState>().engine;
                if engine.is_paused() {
                    engine.resume();
                } else {
                    engine.pause(Some(60));
                }
            }
            "quit" => crate::request_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                panel::toggle_floating(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn update(app: &AppHandle, st: &UiState) {
    let new = st.incoming.iter().filter(|i| i.state == "new").count();
    let paused = st.paused_until > 0;
    static SHOWN_PAUSED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
    if SHOWN_PAUSED.swap(paused as u8, std::sync::atomic::Ordering::Relaxed) != paused as u8
        && let Some(item) = &*PAUSE_ITEM.lock().unwrap_or_else(|e| e.into_inner())
    {
        let _ = item.set_text(if paused { t!("tray.resume") } else { t!("tray.pause") });
    }
    let mut tip = t!("app.name");
    if paused {
        tip.push_str(&format!(" — {}", t!("pause.btn")));
    }
    for p in st.peers.iter().take(3) {
        let state = if p.online { t!("peer.online") } else { t!("peer.offline") };
        tip.push_str(&format!("\n«{}» — {state}", p.name));
    }
    if new > 0 {
        tip.push_str(&format!("\n{}", t!("tray.new_files", n = new)));
    }
    if st.outgoing.iter().any(|o| o.cloud == "uploading") {
        tip.push_str(&format!("\n{}", t!("tray.uploading")));
    }
    let tip: String = tip.chars().take(120).collect();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let Some(tray) = app.tray_by_id("main") else { return };
    if last.0 != (new > 0) {
        let _ = tray.set_icon(Some(icon(new > 0)));
        last.0 = new > 0;
    }
    if last.1 != tip {
        let _ = tray.set_tooltip(Some(&tip));
        last.1 = tip;
    }
}
