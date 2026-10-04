//! Данные, которые хранятся на диске и передаются между компьютерами.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub added_by: String,
}

/// «Семья» — набор связанных устройств и общий ключ для облака.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Group {
    pub id: String,
    /// 32 байта hex: шифрование файлов в облаке.
    pub key: String,
    pub members: Vec<Member>,
    pub removed: Vec<String>,
}

impl Group {
    pub fn active(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| !self.removed.contains(&m.id))
    }

    pub fn is_member(&self, id: &str) -> bool {
        self.active().any(|m| m.id == id)
    }

    pub fn name_of(&self, id: &str) -> String {
        self.members
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.name.clone())
            .unwrap_or_else(|| crate::t!("device.unknown"))
    }

    pub fn key_bytes(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        if let Ok(v) = data_encoding::HEXLOWER.decode(self.key.as_bytes())
            && v.len() == 32
        {
            out.copy_from_slice(&v);
        }
        out
    }

    /// Объединяет списки устройств. Возвращает true, если что-то изменилось.
    pub fn merge(&mut self, members: &[Member], removed: &[String]) -> bool {
        let mut changed = false;
        for r in removed {
            if !self.removed.contains(r) {
                self.removed.push(r.clone());
                changed = true;
            }
        }
        for m in members {
            match self.members.iter_mut().find(|x| x.id == m.id) {
                Some(x) => {
                    if x.name != m.name && !m.name.is_empty() {
                        x.name = m.name.clone();
                        changed = true;
                    }
                }
                None => {
                    self.members.push(m.clone());
                    changed = true;
                }
            }
        }
        changed
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OfferFile {
    /// Путь относительно общей папки, части через '/'.
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub hash: String,
    /// Версия, от которой сделано изменение (для определения конфликтов).
    #[serde(default)]
    pub prev_hash: Option<String>,
    /// Автор и разрешённые получатели: чужие файлы нельзя раздать новому участнику.
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub audience: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileAccess {
    pub owner: String,
    pub audience: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HistoryShare {
    pub peer: String,
    pub added_by: String,
    /// Только файлы, существовавшие при подключении; новые сюда не добавляются.
    pub paths: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Offer {
    pub id: String,
    pub from: String,
    /// Имя файла или папки верхнего уровня.
    pub item: String,
    pub is_folder: bool,
    pub files: Vec<OfferFile>,
    pub created_at: i64,
    #[serde(default)]
    pub supersedes: Vec<String>,
    #[serde(default)]
    pub in_cloud: bool,
}

impl Offer {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    pub fn is_update(&self) -> bool {
        self.files.iter().any(|f| f.prev_hash.is_some())
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutState {
    /// Ещё не дошло до получателя.
    Pending,
    /// Получатель видит предложение.
    Offered,
    Accepted,
    Downloading,
    Delivered,
    Declined,
    /// Заменено более новой версией.
    Superseded,
    /// Файл изменился или удалён до того, как его забрали.
    Unavailable,
}

impl OutState {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Delivered | Self::Declined | Self::Superseded | Self::Unavailable)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum CloudUp {
    None,
    Uploading,
    Uploaded,
    Failed(String),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Outgoing {
    pub offer: Offer,
    pub to: String,
    pub state: OutState,
    #[serde(default)]
    pub progress: u64,
    pub cloud: CloudUp,
    pub updated_at: i64,
    /// Общий для предложений одного элемента разным устройствам (окно показывает их одной карточкой).
    #[serde(default)]
    pub batch: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InState {
    New,
    /// Принято, ждёт источника (отправитель не в сети и в облаке нет).
    Queued,
    Downloading,
    Done,
    Declined,
    /// Ошибка, которую можно повторить (файл занят, нет места).
    Failed,
    Superseded,
    Unavailable,
}

impl InState {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Done | Self::Declined | Self::Superseded | Self::Unavailable)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Incoming {
    pub offer: Offer,
    pub state: InState,
    #[serde(default)]
    pub done_files: Vec<usize>,
    #[serde(default)]
    pub done_bytes: u64,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub saved: Vec<String>,
    #[serde(default)]
    pub auto: bool,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IndexEntry {
    pub size: u64,
    pub mtime: i64,
    pub hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Invite {
    pub secret: String,
    pub created_at: i64,
    pub used: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct CloudCreds {
    /// "yandex", "webdav" или "folder"; в записях до 1.1 пусто — тогда по полям.
    pub provider: String,
    pub client_id: String,
    pub client_secret: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub login: String,
    /// Папка, которую синхронизирует облачный диск (OneDrive, Google Диск…), или папка для проверки.
    pub local_dir: Option<String>,
    /// WebDAV: адрес, логин и пароль (лучше пароль приложения).
    pub url: String,
    pub user: String,
    pub password: String,
}

impl CloudCreds {
    pub fn kind(&self) -> &str {
        if !self.provider.is_empty() {
            &self.provider
        } else if self.local_dir.is_some() {
            "folder"
        } else {
            "yandex"
        }
    }

    pub fn usable(&self) -> bool {
        match self.kind() {
            "folder" => self.local_dir.is_some(),
            "webdav" => !self.url.is_empty(),
            _ => !self.access_token.is_empty(),
        }
    }

    /// Папка на этом компьютере — у каждого своя, другим устройствам не передаётся.
    pub fn is_local(&self) -> bool {
        self.kind() == "folder"
    }

    /// Чем одно подключение отличается от другого (для «отключил — не брать снова»).
    pub fn identity(&self) -> String {
        match self.kind() {
            "webdav" => crate::util::hex(blake3::hash(format!("{}|{}|{}", self.url, self.user, self.password).as_bytes()).as_bytes()),
            "folder" => self.local_dir.clone().unwrap_or_default(),
            _ => self.access_token.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CloudContent {
    pub size: u64,
    pub uploaded_at: i64,
}

/// Что получатель сообщает отправителю о предложении.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Received,
    Accepted,
    Downloading(u64),
    Delivered,
    Declined,
    Unavailable,
}

impl Status {
    pub fn from_in(state: InState, done: u64) -> Option<Self> {
        Some(match state {
            InState::New => Self::Received,
            InState::Queued | InState::Failed => Self::Accepted,
            InState::Downloading => Self::Downloading(done),
            InState::Done => Self::Delivered,
            InState::Declined => Self::Declined,
            InState::Unavailable => Self::Unavailable,
            InState::Superseded => return None,
        })
    }
}
