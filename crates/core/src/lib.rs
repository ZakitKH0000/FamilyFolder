//! «Общая папка»: синхронизация папки между компьютерами семьи без своего сервера.

mod cloud;
pub mod i18n;
mod config;
mod crypto;
mod engine;
mod model;
mod notes;
mod pairing;
mod proto;
mod scan;
mod sharing;
mod store;
mod sync;
mod transfer;
mod update;
mod util;
mod view;

pub use config::{AutoAccept, CloudMode, NotifyVia, Settings, default_data_dir, default_folder};
pub use engine::{Engine, Event};
pub use util::{fmt_size, version_newer};
pub use view::{CloudView, ItemView, NoteTo, NoteView, PeerView, PrepView, UiState};

/// Сохранённые настройки без запуска «двигателя» (для удаления программы).
pub fn saved_settings(data_dir: &std::path::Path) -> Settings {
    store::load(&data_dir.join("state.json")).settings
}
