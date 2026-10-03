//! Остатки пункта «Отправить в общую папку» в меню правой кнопки мыши (был в версиях 1.1–1.3,
//! его заменили шторка и перетаскивание на окно). Программа убирает его при запуске, если он
//! остался от старой версии, и при удалении: запись в реестре и пакет нового меню Windows 11.

use std::os::windows::process::CommandExt;
use std::process::Command;

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const PKG_NAME: &str = "ObshayaPapka.ShellMenu";
const VERB: &str = "ObshayaPapka.Send";
const APP_KEY: &str = r"Software\ObshayaPapka";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Тестовые экземпляры (OBSHAYA_DATA_DIR) не трогают реестр настоящей программы.
static TEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_test_mode() {
    TEST.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn remove_classic() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for class in ["*", "Directory"] {
        let _ = hkcu.delete_subkey_all(format!(r"Software\Classes\{class}\shell\{VERB}"));
    }
    let _ = hkcu.delete_subkey_all(APP_KEY);
}

fn remove_modern() {
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command"])
        .arg(format!("Get-AppxPackage -Name {PKG_NAME} | Remove-AppxPackage"))
        .creation_flags(CREATE_NO_WINDOW)
        .output();
}

/// При запуске: если пункт меню остался от старой версии — убрать (пакет — в отдельном потоке).
pub fn cleanup() {
    if TEST.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let left = hkcu.open_subkey(format!(r"Software\Classes\*\shell\{VERB}")).is_ok() || hkcu.open_subkey(APP_KEY).is_ok();
    if !left {
        return;
    }
    remove_classic();
    std::thread::spawn(|| {
        remove_modern();
        tracing::info!("пункт меню правой кнопки от старой версии убран");
    });
}

/// Удаление программы: убрать все следы в меню.
pub fn uninstall() {
    remove_modern();
    remove_classic();
}
