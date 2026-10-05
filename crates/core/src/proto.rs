//! Сообщения между компьютерами. Каждое — JSON с 4-байтовой длиной впереди.

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::model::{CloudCreds, Group, Member, Offer, Status};

pub const SYNC_ALPN: &[u8] = b"obshaya-papka/sync/1";
pub const PAIR_ALPN: &[u8] = b"obshaya-papka/pair/1";

/// Первый байт каждого потока внутри соединения.
pub const STREAM_CONTROL: u8 = 1;
pub const STREAM_FILE: u8 = 2;
/// Установщик новой версии программы для другого устройства семьи.
pub const STREAM_UPDATE: u8 = 3;
pub const STREAM_VOICE: u8 = 4;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t")]
pub enum Msg {
    Hello {
        name: String,
        members: Vec<Member>,
        removed: Vec<String>,
        version: String,
        cloud: Option<CloudCreds>,
        /// Установщик, который это устройство может отдать (с подписью разработчика).
        #[serde(default)]
        update: Option<UpdateInfo>,
        /// Пауза: файлы с этого устройства сейчас не отдаются.
        #[serde(default)]
        paused: bool,
    },
    Offers {
        offers: Vec<Offer>,
    },
    Statuses {
        items: Vec<(String, Status)>,
    },
    Ping,
    /// Сообщение (текст, ссылка) — только устройствам с версии `notes::SINCE`.
    Note {
        note: crate::notes::NoteMsg,
    },
    NoteAck {
        id: String,
    },
    /// Только устройствам 1.5.0+: отдельный разговор и метаданные голоса.
    Chat { note: crate::notes::ChatMsg },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileReq {
    pub offer_id: String,
    pub index: usize,
    pub offset: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum FileResp {
    Ok,
    /// Файл изменился или удалён — эта версия больше недоступна.
    Changed,
    /// Отправитель на паузе — попробовать позже.
    Busy,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub size: u64,
    /// BLAKE3 установщика — проверка целостности при передаче.
    pub hash: String,
    /// Подпись разработчика (minisign, как у Tauri) — проверяет окно перед установкой.
    pub sig: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UpdateReq {
    pub hash: String,
    pub offset: u64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct JoinReq {
    pub secret: String,
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum JoinResp {
    Ok {
        group: Group,
        cloud: Option<CloudCreds>,
    },
    Error(String),
}

const MAX_MSG: u32 = 64 * 1024 * 1024;

pub async fn write_msg<T: Serialize>(w: &mut (impl AsyncWrite + Unpin), msg: &T) -> Result<()> {
    let data = serde_json::to_vec(msg)?;
    w.write_u32(data.len() as u32).await?;
    w.write_all(&data).await?;
    Ok(())
}

pub async fn read_msg<T: DeserializeOwned>(r: &mut (impl AsyncRead + Unpin)) -> Result<T> {
    read_bounded_msg(r, MAX_MSG).await
}

pub async fn read_bounded_msg<T: DeserializeOwned>(r: &mut (impl AsyncRead + Unpin), max: u32) -> Result<T> {
    let len = r.read_u32().await?;
    anyhow::ensure!(len <= max, "control message too large");
    let mut buf = vec![0; len as usize];
    r.read_exact(&mut buf).await?;
    Ok(serde_json::from_slice(&buf)?)
}
