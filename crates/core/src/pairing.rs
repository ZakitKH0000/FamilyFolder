//! Приглашения: один компьютер показывает код, другой вводит его и попадает в семью.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use iroh::EndpointId;
use iroh::endpoint::Connection;

use crate::engine::Inner;
use crate::model::{Invite, Member};
use crate::proto::{APP_VERSION, JoinReq, JoinResp, PAIR_ALPN, read_msg, write_msg};
use crate::util::{hex, now_ms, random_bytes};

const INVITE_TTL_MS: i64 = 24 * 3600 * 1000;
/// Ответ «код недействителен» — текст подставляет присоединяющийся на своём языке.
const INVITE_INVALID: &str = "invite_invalid";

pub(crate) fn create_invite(inner: &Arc<Inner>) -> String {
    let secret = random_bytes::<16>();
    {
        let mut s = inner.st();
        s.invites.push(Invite { secret: hex(&secret), created_at: now_ms(), used: false });
    }
    inner.save_soon();
    inner.changed();
    let id: EndpointId = inner.me.parse().expect("own id is valid");
    let mut raw = id.as_bytes().to_vec();
    raw.extend_from_slice(&secret);
    let code = data_encoding::BASE32_NOPAD.encode(&raw);
    // Группы по 4 символа легче сверять глазами.
    code.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("-")
}

fn parse_code(code: &str) -> Result<(EndpointId, String)> {
    let cleaned: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    let raw = data_encoding::BASE32_NOPAD
        .decode(cleaned.as_bytes())
        .ok()
        .filter(|r| r.len() == 48)
        .context(crate::t!("pair.bad_code_copy"))?;
    let id = EndpointId::from_bytes(raw[..32].try_into().unwrap())
        .ok()
        .context(crate::t!("pair.bad_code"))?;
    Ok((id, hex(&raw[32..])))
}

/// Сторона пригласившего.
pub(crate) async fn serve(inner: &Arc<Inner>, conn: Connection) -> Result<()> {
    let joiner = conn.remote_id().to_string();
    let (mut send, mut recv) = conn.accept_bi().await?;
    let req: JoinReq = read_msg(&mut recv).await?;
    let resp = {
        let mut s = inner.st();
        let now = now_ms();
        let valid = s
            .invites
            .iter_mut()
            .find(|i| i.secret == req.secret && !i.used && now - i.created_at < INVITE_TTL_MS);
        match valid {
            Some(invite) => {
                invite.used = true;
                s.group.removed.retain(|r| r != &joiner);
                let name = if req.name.trim().is_empty() { crate::t!("device.new") } else { req.name.clone() };
                s.group.merge(&[Member { id: joiner.clone(), name }], &[]);
                JoinResp::Ok { group: s.group.clone(), cloud: s.cloud.clone().filter(|c| !c.is_local()) }
            }
            None => JoinResp::Error(INVITE_INVALID.into()),
        }
    };
    let ok = matches!(resp, JoinResp::Ok { .. });
    write_msg(&mut send, &resp).await?;
    send.finish()?;
    let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
    if ok {
        tracing::info!("новое устройство в семье: {joiner}");
        inner.save_soon();
        inner.broadcast_hello();
        crate::scan::offer_all_to(inner, &joiner);
        inner.dial_now.notify_one();
        inner.changed();
    }
    Ok(())
}

/// Сторона присоединяющегося.
pub(crate) async fn join(inner: &Arc<Inner>, code: &str) -> Result<String> {
    let (id, secret) = parse_code(code)?;
    if id.to_string() == inner.me {
        bail!(crate::t!("pair.own_code"));
    }
    // Только что запущенный компьютер может ещё не успеть сообщить свой адрес, а поиск адреса
    // иногда зависает — пробуем две минуты, каждую попытку не дольше 25 секунд.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let conn = loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let result = tokio::time::timeout(left.min(Duration::from_secs(25)), inner.endpoint.connect(id, PAIR_ALPN)).await;
        match result {
            Ok(Ok(conn)) => break conn,
            Ok(Err(_)) | Err(_) if tokio::time::Instant::now() + Duration::from_secs(2) < deadline => {
                tracing::debug!("приглашение: повтор");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            _ => bail!(crate::t!("pair.no_answer")),
        }
    };
    let (mut send, mut recv) = conn.open_bi().await?;
    let name = inner.st().settings.device_name.clone();
    write_msg(&mut send, &JoinReq { secret, name: name.clone(), version: APP_VERSION.into() }).await?;
    send.finish()?;
    let resp: JoinResp = tokio::time::timeout(Duration::from_secs(30), read_msg(&mut recv))
        .await
        .context(crate::t!("pair.no_reply"))??;
    conn.close(0u32.into(), b"ok");
    match resp {
        JoinResp::Ok { mut group, cloud } => {
            let inviter = group.name_of(&id.to_string());
            let others: Vec<String> = {
                let mut s = inner.st();
                group.removed.retain(|r| r != &inner.me);
                match group.members.iter_mut().find(|m| m.id == inner.me) {
                    Some(m) => m.name = name,
                    None => group.members.push(Member { id: inner.me.clone(), name }),
                }
                s.group = group;
                if s.cloud.is_none() {
                    s.cloud = cloud.filter(|c| !c.is_local());
                }
                s.group.active().filter(|m| m.id != inner.me).map(|m| m.id.clone()).collect()
            };
            inner.save_soon();
            for peer in others {
                crate::scan::offer_all_to(inner, &peer);
            }
            inner.dial_now.notify_one();
            inner.kick_cloud();
            inner.changed();
            Ok(inviter)
        }
        // Старые версии присылали готовый текст по-русски.
        JoinResp::Error(msg) if msg == INVITE_INVALID => bail!(crate::t!("pair.invalid")),
        JoinResp::Error(msg) => bail!(msg),
    }
}
