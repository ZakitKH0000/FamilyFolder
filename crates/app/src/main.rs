//! «Общая папка» для Windows: значок у часов, панель, уведомления, связка с Проводником.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dock;
mod island;
mod menu;
mod notify;
mod panel;
mod send;
mod shell;
mod tour;
mod tray;
mod updater;

use std::path::{Path, PathBuf};

use obshaya_core::{Engine, Event, Settings, UiState, t};
use serde_json::json;
use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;

pub struct AppState {
    pub engine: Engine,
    pub data_dir: PathBuf,
    pub icon: PathBuf,
}

type Res<T = ()> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn get_state(st: State<AppState>) -> UiState {
    st.engine.snapshot()
}

/// Все строки интерфейса на текущем языке.
#[tauri::command]
fn get_strings() -> serde_json::Value {
    obshaya_core::i18n::strings()
}

#[tauri::command]
fn accept(st: State<AppState>, id: String) -> Res {
    st.engine.accept(&id).map_err(err)
}

#[tauri::command]
fn decline(st: State<AppState>, id: String) {
    st.engine.decline(&id);
}

#[tauri::command]
fn dismiss(st: State<AppState>, id: String) {
    st.engine.dismiss(&id);
}

#[tauri::command]
fn upload_now(st: State<AppState>, id: String) {
    st.engine.upload_now(&id);
}

#[tauri::command]
fn open_path(st: State<AppState>, path: String) {
    let p = PathBuf::from(&path);
    if !path.is_empty() && p.exists() {
        shell::reveal(&p);
    } else {
        shell::reveal(&st.engine.folder());
    }
}

#[tauri::command]
fn open_folder(st: State<AppState>) {
    shell::reveal(&st.engine.folder());
}

#[tauri::command]
fn open_url(url: String) {
    shell::open_url(&url);
}

/// Сообщение семье: `peers` пусто — всем.
#[tauri::command]
fn send_note(st: State<AppState>, text: String, peers: Vec<String>) -> Res {
    st.engine.send_note(&text, &peers).map_err(err)
}

#[tauri::command]
fn notes_seen(st: State<AppState>, ids: Vec<String>) {
    st.engine.notes_seen(&ids);
}

#[tauri::command]
fn copy_text(text: String) -> Res {
    shell::copy_text(&text).map_err(err)
}

#[tauri::command]
fn open_logs(st: State<AppState>) {
    shell::reveal(&st.data_dir.join("logs"));
}

#[tauri::command]
fn create_invite(st: State<AppState>) -> String {
    st.engine.create_invite()
}

#[tauri::command]
async fn join(st: State<'_, AppState>, code: String) -> Res<String> {
    st.engine.join(&code).await.map_err(err)
}

#[tauri::command]
fn remove_device(st: State<AppState>, id: String) {
    st.engine.remove_device(&id);
}

fn apply_settings(app: &AppHandle, new: Settings) -> Res {
    let st = app.state::<AppState>();
    let old = st.engine.settings();
    st.engine.set_settings(new.clone()).map_err(err)?;
    if old.folder != new.folder {
        shell::decorate_folder(&new.folder);
        shell::move_links(&old.folder, &new.folder);
    }
    if old.language != new.language {
        tray::refresh(app);
        shell::register_aumid(&st.icon);
        if new.onboarded {
            shell::relabel(&new.folder);
        }
    }
    if old.autostart != new.autostart || !cfg!(debug_assertions) {
        sync_autostart(app, new.autostart);
    }
    Ok(())
}

fn sync_autostart(app: &AppHandle, enabled: bool) {
    if cfg!(debug_assertions) || std::env::var_os("OBSHAYA_DATA_DIR").is_some() {
        return; // отладочная сборка и тестовые экземпляры не прописываются в автозапуск
    }
    let al = app.autolaunch();
    let now = al.is_enabled().unwrap_or(false);
    let r = if enabled && !now { al.enable() } else if !enabled && now { al.disable() } else { Ok(()) };
    if let Err(e) = r {
        tracing::warn!("автозапуск: {e}");
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Res {
    apply_settings(&app, settings)
}

#[tauri::command]
fn setup_device(app: AppHandle, name: String, folder: String, shortcut: bool, pin: bool) -> Res {
    let folder = PathBuf::from(folder);
    std::fs::create_dir_all(&folder).map_err(|e| t!("err.create_folder_reason", reason = e))?;
    let mut s = app.state::<AppState>().engine.settings();
    if !name.trim().is_empty() {
        s.device_name = name.trim().to_string();
    }
    s.folder = folder.clone();
    apply_settings(&app, s)?;
    shell::decorate_folder(&folder);
    if shortcut && let Err(e) = shell::create_desktop_shortcut(&folder) {
        tracing::warn!("ярлык: {e:#}");
    }
    if pin {
        shell::pin_quick_access(&folder);
    }
    Ok(())
}

#[tauri::command]
fn pin_folder(st: State<AppState>) {
    shell::pin_quick_access(&st.engine.folder());
}

#[tauri::command]
fn add_send_to(st: State<AppState>) -> Res {
    shell::add_send_to(&st.engine.folder()).map_err(|e| t!("err.sendto", reason = format!("{e:#}")))
}

#[tauri::command]
fn complete_onboarding(app: AppHandle, st: State<AppState>) -> Res {
    let mut s = st.engine.settings();
    s.onboarded = true;
    let first = s.tour_seen < tour::VERSION;
    st.engine.set_settings(s).map_err(err)?;
    // Знакомство закончилось — Папыч покажет, что умеет.
    if first && std::env::var_os("OBSHAYA_DATA_DIR").is_none() {
        tour::start_when_active(app, std::time::Duration::from_millis(1500));
    }
    Ok(())
}

#[tauri::command]
async fn pick_folder(app: AppHandle, title: Option<String>) -> Option<String> {
    app.dialog()
        .file()
        .set_title(title.unwrap_or_else(|| t!("dialog.pick_folder")))
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
async fn cloud_connect(st: State<'_, AppState>, client_id: String, client_secret: String) -> Res {
    st.engine.cloud_connect_yandex(&client_id, &client_secret).await.map_err(err)
}

#[tauri::command]
async fn cloud_connect_webdav(st: State<'_, AppState>, url: String, user: String, password: String) -> Res {
    st.engine.cloud_connect_webdav(&url, &user, &password).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn cloud_use_folder(st: State<AppState>, path: String) -> Res {
    if path.trim().is_empty() {
        return Err(t!("err.no_folder"));
    }
    st.engine.cloud_use_local(PathBuf::from(path));
    Ok(())
}

#[tauri::command]
fn cloud_folders(st: State<AppState>) -> Vec<(String, String)> {
    st.engine.cloud_folders()
}

#[tauri::command]
fn cloud_disconnect(st: State<AppState>) {
    st.engine.cloud_disconnect();
}

#[tauri::command]
fn create_shortcut(st: State<AppState>) -> Res {
    shell::create_desktop_shortcut(&st.engine.folder()).map_err(|e| t!("err.shortcut", reason = format!("{e:#}")))
}

/// Пауза: `minutes` — на сколько, None — пока не нажмут «Продолжить».
#[tauri::command]
fn pause(st: State<AppState>, minutes: Option<u64>) {
    st.engine.pause(minutes);
}

#[tauri::command]
fn resume(st: State<AppState>) {
    st.engine.resume();
}

#[tauri::command]
fn update_install(app: AppHandle) -> Res {
    updater::install(&app)
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    panel::hide(&app);
}

/// Выход. Если идёт загрузка в облако и не `force` — возвращает false (окно спросит).
#[tauri::command]
async fn quit(app: AppHandle, st: State<'_, AppState>, force: bool) -> Result<bool, String> {
    if !force && st.engine.uploads_in_progress() {
        return Ok(false);
    }
    st.engine.shutdown().await;
    app.exit(0);
    Ok(true)
}

/// Выход из меню значка: при незаконченной загрузке в облако — спросить в окне.
pub fn request_quit(app: &AppHandle) {
    let engine = app.state::<AppState>().engine.clone();
    if engine.uploads_in_progress() {
        panel::show_floating(app);
        let _ = app.emit("ask-quit", ());
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        engine.shutdown().await;
        app.exit(0);
    });
}

fn init_logging(data_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    let dir = data_dir.join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("app")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(EnvFilter::new(std::env::var("OBSHAYA_LOG").unwrap_or_else(|_| "info".into()) + ",iroh=warn,noq=warn,iroh_relay=warn,portmapper=warn,netwatch=warn"))
        .init();
    std::panic::set_hook(Box::new(|info| tracing::error!("сбой: {info}")));
    Some(guard)
}

fn create_window(app: &AppHandle, data_dir: &Path) -> tauri::Result<()> {
    let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title(t!("app.name"))
        .inner_size(380.0, 620.0)
        .min_inner_size(340.0, 440.0)
        .decorations(false)
        .shadow(true)
        .resizable(true)
        .skip_taskbar(true)
        .visible(false)
        .focused(false)
        .data_directory(data_dir.join("webview"));
    // Windows 11: живой фон (Mica), как у Проводника; окно рисует поверх лёгкий оттенок.
    if shell::is_win11() {
        b = b.transparent(true).effects(EffectsBuilder::new().effect(Effect::Mica).build());
    }
    b.build()?;
    Ok(())
}

/// Есть ли у окна живой фон Windows 11 (тогда страница делает свой фон полупрозрачным).
#[tauri::command]
fn backdrop() -> bool {
    shell::is_win11()
}

fn pump_events(app: AppHandle, mut rx: tokio::sync::mpsc::UnboundedReceiver<Event>) {
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let engine = app.state::<AppState>().engine.clone();
            match ev {
                Event::Changed => {
                    let st = engine.snapshot();
                    tray::update(&app, &st);
                    let _ = app.emit("state", &st);
                }
                Event::NewOffer { id, from, title, detail, is_update, has_exe } => {
                    let r = island::route(&app);
                    if r.island {
                        let data = json!({ "id": id, "from": from, "title": title, "detail": detail, "is_update": is_update, "has_exe": has_exe });
                        island::show(&app, "offer", data);
                    }
                    if r.toast {
                        notify::new_offer(&app, &id, &from, &title, &detail, is_update, has_exe)
                    }
                }
                Event::Received { title, path, conflicts, .. } => {
                    let r = island::route(&app);
                    if r.island {
                        island::show(&app, "received", json!({ "title": title, "path": path, "conflicts": conflicts }));
                    }
                    if r.toast {
                        notify::received(&app, &title, &path, conflicts)
                    }
                }
                Event::Delivered { title, to } => {
                    let r = island::route(&app);
                    if r.island {
                        island::show(&app, "delivered", json!({ "title": title, "to": to }));
                    }
                    if r.toast {
                        notify::delivered(&app, &title, &to)
                    }
                }
                Event::Joined { name } => {
                    let r = island::route(&app);
                    if r.island {
                        island::show(&app, "joined", json!({ "name": name }));
                    }
                    if r.toast {
                        notify::joined(&app, &name)
                    }
                }
                Event::Note { id, from, text } => {
                    let r = island::route(&app);
                    let url = shell::first_url(&text);
                    if r.island {
                        island::show(&app, "note", json!({ "id": id, "from": from, "text": text, "url": url }));
                    }
                    if r.toast {
                        notify::note(&app, &id, &from, &text, url.as_deref())
                    }
                }
                Event::UpdateReady { version, path, sig, hash } => updater::downloaded(&app, version, path, sig, hash),
            }
        }
    });
}

/// Вызывает удаление программы (до её файлов): убрать ярлыки, меню, автозапуск.
fn uninstall(data_dir: &Path) {
    let settings = obshaya_core::saved_settings(data_dir);
    obshaya_core::i18n::set_lang(&settings.language);
    menu::uninstall();
    shell::uninstall(data_dir, &settings.folder);
    tracing::info!("следы программы в Windows убраны");
}

fn main() {
    let data_dir = obshaya_core::default_data_dir().expect("no data folder");
    let _log = init_logging(&data_dir);
    let args: Vec<String> = std::env::args().collect();
    tracing::info!("запуск, версия {}", env!("CARGO_PKG_VERSION"));
    if args.iter().any(|a| a == "--uninstall") {
        uninstall(&data_dir);
        return;
    }
    let test_instance = std::env::var_os("OBSHAYA_DATA_DIR").is_some();
    if test_instance {
        menu::set_test_mode();
    }
    let updated = args.iter().any(|a| a == "--updated");
    // После автозапуска и обновления окно не показываем — программа просто работает у часов.
    let autostarted = updated || args.iter().any(|a| a == "--autostart");

    let icon_path = data_dir.join("icon.png");
    let _ = std::fs::create_dir_all(&data_dir);
    let _ = std::fs::write(&icon_path, include_bytes!("../icons/128x128@2x.png"));
    notify::init(icon_path.clone());

    let mut builder = tauri::Builder::default();
    if !test_instance {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            panel::show_floating(app);
        }));
    }
    let app = builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .manage(panel::Panel::new())
        .setup(move |app| {
            let handle = app.handle().clone();
            let (engine, rx) = tauri::async_runtime::block_on(Engine::start(data_dir.clone()))?;
            // Язык уже выбран «двигателем» — теперь имя в уведомлениях на нём.
            shell::register_aumid(&icon_path);
            app.manage(AppState { engine: engine.clone(), data_dir: data_dir.clone(), icon: icon_path.clone() });
            create_window(&handle, &data_dir)?;
            island::create(&handle, &data_dir)?;
            tray::create(&handle)?;
            pump_events(handle.clone(), rx);
            let settings = engine.settings();
            if settings.onboarded {
                shell::decorate_folder(&settings.folder);
                sync_autostart(&handle, settings.autostart);
                shell::restore(&data_dir, &settings.folder);
            }
            menu::cleanup();
            dock::start(handle.clone());
            updater::startup(&handle);
            tour::startup(&handle);
            updater::auto_loop(&handle);
            if updated {
                notify::updated(env!("CARGO_PKG_VERSION"));
            }
            if !autostarted || !settings.onboarded {
                panel::show_floating(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    panel::hide(window.app_handle());
                } else if window.label() == tour::LABEL {
                    api.prevent_close();
                    tour::finish(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_strings,
            accept,
            decline,
            dismiss,
            upload_now,
            open_path,
            open_folder,
            open_url,
            send_note,
            notes_seen,
            copy_text,
            open_logs,
            create_invite,
            join,
            remove_device,
            save_settings,
            setup_device,
            complete_onboarding,
            pick_folder,
            cloud_connect,
            cloud_connect_webdav,
            cloud_use_folder,
            cloud_folders,
            cloud_disconnect,
            create_shortcut,
            pin_folder,
            add_send_to,
            pause,
            resume,
            update_install,
            hide_panel,
            quit,
            island::island_close,
            island::island_pass,
            island::island_ready,
            island::send_dropped,
            backdrop,
            tour::tour_start,
            tour::tour_done,
            tour::island_demo,
            island::show_main,
        ])
        .build(tauri::generate_context!())
        .expect("window start failed");
    app.run(|_app, event| {
        // Окно скрыто, но программа продолжает работать у часов.
        if let tauri::RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
