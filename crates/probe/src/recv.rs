//! Получатель: подключается по коду, спрашивает согласие, скачивает с докачкой и проверкой.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use iroh::EndpointId;
use tokio::io::AsyncWriteExt;

use crate::common::{
    ALPN, CHUNK, FileInfo, PROTOCOL_VERSION, Progress, bind, fmt_size, hash_prefix, interactive,
    prompt, read_msg, route,
};

pub async fn run(code: String) -> Result<()> {
    let id: EndpointId = code
        .trim()
        .parse()
        .context("код неправильный — скопируйте его целиком")?;

    println!("\nПодключаюсь к сети...");
    let endpoint = bind(false).await?;
    println!("Ищу отправителя...");
    let conn = match tokio::time::timeout(Duration::from_secs(60), endpoint.connect(id, ALPN)).await {
        Err(_) => bail!("отправитель не найден за 60 секунд. Проверьте, что у него открыто окно программы"),
        Ok(res) => res.context("не удалось соединиться с отправителем")?,
    };
    println!("Соединение установлено ({}).", route(&conn));

    let (mut send, mut recv) = conn.open_bi().await?;
    send.write_u8(PROTOCOL_VERSION).await?;
    let info: FileInfo = read_msg(&mut recv).await?;
    let name = safe_name(&info.name);
    println!("\nВам отправляют файл: {name} ({})", fmt_size(info.size));

    let answer = prompt("Получить? Enter — да, Н — нет: ").await?.to_lowercase();
    if answer.starts_with('н') || answer.starts_with('n') {
        conn.close(0u32.into(), b"declined");
        println!("Отменено.");
        return Ok(());
    }

    let dir = download_dir()?;
    let hash_tag = info.hash.get(..12).context("неверная контрольная сумма")?;
    let part_path = dir.join(format!("{name}.{hash_tag}.part"));

    // Докачка: если уже есть недокачанный кусок этого же файла, считаем его сумму и продолжаем.
    let mut hasher = blake3::Hasher::new();
    let mut offset = 0;
    if let Ok(meta) = tokio::fs::metadata(&part_path).await {
        if meta.len() <= info.size {
            println!("Нашёл недокачанную часть ({}), продолжаю с того же места.", fmt_size(meta.len()));
            hash_prefix(&part_path, meta.len(), &mut hasher, "Проверяю скачанное").await?;
            offset = meta.len();
        } else {
            tokio::fs::remove_file(&part_path).await?;
        }
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part_path)
        .await
        .with_context(|| format!("не удалось создать файл в {}", dir.display()))?;
    send.write_u64(offset).await?;

    let mut buf = vec![0u8; CHUNK];
    let mut done = offset;
    let mut progress = Progress::new("Получение", info.size, offset);
    let mut broken = None;
    while done < info.size {
        let n = match recv.read(&mut buf).await {
            Ok(Some(n)) => n,
            Ok(None) => break,
            Err(e) => {
                broken = Some(e);
                break;
            }
        };
        file.write_all(&buf[..n]).await?;
        hasher.update(&buf[..n]);
        done += n as u64;
        progress.update(done, || route(&conn));
    }
    file.flush().await?;
    drop(file);
    progress.finish();

    if done < info.size {
        let reason = broken.map(|e| format!(" ({e})")).unwrap_or_default();
        bail!(
            "связь прервалась на {} из {}{reason}.\nЗапустите получение ещё раз с тем же кодом — продолжится с этого места",
            fmt_size(done),
            fmt_size(info.size)
        );
    }

    let ok = hasher.finalize().to_hex().as_str() == info.hash;
    send.write_u8(u8::from(ok)).await?;
    send.finish()?;
    if !ok {
        tokio::fs::remove_file(&part_path).await.ok();
        bail!("файл повредился при передаче — попробуйте ещё раз");
    }

    let final_path = unique_path(&dir.join(&name));
    tokio::fs::rename(&part_path, &final_path).await?;
    println!("✓ Готово! Контрольная сумма совпала.");
    println!("Файл сохранён: {}", final_path.display());
    if interactive() {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", final_path.display()))
            .spawn();
    }

    // Даём подтверждению дойти до отправителя: он сам закроет соединение.
    let _ = tokio::time::timeout(Duration::from_secs(3), conn.closed()).await;
    endpoint.close().await;
    Ok(())
}

/// Папка «Получено» рядом с программой.
fn download_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().context("не найдена папка программы")?.join("Получено");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Имя приходит от другого компьютера: убираем всё, что может вывести файл за пределы папки.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || r#"\/:*?"<>|"#.contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    match cleaned.as_str() {
        "" => "файл".to_string(),
        _ if RESERVED.contains(&stem.as_str()) => format!("_{cleaned}"),
        _ => cleaned,
    }
}

/// «фото.jpg» → «фото (2).jpg», если такой файл уже есть.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|i| path.with_file_name(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .expect("бесконечный перебор")
}
