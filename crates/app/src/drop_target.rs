//! Windows OLE: шторка принимает только файлы, показывает «копировать», не «переместить».
//! Приёмник установлен и на HWND окна, и на дочерние HWND WebView2.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, POINTL};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl,
    RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
use windows::core::{BOOL, Ref, implement};

static BOUNDS: Mutex<(f64, f64, f64, f64)> = Mutex::new((0., 0., 0., 0.));
thread_local! {
    static REGISTERED: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
}

#[tauri::command]
pub fn island_drop_bounds(x: f64, y: f64, width: f64, height: f64) {
    *BOUNDS.lock().unwrap_or_else(|e| e.into_inner()) = (x, y, width, height);
}

#[implement(IDropTarget)]
struct FileDropTarget {
    app: AppHandle,
    root: HWND,
    files: Cell<bool>,
}

impl FileDropTarget {
    fn point(&self, pt: &POINTL) -> POINT {
        let mut p = POINT { x: pt.x, y: pt.y };
        let _ = unsafe { ScreenToClient(self.root, &mut p) };
        p
    }

    fn accepts(&self, p: POINT) -> bool {
        let scale = unsafe { GetDpiForWindow(self.root) }.max(96) as f64 / 96.;
        let (x, y, w, h) = *BOUNDS.lock().unwrap_or_else(|e| e.into_inner());
        let (px, py) = (p.x as f64 / scale, p.y as f64 / scale);
        w > 0. && h > 0. && px >= x && px <= x + w && py >= y && py <= y + h
    }

    fn emit(&self, kind: &str, p: POINT, paths: Vec<String>) {
        let _ = self.app.emit_to(
            crate::island::LABEL,
            "island-drag",
            serde_json::json!({
                "type": kind, "position": { "x": p.x, "y": p.y }, "paths": paths,
            }),
        );
    }
}

fn filenames(data: Ref<'_, IDataObject>) -> Vec<String> {
    let Some(data) = data.as_ref() else {
        return vec![];
    };
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    unsafe {
        let Ok(mut medium) = data.GetData(&format) else {
            return vec![];
        };
        let hdrop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        let mut paths = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            paths.push(
                OsString::from_wide(&buf[..len])
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        ReleaseStgMedium(&mut medium);
        paths
    }
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for FileDropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let paths = filenames(data);
        self.files.set(!paths.is_empty());
        let p = self.point(pt);
        if self.files.get() {
            self.emit("enter", p, paths);
        }
        unsafe {
            if !effect.is_null() {
                *effect = if self.files.get() && self.accepts(p) {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let p = self.point(pt);
        if self.files.get() {
            self.emit("over", p, vec![]);
        }
        unsafe {
            if !effect.is_null() {
                *effect = if self.files.get() && self.accepts(p) {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.files.set(false);
        self.emit("leave", POINT::default(), vec![]);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let p = self.point(pt);
        let paths = filenames(data);
        let accepted = !paths.is_empty() && self.accepts(p);
        unsafe {
            if !effect.is_null() {
                *effect = if accepted {
                    DROPEFFECT_COPY
                } else {
                    DROPEFFECT_NONE
                };
            }
        }
        if accepted {
            self.emit("drop", p, paths);
        }
        self.files.set(false);
        Ok(())
    }
}

pub fn install(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let root = window.hwnd()?.0 as usize;
    let app = window.app_handle().clone();
    window.run_on_main_thread(move || unsafe {
        let root = HWND(root as *mut std::ffi::c_void);
        let mut context = (root, app);
        register(root, &context);
        let _ = EnumChildWindows(
            Some(root),
            Some(child),
            LPARAM(&mut context as *mut _ as isize),
        );
    })
}

unsafe fn register(hwnd: HWND, context: &(HWND, AppHandle)) {
    if REGISTERED.with(|r| r.borrow().contains(&(hwnd.0 as usize))) {
        return;
    }
    let target: IDropTarget = FileDropTarget {
        app: context.1.clone(),
        root: context.0,
        files: Cell::new(false),
    }
    .into();
    unsafe {
        let _ = RevokeDragDrop(hwnd);
        if let Err(e) = RegisterDragDrop(hwnd, &target) {
            tracing::warn!("шторка: не удалось зарегистрировать приём файлов: {e}");
        } else {
            REGISTERED.with(|r| {
                r.borrow_mut().insert(hwnd.0 as usize);
            });
        }
    }
}

unsafe extern "system" fn child(hwnd: HWND, param: LPARAM) -> BOOL {
    let context = unsafe { &*(param.0 as *const (HWND, AppHandle)) };
    unsafe {
        register(hwnd, context);
    }
    true.into()
}
