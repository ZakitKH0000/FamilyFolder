//! Шторка сверху экрана с Папычем: выезжает, когда курсор упирается в верхний край по центру
//! или когда туда тащат файлы, и показывает новые файлы с кнопками «Получить / Отклонить».
//!
//! Это отдельное прозрачное окно поверх всех (`island.html`). Здесь — только окно и курсор:
//! поток следит за мышью, открывает шторку и передаёт положение курсора (глаза Папыча смотрят
//! на него и в окне программы). Когда спрятаться, решает сама страница (`island_close`).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use obshaya_core::NotifyVia;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect, IsWindowVisible, SM_SWAPBUTTON,
};

use crate::AppState;

pub const LABEL: &str = "island";
/// Размер окна шторки (логические пиксели): с запасом на самую большую шторку и тень.
const WIDTH: f64 = 480.0;
const HEIGHT: f64 = 360.0;

static OPEN: AtomicBool = AtomicBool::new(false);
/// Страница шторки загрузилась; до этого события копятся в PENDING.
static READY: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Vec<Open>> = Mutex::new(Vec::new());

pub fn create(app: &AppHandle, data_dir: &Path) -> tauri::Result<()> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("island.html".into()))
        .title("Papych")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        // Не забирать фокус у программы, в которой человек работает.
        .focused(false)
        .focusable(false)
        // Свой OLE-приёмник на родительском HWND и WebView: одинаковый знак копирования
        // на всей видимой шторке, в том числе при появлении окна во время перетаскивания.
        .disable_drag_drop_handler()
        .data_directory(data_dir.join("webview"))
        .build()?;
    crate::drop_target::install(&w)?;
    let app = app.clone();
    let _ = std::thread::Builder::new().name("island".into()).spawn(move || watch(app));
    Ok(())
}

fn hwnd(w: &tauri::WebviewWindow) -> Option<HWND> {
    w.hwnd().ok().map(|h| HWND(h.0 as *mut c_void))
}

fn island_hwnd(app: &AppHandle) -> Option<HWND> {
    app.get_webview_window(LABEL).as_ref().and_then(hwnd)
}

/// Куда сообщать о событии: в шторку, уведомлением Windows или туда и туда.
pub struct Route {
    pub island: bool,
    pub toast: bool,
}

pub fn route(app: &AppHandle) -> Route {
    let s = app.state::<AppState>().engine.settings();
    let island = s.island && s.onboarded && s.notify_via != NotifyVia::Windows;
    // Игра или фильм на весь экран — шторка помешает; уведомление тихо ляжет в центр уведомлений.
    if island && fullscreen() {
        return Route { island: false, toast: true };
    }
    Route { island, toast: !island || s.notify_via == NotifyVia::Both }
}

pub fn fullscreen() -> bool {
    matches!(
        unsafe { SHQueryUserNotificationState() },
        Ok(s) if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE
    )
}

#[derive(Serialize, Clone)]
pub struct Open {
    reason: &'static str,
    data: serde_json::Value,
}

/// Показать шторку (на мониторе, где курсор) с событием для страницы.
pub fn show(app: &AppHandle, reason: &'static str, data: serde_json::Value) {
    let Some(win) = app.get_webview_window(LABEL) else { return };
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    let (mon, scale) = monitor(p);
    let size = PhysicalSize::new((WIDTH * scale) as u32, (HEIGHT * scale) as u32);
    let pos = PhysicalPosition::new(mon.left + (mon.right - mon.left - size.width as i32) / 2, mon.top);
    // Показывает сам Tauri (иначе он считает окно скрытым и прячет его при следующей настройке).
    // Размер — дважды: переезд на монитор с другим масштабом меняет размер окна.
    let _ = win.set_position(pos);
    let _ = win.set_size(size);
    let _ = win.set_position(pos);
    let _ = win.set_ignore_cursor_events(false);
    if !OPEN.swap(true, Ordering::Relaxed) {
        let _ = win.show();
        let _ = win.set_always_on_top(true);
        // WebView2 может создать дополнительные HWND при первом показе.
        let _ = crate::drop_target::install(&win);
    }
    let open = Open { reason, data };
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    if READY.load(Ordering::Relaxed) {
        let _ = app.emit_to(LABEL, "island-open", open);
    } else {
        pending.push(open);
    }
}

fn monitor(p: POINT) -> (RECT, f64) {
    unsafe {
        let m = MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(m, &mut mi);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        (mi.rcMonitor, dx.max(96) as f64 / 96.0)
    }
}

/// Положение курсора относительно окна в логических пикселях — как в странице.
fn client_point(h: HWND, p: POINT) -> (f64, f64) {
    unsafe {
        let mut origin = POINT::default();
        let _ = ClientToScreen(h, &mut origin);
        let scale = GetDpiForWindow(h).max(96) as f64 / 96.0;
        (((p.x - origin.x) as f64 / scale), ((p.y - origin.y) as f64 / scale))
    }
}

fn primary_down() -> bool {
    let vk = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 { VK_RBUTTON } else { VK_LBUTTON };
    let state = unsafe { GetAsyncKeyState(vk.0 as i32) };
    state < 0
}

/// Нажатие мыши: где и какое окно было впереди (чтобы не путать перетаскивание окна с файлами).
struct Press {
    at: Instant,
    pos: POINT,
    in_zone: bool,
    fg: HWND,
    fg_rect: RECT,
    opened: bool,
}

fn watch(app: AppHandle) {
    let main = app.get_webview_window("main").as_ref().and_then(hwnd);
    let mut press: Option<Press> = None;
    let mut edge_since: Option<Instant> = None;
    let mut last_sent: Option<(POINT, bool)> = None;
    let mut main_visible = false;
    let mut was_open = false;
    loop {
        std::thread::sleep(Duration::from_millis(33));
        let settings = app.state::<AppState>().engine.settings();
        let mut p = POINT::default();
        if unsafe { GetCursorPos(&mut p) }.is_err() {
            continue;
        }
        let down = primary_down();
        let open = OPEN.load(Ordering::Relaxed);
        if open && !was_open {
            last_sent = None; // только что открылась — сразу сказать, где курсор
        }
        was_open = open;

        // Глаза Папыча в окне программы: курсор, пока окно видно.
        if let Some(m) = main {
            let vis = unsafe { IsWindowVisible(m) }.as_bool();
            if vis != main_visible {
                main_visible = vis;
                let _ = app.emit_to("main", "papych-active", vis);
            }
        }
        let moved = last_sent.is_none_or(|(lp, ld)| lp.x != p.x || lp.y != p.y || ld != down);
        if moved && (open || main_visible) {
            last_sent = Some((p, down));
            if open && let Some(h) = island_hwnd(&app) {
                let (x, y) = client_point(h, p);
                let _ = app.emit_to(LABEL, "island-cursor", (x, y, down));
            }
            if main_visible && let Some(m) = main {
                let (x, y) = client_point(m, p);
                let _ = app.emit_to("main", "cursor", (x, y));
            }
        }

        if !(settings.island && settings.onboarded) {
            if open {
                close(&app);
            }
            press = None;
            continue;
        }
        let (mon, scale) = monitor(p);
        let cx = (mon.left + mon.right) / 2;
        let dx = ((p.x - cx).abs() as f64) / scale;
        let dy = ((p.y - mon.top) as f64) / scale;
        let in_zone = dx < 280.0 && dy < 150.0;

        // Перетаскивание к центру сверху. Окно, которое тащат за заголовок (и прилипание окон
        // к краю), не считается: его прямоугольник при этом меняется.
        if down {
            let pr = press.get_or_insert_with(|| unsafe {
                let fg = GetForegroundWindow();
                let mut r = RECT::default();
                let _ = GetWindowRect(fg, &mut r);
                Press { at: Instant::now(), pos: p, in_zone, fg, fg_rect: r, opened: false }
            });
            let travelled = (p.x - pr.pos.x).abs() + (p.y - pr.pos.y).abs();
            if !pr.opened && !open && !pr.in_zone && in_zone && pr.at.elapsed() >= Duration::from_millis(150) && travelled > 30 {
                let mut r = RECT::default();
                let moving = unsafe { GetWindowRect(pr.fg, &mut r) }.is_ok() && r != pr.fg_rect;
                if !moving && !fullscreen() {
                    pr.opened = true;
                    show(&app, "drag", serde_json::Value::Null);
                }
            }
            // Ушли из зоны — шторка закрылась сама; вернулись — можно открыть снова.
            if pr.opened && !in_zone && !open {
                pr.opened = false;
            }
            edge_since = None;
            continue;
        }
        press = None;

        // Курсор упёрся в верхний край по центру — шторка выглядывает.
        if !open && dy <= 1.0 && dx < 150.0 {
            let since = *edge_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_millis(180) && !fullscreen() {
                edge_since = None;
                show(&app, "peek", serde_json::Value::Null);
            }
        } else {
            edge_since = None;
        }
    }
}

fn close(app: &AppHandle) {
    OPEN.store(false, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/// Экскурсия показывает шторку: "peek", "drag" или "close".
pub fn demo(app: &AppHandle, kind: &str) {
    if kind == "close" {
        if OPEN.load(Ordering::Relaxed) {
            let _ = app.emit_to(LABEL, "island-open", Open { reason: "demo", data: serde_json::json!({ "kind": "close" }) });
        }
        return;
    }
    show(app, "demo", serde_json::json!({ "kind": kind }));
}

/// Страница спрятала шторку (анимация закончилась).
#[tauri::command]
pub fn island_close(app: AppHandle) {
    close(&app);
}

/// Прозрачная часть окна пропускает мышь к окнам под ней.
#[tauri::command]
pub fn island_pass(app: AppHandle, pass: bool) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.set_ignore_cursor_events(pass);
    }
}

/// Страница загрузилась: отдать то, что пришло раньше неё.
#[tauri::command]
pub fn island_ready() -> Vec<Open> {
    let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    READY.store(true, Ordering::Relaxed);
    std::mem::take(&mut *pending)
}

/// Файлы бросили в шторку или на окно: `peers` пусто — всем, иначе только им.
/// Возвращает, сколько вещей ляжет в папку (0 — всё уже там).
#[tauri::command]
pub fn send_dropped(app: AppHandle, paths: Vec<String>, peers: Vec<String>) -> usize {
    crate::send::drop_in(&app, paths.into_iter().map(PathBuf::from).collect(), peers)
}

#[tauri::command]
pub fn show_main(app: AppHandle) {
    crate::panel::show_floating(&app);
}
