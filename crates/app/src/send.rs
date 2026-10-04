//! Файлы, брошенные в шторку или на окно программы: копируются в общую папку обычным окном
//! копирования Windows — с прогрессом и вопросом «Заменить или пропустить», — а дальше
//! предлагаются семье как обычно.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMMKDIR, FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName,
};
use windows::core::HSTRING;

use crate::AppState;

/// Файлы бросили в шторку или на окно. `peers` не пусто — предложить только этим устройствам.
/// Возвращает, сколько вещей скопируется.
pub fn drop_in(app: &AppHandle, paths: Vec<PathBuf>, peers: Vec<String>) -> usize {
    let engine = app.state::<AppState>().engine.clone();
    let folder = engine.folder();
    let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists() && !inside(p, &folder)).collect();
    if paths.is_empty() {
        return 0;
    }
    {
        let peers = if peers.is_empty() { engine.snapshot().peers.into_iter().map(|p| p.id).collect() } else { peers };
        let items = paths.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
        engine.target(items, peers);
    }
    let n = paths.len();
    std::thread::spawn(move || {
        if let Err(e) = copy_into(&paths, &folder) {
            tracing::warn!("копирование в общую папку: {e:#}");
        }
    });
    n
}

fn inside(p: &Path, folder: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches('\\').to_lowercase();
    let (p, f) = (norm(p), norm(folder));
    p == f || p.starts_with(&format!("{f}\\"))
}

fn copy_into(paths: &[PathBuf], dest: &Path) -> windows::core::Result<()> {
    std::fs::create_dir_all(dest).ok();
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let op: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)?;
        op.SetOperationFlags(FOF_ALLOWUNDO | FOF_NOCONFIRMMKDIR)?;
        let target: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(dest.as_os_str()), None)?;
        for p in paths {
            let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(p.as_os_str()), None)?;
            op.CopyItem(&item, &target, None, None)?;
        }
        op.PerformOperations()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_folder() {
        assert!(inside(Path::new("C:\\Users\\x\\Общая\\a"), Path::new("c:\\users\\x\\общая")));
        assert!(!inside(Path::new("C:\\Users\\x\\Общая2"), Path::new("C:\\Users\\x\\Общая")));
    }
}
