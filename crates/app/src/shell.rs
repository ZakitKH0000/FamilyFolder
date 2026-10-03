//! Windows: значок папки, ярлык, Проводник, имя программы в уведомлениях.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use obshaya_core::t;
use windows::Win32::Foundation::RECT;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_SYSTEM,
    FILE_FLAGS_AND_ATTRIBUTES, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_SendTo, IShellLinkW, KF_FLAG_DEFAULT, SHCNE_UPDATEITEM, SHCNF_PATHW, SHChangeNotify,
    SHGetKnownFolderPath, SetCurrentProcessExplicitAppUserModelID, ShellExecuteW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{SPI_GETWORKAREA, SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW};
use windows::core::{HSTRING, Interface, PCWSTR, w};

pub const AUMID: &str = "com.obshayapapka.app";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Чтобы уведомления показывались от имени «Общая папка» с нашим значком.
pub fn register_aumid(icon: &Path) {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(format!(r"Software\Classes\AppUserModelId\{AUMID}")) {
        let _ = key.set_value("DisplayName", &t!("app.name"));
        let _ = key.set_value("IconUri", &icon.to_string_lossy().into_owned());
    }
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("com.obshayapapka.app"));
    }
}

fn set_attrs(path: &Path, attrs: FILE_FLAGS_AND_ATTRIBUTES) {
    let w = wide(path);
    unsafe {
        let _ = SetFileAttributesW(PCWSTR(w.as_ptr()), attrs);
    }
}

/// Свой значок у общей папки (через desktop.ini).
pub fn decorate_folder(folder: &Path) {
    let Ok(exe) = std::env::current_exe() else { return };
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    let ini = folder.join("desktop.ini");
    let content = format!(
        "[.ShellClassInfo]\r\nIconResource={},0\r\nInfoTip={}\r\n",
        exe.display(),
        t!("folder.tip")
    );
    let mut bytes = vec![0xFF, 0xFE];
    for u in content.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    if std::fs::read(&ini).ok().as_deref() == Some(bytes.as_slice()) {
        return;
    }
    set_attrs(&ini, FILE_ATTRIBUTE_NORMAL);
    if std::fs::write(&ini, &bytes).is_err() {
        return;
    }
    set_attrs(&ini, FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM);
    let w = wide(folder);
    unsafe {
        let attrs = GetFileAttributesW(PCWSTR(w.as_ptr()));
        if attrs != INVALID_FILE_ATTRIBUTES {
            let _ = SetFileAttributesW(PCWSTR(w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(attrs) | FILE_ATTRIBUTE_READONLY);
        }
        SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW, Some(w.as_ptr() as _), None);
    }
}

fn known_dir(id: &windows::core::GUID) -> Result<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None)?;
        let s = p.to_string();
        CoTaskMemFree(Some(p.0 as _));
        Ok(PathBuf::from(s?))
    }
}

fn lnk_name() -> String {
    format!("{}.lnk", t!("app.name"))
}

fn desktop_lnk() -> Result<PathBuf> {
    Ok(known_dir(&FOLDERID_Desktop)?.join(lnk_name()))
}

/// Меню «Отправить» (правая кнопка мыши на любом файле).
fn sendto_lnk() -> Result<PathBuf> {
    Ok(known_dir(&FOLDERID_SendTo)?.join(lnk_name()))
}

/// Наши ярлыки, созданные на любом языке.
fn existing_links() -> Vec<PathBuf> {
    let dirs = [known_dir(&FOLDERID_Desktop), known_dir(&FOLDERID_SendTo)];
    let names = obshaya_core::i18n::all_values("app.name");
    dirs.into_iter()
        .flatten()
        .flat_map(|d| names.iter().map(move |n| d.join(format!("{n}.lnk"))))
        .filter(|p| p.exists())
        .collect()
}

fn write_shortcut(lnk: &Path, folder: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(folder.as_os_str()))?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        link.SetDescription(&HSTRING::from(t!("folder.tip")))?;
        let file: IPersistFile = link.cast()?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true)?;
    }
    Ok(())
}

pub fn create_desktop_shortcut(folder: &Path) -> Result<()> {
    write_shortcut(&desktop_lnk()?, folder)
}

pub fn add_send_to(folder: &Path) -> Result<()> {
    write_shortcut(&sendto_lnk()?, folder)
}

fn powershell(script: &str) {
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn ps_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "''"))
}

const QUICK_ACCESS: &str = "shell:::{679f85cb-0220-4080-b29b-5540cc05aab6}";

/// Закрепить в Проводнике слева (раздел «Главная», рядом с «Загрузками»).
pub fn pin_quick_access(folder: &Path) {
    let f = ps_quote(folder);
    powershell(&format!(
        "$sh = New-Object -ComObject Shell.Application; \
         if (-not ($sh.Namespace('{QUICK_ACCESS}').Items() | Where-Object {{ $_.Path -eq {f} }})) \
         {{ $sh.Namespace({f}).Self.InvokeVerb('pintohome') }}"
    ));
}

/// Папку перенесли: закрепление и ярлыки, которые уже были, переводятся на новое место.
pub fn move_links(old: &Path, new: &Path) {
    for lnk in existing_links() {
        if let Err(e) = write_shortcut(&lnk, new) {
            tracing::warn!("ярлык {}: {e:#}", lnk.display());
        }
    }
    let (o, n) = (ps_quote(old), ps_quote(new));
    powershell(&format!(
        "$sh = New-Object -ComObject Shell.Application; \
         $old = $sh.Namespace('{QUICK_ACCESS}').Items() | Where-Object {{ $_.Path -eq {o} }}; \
         if ($old) {{ $old | ForEach-Object {{ $_.InvokeVerb('unpinfromhome') }}; $sh.Namespace({n}).Self.InvokeVerb('pintohome') }}"
    ));
}

/// Язык сменился: ярлыки называются по-новому, подсказка у папки — на новом языке.
pub fn relabel(folder: &Path) {
    for old in existing_links() {
        let new = old.with_file_name(lnk_name());
        if old != new && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
        let _ = write_shortcut(&new, folder);
    }
    decorate_folder(folder);
}

fn is_pinned(folder: &Path) -> bool {
    let f = ps_quote(folder);
    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "$sh = New-Object -ComObject Shell.Application; \
                 [bool]($sh.Namespace('{QUICK_ACCESS}').Items() | Where-Object {{ $_.Path -eq {f} }})"
            ),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "True")
}

/// Что было в Windows перед удалением: при переустановке (новая версия ставится через удаление
/// старой) программа вернёт это при первом запуске.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct Restore {
    desktop: bool,
    sendto: bool,
    pin: bool,
}

pub fn restore(data_dir: &Path, folder: &Path) {
    let file = data_dir.join("restore.json");
    let Some(r) = std::fs::read(&file).ok().and_then(|d| serde_json::from_slice::<Restore>(&d).ok()) else { return };
    let _ = std::fs::remove_file(&file);
    tracing::info!("возвращаю ярлыки после переустановки");
    if r.desktop {
        let _ = create_desktop_shortcut(folder);
    }
    if r.sendto {
        let _ = add_send_to(folder);
    }
    if r.pin {
        pin_quick_access(folder);
    }
}

/// Удаление программы: ярлыки, закрепление, автозапуск. Сама общая папка с файлами остаётся.
pub fn uninstall(data_dir: &Path, folder: &Path) {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    let links = existing_links();
    let has = |dir: Result<PathBuf>| dir.is_ok_and(|d| links.iter().any(|l| l.parent() == Some(d.as_path())));
    let r = Restore {
        desktop: has(known_dir(&FOLDERID_Desktop)),
        sendto: has(known_dir(&FOLDERID_SendTo)),
        pin: is_pinned(folder),
    };
    if let Ok(data) = serde_json::to_vec(&r) {
        let _ = std::fs::write(data_dir.join("restore.json"), data);
    }
    for lnk in links {
        let _ = std::fs::remove_file(lnk);
    }
    let f = ps_quote(folder);
    let _ = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!(
                "$sh = New-Object -ComObject Shell.Application; \
                 $sh.Namespace('{QUICK_ACCESS}').Items() | Where-Object {{ $_.Path -eq {f} }} | ForEach-Object {{ $_.InvokeVerb('unpinfromhome') }}"
            ),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    let ini = folder.join("desktop.ini");
    set_attrs(&ini, FILE_ATTRIBUTE_NORMAL);
    let _ = std::fs::remove_file(ini);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let exe = std::env::current_exe().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if let Ok(run) = hkcu.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_ALL_ACCESS) {
        let ours: Vec<String> = run
            .enum_values()
            .flatten()
            .filter(|(_, v)| v.to_string().to_lowercase().contains(&exe))
            .map(|(n, _)| n)
            .collect();
        for name in ours {
            let _ = run.delete_value(name);
        }
    }
    let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\AppUserModelId\{AUMID}"));
}

/// Показать файл в Проводнике (с выделением) или открыть папку.
pub fn reveal(path: &Path) {
    let result = if path.is_file() {
        Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", path.display())).spawn()
    } else {
        Command::new("explorer.exe").arg(path).spawn()
    };
    if let Err(e) = result {
        tracing::warn!("не удалось открыть Проводник: {e}");
    }
}

/// Первая веб-ссылка в тексте (для кнопки «Открыть»).
pub fn first_url(text: &str) -> Option<String> {
    let start = text.find("https://").into_iter().chain(text.find("http://")).min()?;
    let url: String = text[start..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '<' | '>' | '«' | '»')).collect();
    Some(url.trim_end_matches(['.', ',', ')', ';', '!', '?']).to_string())
}

/// Скопировать текст в буфер обмена.
pub fn copy_text(text: &str) -> windows::core::Result<()> {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::CF_UNICODETEXT;
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    unsafe {
        // Буфер может быть ненадолго занят другой программой.
        let mut opened = OpenClipboard(None);
        for _ in 0..10 {
            if opened.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
            opened = OpenClipboard(None);
        }
        opened?;
        let result = (|| {
            EmptyClipboard()?;
            let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2)?;
            let p = GlobalLock(h) as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(h.0)))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}

/// Открыть ссылку в браузере (только веб-ссылки: из сообщений семьи приходит что угодно).
pub fn open_url(url: &str) {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) || url.chars().any(char::is_whitespace) {
        return;
    }
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
    }
}

/// Windows 11 (сборка 22000 и новее): там есть живой фон окон (Mica).
pub fn is_win11() -> bool {
    static WIN11: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *WIN11.get_or_init(|| {
        winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
            .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
            .ok()
            .and_then(|b| b.trim().parse::<u32>().ok())
            .is_some_and(|b| b >= 22000)
    })
}

/// Рабочая область основного экрана (без панели задач).
pub fn work_area() -> RECT {
    let mut r = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut RECT as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    r
}
