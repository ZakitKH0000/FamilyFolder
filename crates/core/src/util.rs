//! Мелкие помощники: время, атомарная запись, безопасные пути, форматирование.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn system_time(ms: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(ms.max(0) as u64)
}

/// Запись через временный файл: при сбое старая версия остаётся целой.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("системный генератор случайных чисел недоступен");
    b
}

pub fn hex(bytes: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(bytes)
}

pub fn short_id(id: &str) -> &str {
    id.get(..16).unwrap_or(id)
}

/// Относительный путь из сети ('/' между частями) → путь внутри `root`.
/// Отбрасывает «..», диски и прочее, что может вывести за пределы папки.
pub fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    let mut any = false;
    for comp in rel.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." || comp.contains(['\\', ':']) {
            return None;
        }
        let name = sanitize_name(comp);
        if name.eq_ignore_ascii_case(".obshaya") { return None; }
        p.push(name);
        any = true;
    }
    any.then_some(p)
}

/// Делает имя допустимым для Windows.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    if cleaned.is_empty() {
        crate::t!("name.untitled")
    } else if RESERVED.contains(&stem.as_str()) {
        format!("_{cleaned}")
    } else {
        cleaned
    }
}

/// Путь внутри `root` → относительный путь с '/'.
pub fn rel_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// Первая часть относительного пути — «элемент» верхнего уровня (файл или папка).
pub fn top_item(rel: &str) -> &str {
    rel.split('/').next().unwrap_or(rel)
}

/// «фото.jpg» → «фото (2).jpg», если такой уже есть. `suffix` вставляется перед номером.
pub fn unique_path(path: &Path, suffix: &str) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let base = if suffix.is_empty() { stem } else { format!("{stem} ({suffix})") };
    let first = path.with_file_name(format!("{base}{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|i| path.with_file_name(format!("{base} ({i}){ext}")))
        .find(|p| !p.exists())
        .expect("бесконечный перебор")
}

/// Версия `a` новее `b` («1.2.0» новее «1.1.9»).
pub fn version_newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| v.split('.').map(|p| p.trim().parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    parse(a) > parse(b)
}

pub fn fmt_size(bytes: u64) -> String {
    let units = crate::t!("units");
    let units: Vec<&str> = units.split('|').collect();
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i + 1 < units.len() {
        v /= 1024.0;
        i += 1;
    }
    let num = if i == 0 {
        bytes.to_string()
    } else if v >= 100.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}").replace('.', &crate::t!("decimal"))
    };
    format!("{num} {}", units.get(i).copied().unwrap_or("B"))
}

/// Свободное место на диске, где лежит `path`.
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

    let mut dir = path.to_path_buf();
    while !dir.exists() {
        dir = dir.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None).ok()? };
    Some(free)
}

/// Скрытая папка (для служебной `.obshaya`).
pub fn set_hidden(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
    use windows::core::PCWSTR;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_HIDDEN);
    }
}

/// Помечает файл «загружен из интернета», чтобы Windows проверила его перед запуском.
pub fn mark_from_internet(path: &Path) {
    let mut ads = path.as_os_str().to_owned();
    ads.push(":Zone.Identifier");
    let _ = std::fs::write(PathBuf::from(ads), "[ZoneTransfer]\r\nZoneId=3\r\n");
}

pub fn is_executable(name: &str) -> bool {
    const EXT: [&str; 14] = [
        "exe", "msi", "bat", "cmd", "com", "scr", "ps1", "vbs", "vbe", "js", "jse", "wsf", "lnk", "hta",
    ];
    ext_of(name).is_some_and(|e| EXT.contains(&e.as_str()))
}

pub fn ext_of(name: &str) -> Option<String> {
    let name = name.trim().trim_end_matches(['.', ' ']);
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext.to_lowercase())
}

/// Вид файла для значка в окне.
pub fn kind_of(name: &str, is_folder: bool) -> &'static str {
    if is_folder {
        return "folder";
    }
    if is_executable(name) {
        return "app";
    }
    match ext_of(name).as_deref() {
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "heif" | "tif" | "tiff" | "raw" | "svg") => "image",
        Some("mp4" | "mov" | "avi" | "mkv" | "wmv" | "webm" | "m4v" | "3gp") => "video",
        Some("mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "wma" | "opus") => "audio",
        Some("zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso") => "archive",
        Some("doc" | "docx" | "odt" | "rtf" | "txt" | "pdf" | "xls" | "xlsx" | "ods" | "csv" | "ppt" | "pptx" | "odp" | "md") => "doc",
        _ => "file",
    }
}

pub fn is_image(name: &str) -> bool {
    kind_of(name, false) == "image"
}

#[cfg(test)]
mod path_tests {
    use super::*;
    #[test]
    fn network_paths_cannot_target_service_data() {
        let root = Path::new("C:\\family");
        assert!(safe_join(root, "photos/summer.jpg").is_some());
        for rel in ["../secret", ".obshaya/state.json", "Photos/.OBSHAYA/identity", "C:\\secret"] {
            assert!(safe_join(root, rel).is_none(), "{rel}");
        }
        assert!(is_executable("setup.EXE. "));
        assert!(!is_executable("photo.jpg"));
    }
}
