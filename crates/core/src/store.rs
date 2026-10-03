//! Всё состояние программы одним JSON-файлом (`state.json`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::Settings;
use crate::model::{CloudContent, CloudCreds, Group, Incoming, IndexEntry, Invite, Outgoing};
use crate::util::now_ms;

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct State {
    pub settings: Settings,
    pub group: Group,
    /// Последнее известное содержимое общей папки: путь → размер, время, сумма.
    pub index: BTreeMap<String, IndexEntry>,
    /// Для какой папки построен индекс (при смене папки индекс строится заново).
    pub index_root: String,
    pub outgoing: Vec<Outgoing>,
    pub incoming: Vec<Incoming>,
    pub invites: Vec<Invite>,
    pub cloud: Option<CloudCreds>,
    /// Последние ClientID и Client secret — чтобы не вводить заново при переподключении.
    pub cloud_client: Option<(String, String)>,
    /// Ключ, от которого отказались кнопкой «Отключить», — не принимать его снова от других устройств.
    pub cloud_dropped: Option<String>,
    pub cloud_content: BTreeMap<String, CloudContent>,
    /// Когда устройство семьи последний раз было на связи.
    pub last_seen: BTreeMap<String, i64>,
    /// Пауза до этого времени (мс); i64::MAX — пока не продолжат, 0 — нет паузы.
    pub paused_until: i64,
    /// Сообщения семье и от семьи (старые сначала).
    pub notes: Vec<crate::notes::Note>,
}

pub fn load(path: &Path) -> State {
    let Ok(data) = std::fs::read(path) else {
        return State::default();
    };
    match serde_json::from_slice(&data) {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("state.json повреждён ({e}), начинаю с чистого состояния");
            let _ = std::fs::rename(path, path.with_extension(format!("broken-{}", now_ms())));
            State::default()
        }
    }
}

impl State {
    /// Убирает старую историю, чтобы файл не рос бесконечно.
    pub fn prune(&mut self) {
        const KEEP_MS: i64 = 30 * 24 * 3600 * 1000;
        const MAX: usize = 500;
        let cutoff = now_ms() - KEEP_MS;
        self.incoming
            .retain(|i| !(i.state.is_final() && i.updated_at < cutoff));
        self.outgoing
            .retain(|o| !(o.state.is_final() && o.updated_at < cutoff));
        if self.incoming.len() > MAX {
            let extra = self.incoming.len() - MAX;
            let mut removed = 0;
            self.incoming.retain(|i| {
                if removed < extra && i.state.is_final() {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
        if self.outgoing.len() > MAX {
            let extra = self.outgoing.len() - MAX;
            let mut removed = 0;
            self.outgoing.retain(|o| {
                if removed < extra && o.state.is_final() {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
        self.invites
            .retain(|i| !i.used && i.created_at > now_ms() - 24 * 3600 * 1000);
    }
}
