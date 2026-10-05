//! Уведомления Windows с кнопками «Получить» / «Отклонить».

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager};
use tauri_winrt_notification::{IconCrop, Toast};

use obshaya_core::t;

use crate::{AppState, panel};

static ICON: OnceLock<PathBuf> = OnceLock::new();

pub fn init(icon: PathBuf) {
    let _ = ICON.set(icon);
}

fn base(title: &str) -> Toast {
    let mut t = Toast::new(&crate::shell::aumid()).title(title);
    // У программы из Store значок — у пакета, а файл из её папки данных Windows не увидит.
    if let Some(icon) = ICON.get().filter(|_| !crate::shell::is_packaged()) {
        t = t.icon(icon, IconCrop::Square, "");
    }
    t
}

fn on_action(app: &AppHandle, action: Option<String>) {
    let action = action.unwrap_or_default();
    let engine = app.state::<AppState>().engine.clone();
    if let Some(peer) = action.strip_prefix("share:") {
        if let Err(e) = engine.share_history(peer, true) { simple(&e.to_string(), ""); }
    } else if let Some(peer) = action.strip_prefix("keep-private:") {
        let _ = engine.share_history(peer, false);
    } else if let Some(id) = action.strip_prefix("accept:") {
        if let Err(e) = engine.accept(id) {
            show_panel_tab(app, "in");
            simple(&e.to_string(), "");
        }
    } else if let Some(id) = action.strip_prefix("decline:") {
        engine.decline(id);
    } else if let Some(id) = action.strip_prefix("copy:") {
        if let Some(text) = engine.note_text(id) {
            let _ = crate::shell::copy_text(&text);
        }
    } else if let Some(id) = action.strip_prefix("chat:") {
        crate::open_chat(app.clone(), id.into());
    } else if let Some(url) = action.strip_prefix("url:") {
        crate::shell::open_url(url);
    } else if let Some(path) = action.strip_prefix("open:") {
        crate::shell::reveal(Path::new(path));
    } else if action == "update" {
        if let Err(e) = crate::updater::install(app) {
            simple(&e, "");
        }
    } else if action == "out" {
        show_panel_tab(app, "out");
    } else {
        show_panel_tab(app, "in");
    }
}

fn show_panel_tab(app: &AppHandle, tab: &str) {
    panel::show_floating(app);
    let _ = app.emit("show-tab", tab);
}

pub fn new_offer(app: &AppHandle, id: &str, from: &str, title: &str, detail: &str, is_update: bool, has_exe: bool) {
    let heading = if is_update { t!("toast.new_version", from = from) } else { t!("toast.new_file", from = from) };
    let detail = if has_exe { format!("{detail} · {}", t!("toast.is_program")) } else { detail.to_string() };
    let app2 = app.clone();
    let toast = base(&heading)
        .text1(title)
        .text2(&detail)
        .add_button(&t!("btn.accept"), &format!("accept:{id}"))
        .add_button(&t!("btn.decline"), &format!("decline:{id}"))
        .on_activated(move |action| {
            on_action(&app2, action);
            Ok(())
        });
    if let Err(e) = toast.show() {
        tracing::warn!("уведомление не показалось: {e}");
    }
}

pub fn received(app: &AppHandle, title: &str, path: &Path, conflicts: usize) {
    let text2 = if conflicts > 0 {
        t!("toast.conflict")
    } else {
        t!("toast.saved")
    };
    let app2 = app.clone();
    let toast = base(&t!("toast.received", name = title))
        .text1(&text2)
        .add_button(&t!("btn.show"), &format!("open:{}", path.display()))
        .on_activated(move |action| {
            on_action(&app2, action);
            Ok(())
        });
    let _ = toast.show();
}

pub fn delivered(app: &AppHandle, title: &str, to: &str) {
    let app2 = app.clone();
    let toast = base(&t!("toast.delivered", name = title))
        .text1(&t!("toast.delivered_to", to = to))
        .on_activated(move |_| {
            on_action(&app2, Some("out".into()));
            Ok(())
        });
    let _ = toast.show();
}

pub fn note(app: &AppHandle, id: &str, from: &str, text: &str, url: Option<&str>, voice: bool) {
    let app2 = app.clone();
    let short: String = text.chars().take(240).collect();
    let mut toast = base(&t!("note.from", from = from)).sound(None).text1(&short).add_button(&t!("chat.title"), &format!("chat:{id}"));
    if !voice { toast = toast.add_button(&t!("btn.copy"), &format!("copy:{id}")); }
    if let Some(url) = url {
        toast = toast.add_button(&t!("btn.open_link"), &format!("url:{url}"));
    }
    let default_action = format!("chat:{id}");
    let toast = toast.on_activated(move |action| {
        on_action(&app2, action.or(Some(default_action.clone())));
        Ok(())
    });
    if let Err(e) = toast.show() {
        tracing::warn!("уведомление не показалось: {e}");
    }
}

pub fn joined(app: &AppHandle, id: &str, name: &str, added_by: &str, files: usize) {
    let app2 = app.clone();
    let detail = if files > 0 { t!("share.question", name = name, n = files) } else { t!("toast.joined_text") };
    let mut toast = base(&t!("toast.joined", name = name))
        .text1(&t!("share.added_by", name = added_by)).text2(&detail);
    if files > 0 {
        toast = toast.add_button(&t!("share.allow"), &format!("share:{id}"))
            .add_button(&t!("share.deny"), &format!("keep-private:{id}"));
    }
    let toast = toast.on_activated(move |action| {
        on_action(&app2, action);
        Ok(())
    });
    let _ = toast.show();
}

pub fn update_ready(app: &AppHandle, version: &str) {
    let app2 = app.clone();
    let toast = base(&t!("toast.update_ready", v = version))
        .text1(&t!("toast.update_ready_text"))
        .add_button(&t!("update.install"), "update")
        .on_activated(move |action| {
            on_action(&app2, action);
            Ok(())
        });
    let _ = toast.show();
}

pub fn updated(version: &str) {
    let _ = base(&t!("toast.updated", v = version)).text1(&t!("toast.updated_text")).show();
}

pub fn simple(title: &str, text: &str) {
    let _ = base(title).text1(text).show();
}
