//! Панель рядом с Проводником: когда в нём открыта общая папка (или папка внутри неё),
//! панель встаёт сбоку от окна и двигается вместе с ним.

use std::ffi::c_void;
use std::path::Path;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IServiceProvider};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{
    IFolderView, IPersistFolder2, IShellBrowser, IShellWindows, IWebBrowser2, SHGetPathFromIDListW,
    SID_STopLevelBrowser, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST, IsIconic, IsWindow, IsWindowVisible,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos,
};
use windows::core::Interface;

use crate::AppState;
use crate::panel::{self, Mode, Panel};

pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new().name("dock".into()).spawn(move || unsafe { run(app) });
}

#[derive(PartialEq, Eq, Clone, Copy)]
struct Placed {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    topmost: bool,
}

unsafe fn run(app: AppHandle) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut shell: Option<IShellWindows> = None;
    let mut target: Option<HWND> = None;
    let mut last_scan = Instant::now() - Duration::from_secs(5);
    let mut placed: Option<Placed> = None;
    loop {
        std::thread::sleep(Duration::from_millis(40));
        let settings = app.state::<AppState>().engine.settings();
        let panel_state = app.state::<Panel>();
        if !(settings.dock_panel && settings.onboarded) {
            if panel::mode(&app) == Mode::Docked {
                panel::hide_quiet(&app);
            }
            target = None;
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        if last_scan.elapsed() >= Duration::from_millis(300) {
            last_scan = Instant::now();
            if shell.is_none() {
                shell = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok() };
            }
            target = match &shell {
                Some(sw) => match unsafe { find(sw, &settings.folder) } {
                    Ok(t) => t,
                    Err(_) => {
                        shell = None;
                        None
                    }
                },
                None => None,
            };
        }
        let target_id = target.map(|h| h.0 as isize);
        {
            let mut dismissed = panel_state.dismissed.lock().unwrap();
            if dismissed.is_some() && *dismissed != target_id {
                *dismissed = None;
            }
        }
        let dismissed = *panel_state.dismissed.lock().unwrap();
        let usable = target.filter(|h| unsafe {
            IsWindow(Some(*h)).as_bool() && IsWindowVisible(*h).as_bool() && !IsIconic(*h).as_bool()
        });
        let mode = panel::mode(&app);
        match usable {
            Some(explorer) if dismissed != target_id && mode != Mode::Floating => {
                unsafe { place(&app, explorer, &mut placed) };
                *panel_state.docked_to.lock().unwrap() = target_id;
            }
            _ => {
                if mode == Mode::Docked {
                    panel::hide_quiet(&app);
                    placed = None;
                }
            }
        }
        if panel::mode(&app) != Mode::Docked {
            placed = None;
        }
    }
}

/// Ищет окно Проводника, где (в активной вкладке) открыта общая папка или папка внутри неё.
unsafe fn find(sw: &IShellWindows, root: &Path) -> windows::core::Result<Option<HWND>> {
    unsafe {
        let count = sw.Count()?;
        let fg = GetForegroundWindow();
        let mut best: Option<HWND> = None;
        for i in 0..count {
            let Ok(disp) = sw.Item(&VARIANT::from(i)) else { continue };
            let Ok(wb) = disp.cast::<IWebBrowser2>() else { continue };
            let Ok(h) = wb.HWND() else { continue };
            let hwnd = HWND(h.0 as *mut c_void);
            let Some(path) = folder_of(&wb) else { continue };
            if !is_inside(&path, root) {
                continue;
            }
            if best.is_none() || hwnd == fg {
                best = Some(hwnd);
            }
        }
        Ok(best)
    }
}

unsafe fn folder_of(wb: &IWebBrowser2) -> Option<String> {
    unsafe {
        let sp: IServiceProvider = wb.cast().ok()?;
        let sb: IShellBrowser = sp.QueryService(&SID_STopLevelBrowser).ok()?;
        // В Windows 11 у Проводника вкладки: невидимое окно вкладки — не активная вкладка.
        if let Ok(tab) = sb.GetWindow()
            && !IsWindowVisible(tab).as_bool()
        {
            return None;
        }
        let view = sb.QueryActiveShellView().ok()?;
        let fv: IFolderView = view.cast().ok()?;
        let pf: IPersistFolder2 = fv.GetFolder().ok()?;
        let pidl = pf.GetCurFolder().ok()?;
        let mut buf = [0u16; 260];
        let ok = SHGetPathFromIDListW(pidl, &mut buf).as_bool();
        CoTaskMemFree(Some(pidl as _));
        if !ok {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

fn is_inside(path: &str, root: &Path) -> bool {
    let p = path.trim_end_matches('\\').to_lowercase();
    let r = root.to_string_lossy().trim_end_matches('\\').to_lowercase();
    p == r || p.starts_with(&format!("{r}\\"))
}

unsafe fn place(app: &AppHandle, explorer: HWND, placed: &mut Option<Placed>) {
    let Some(win) = panel::window(app) else { return };
    let Ok(raw) = win.hwnd() else { return };
    let panel_hwnd = HWND(raw.0 as *mut c_void);
    unsafe {
        let mut r = RECT::default();
        if DwmGetWindowAttribute(explorer, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as _, size_of::<RECT>() as u32).is_err() {
            let _ = GetWindowRect(explorer, &mut r);
        }
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromWindow(explorer, MONITOR_DEFAULTTONEAREST), &mut mi);
        let wa = mi.rcWork;
        let scale = GetDpiForWindow(explorer).max(96) as f64 / 96.0;
        let px = |v: f64| (v * scale).round() as i32;
        let w = px(360.0);
        let gap = px(6.0);
        let (x, y, h) = if wa.right - r.right >= w + gap {
            (r.right + gap, r.top, r.bottom - r.top)
        } else if r.left - wa.left >= w + gap {
            (r.left - gap - w, r.top, r.bottom - r.top)
        } else {
            // Места сбоку нет (окно развёрнуто) — внутри окна, у правого края.
            let top = r.top + px(96.0);
            (r.right - w - px(12.0), top, r.bottom - top - px(12.0))
        };
        let h = h.max(px(440.0)).min(wa.bottom - wa.top);
        let y = y.min(wa.bottom - h).max(wa.top);
        // Пока активен Проводник (или сама панель) — панель поверх него. Фоновой программе
        // Windows не даёт подняться над активным окном обычным способом, поэтому «поверх всех»,
        // а при переходе в другую программу — сразу под окно Проводника.
        let fg = GetForegroundWindow();
        let topmost = fg == explorer || fg == panel_hwnd;
        let want = Placed { x, y, w, h, topmost };
        let visible = IsWindowVisible(panel_hwnd).as_bool();
        if *placed == Some(want) && visible {
            return;
        }
        tracing::debug!("панель у Проводника: {x},{y} {w}x{h} поверх={topmost} fg={:?} explorer={:?}", fg.0, explorer.0);
        let flags = SWP_NOACTIVATE | SWP_SHOWWINDOW;
        if topmost {
            let _ = SetWindowPos(panel_hwnd, Some(HWND_TOPMOST), x, y, w, h, flags);
        } else {
            let _ = SetWindowPos(panel_hwnd, Some(HWND_NOTOPMOST), x, y, w, h, flags);
            let _ = SetWindowPos(panel_hwnd, Some(explorer), x, y, w, h, flags);
        }
        *placed = Some(want);
    }
    panel::set_mode(app, Mode::Docked);
}
