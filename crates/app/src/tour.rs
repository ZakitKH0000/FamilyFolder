//! Экскурсия с Папычем: после установки и после обновления до версии с новой экскурсией Папыч
//! выпрыгивает из окна программы на экран и с указкой показывает шторку, окно, сообщения и паузу.
//! Это прозрачное окно поверх всего монитора (`tour.html`); показать шторку помогает `island::demo`.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::{AppState, island, panel};

/// Номер экскурсии: увеличить, когда в ней появится новое, — тогда её покажут ещё раз после обновления.
pub const VERSION: u32 = 2;
pub const LABEL: &str = "tour";

/// При запуске: экскурсию ещё не видели — показать, когда человек за компьютером.
pub fn startup(app: &AppHandle) {
    let s = app.state::<AppState>().engine.settings();
    if s.onboarded && s.tour_seen < VERSION && std::env::var_os("OBSHAYA_DATA_DIR").is_none() {
        start_when_active(app.clone(), Duration::from_secs(4));
    }
}

/// Дождаться, что мышь шевелится (человек тут, а не ушёл после обновления ночью) и нет игры
/// или фильма на весь экран, — и начать.
pub fn start_when_active(app: AppHandle, delay: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let mut last = cursor();
        loop {
            std::thread::sleep(Duration::from_millis(700));
            let now = cursor();
            if (now.x, now.y) != (last.x, last.y) && !island::fullscreen() {
                break;
            }
            last = now;
        }
        start(&app);
    });
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    p
}

/// Начать экскурсию. Долго (окно создаётся и ждёт окно программы) — не из потока окна.
pub fn start(app: &AppHandle) {
    if app.get_webview_window(LABEL).is_some() {
        return;
    }
    // Папыч выпрыгивает из окна программы — значит, окно должно быть на экране.
    panel::show_floating(app);
    std::thread::sleep(Duration::from_millis(700));
    let Some(main) = panel::window(app) else { return };
    let Some(monitor) = main.current_monitor().ok().flatten() else { return };
    let (mpos, msize, scale) = (*monitor.position(), *monitor.size(), monitor.scale_factor());
    let wpos = main.outer_position().unwrap_or_default();
    let wsize = main.outer_size().unwrap_or_default();
    // Где окно программы — в пикселях страницы экскурсии (от левого верхнего угла монитора).
    let geo = serde_json::json!({
        "main": [
            (wpos.x - mpos.x) as f64 / scale,
            (wpos.y - mpos.y) as f64 / scale,
            wsize.width as f64 / scale,
            wsize.height as f64 / scale,
        ],
    });
    let data_dir = app.state::<AppState>().data_dir.clone();
    let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("tour.html".into()))
        .title("Papych")
        .initialization_script(format!("window.TOUR = {geo};"))
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .data_directory(data_dir.join("webview"))
        .build();
    let w = match built {
        Ok(w) => w,
        Err(e) => return tracing::warn!("экскурсия не открылась: {e}"),
    };
    // Размер — дважды: переезд на монитор с другим масштабом меняет размер окна.
    let _ = w.set_position(PhysicalPosition::new(mpos.x, mpos.y));
    let _ = w.set_size(PhysicalSize::new(msize.width, msize.height));
    let _ = w.set_position(PhysicalPosition::new(mpos.x, mpos.y));
    let _ = w.set_size(PhysicalSize::new(msize.width, msize.height));
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit_to("main", "tour", true);
    tracing::info!("экскурсия с Папычем");
}

/// Экскурсия закончилась или её пропустили: больше не показывать сама, Папыч — обратно в окно.
pub fn finish(app: &AppHandle) {
    let engine = app.state::<AppState>().engine.clone();
    let mut s = engine.settings();
    if s.tour_seen < VERSION {
        s.tour_seen = VERSION;
        let _ = engine.set_settings(s);
    }
    island::demo(app, "close");
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
    let _ = app.emit_to("main", "tour", false);
}

/// Кнопка «Показать экскурсию» в настройках.
#[tauri::command]
pub fn tour_start(app: AppHandle) {
    std::thread::spawn(move || start(&app));
}

#[tauri::command]
pub fn tour_done(app: AppHandle) {
    finish(&app);
}

/// Экскурсия показывает шторку: "peek" — выглянула, "drag" — раскрыта для файлов, "close".
#[tauri::command]
pub fn island_demo(app: AppHandle, kind: String) {
    island::demo(&app, &kind);
}
