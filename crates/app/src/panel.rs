//! Окно-панель: всплывает у часов или «приклеивается» к Проводнику.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Hidden,
    /// Открыта вручную (значок у часов, уведомление) — стоит, где поставили.
    Floating,
    /// Рядом с окном Проводника, где открыта общая папка.
    Docked,
}

pub struct Panel {
    pub mode: Mutex<Mode>,
    /// Окно Проводника, рядом с которым панель закрыли — не показывать снова, пока папку не откроют заново.
    pub dismissed: Mutex<Option<isize>>,
    pub docked_to: Mutex<Option<isize>>,
}

impl Panel {
    pub fn new() -> Self {
        Self { mode: Mutex::new(Mode::Hidden), dismissed: Mutex::new(None), docked_to: Mutex::new(None) }
    }
}

pub fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

pub fn mode(app: &AppHandle) -> Mode {
    *app.state::<Panel>().mode.lock().unwrap()
}

pub fn set_mode(app: &AppHandle, m: Mode) {
    *app.state::<Panel>().mode.lock().unwrap() = m;
}

pub fn show_floating(app: &AppHandle) {
    let Some(w) = window(app) else { return };
    let scale = w.scale_factor().unwrap_or(1.0);
    let width = (380.0 * scale) as i32;
    let height = (620.0 * scale) as i32;
    let margin = (12.0 * scale) as i32;
    let wa = crate::shell::work_area();
    if mode(app) != Mode::Floating {
        let _ = w.set_size(PhysicalSize::new(width as u32, height as u32));
        let _ = w.set_position(PhysicalPosition::new(wa.right - width - margin, wa.bottom - height - margin));
    }
    set_mode(app, Mode::Floating);
    // После стыковки с Проводником окно могло остаться «поверх всех».
    let _ = w.set_always_on_top(false);
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

pub fn toggle_floating(app: &AppHandle) {
    let visible = window(app).and_then(|w| w.is_visible().ok()).unwrap_or(false);
    if visible && mode(app) == Mode::Floating {
        hide(app);
    } else {
        show_floating(app);
    }
}

/// Скрыть по просьбе пользователя. Если панель была у Проводника — не возвращать её к этому окну.
pub fn hide(app: &AppHandle) {
    let state = app.state::<Panel>();
    if *state.mode.lock().unwrap() == Mode::Docked {
        *state.dismissed.lock().unwrap() = *state.docked_to.lock().unwrap();
    }
    hide_quiet(app);
}

/// Скрыть без запоминания (Проводник закрыли или свернули).
pub fn hide_quiet(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.hide();
        // У Проводника панель показывается напрямую через Windows, и Tauri может
        // считать её уже скрытой — поэтому прячем и напрямую.
        if let Ok(h) = w.hwnd() {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};
            unsafe {
                let _ = ShowWindow(HWND(h.0 as *mut std::ffi::c_void), SW_HIDE);
            }
        }
    }
    set_mode(app, Mode::Hidden);
}
