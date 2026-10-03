//! Отправитель: считает сумму файла, показывает код и раздаёт файл всем, кто подключится.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use iroh::endpoint::Incoming;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::common::{
    CHUNK, FileInfo, PROTOCOL_VERSION, Progress, bind, copy_to_clipboard, fmt_size, hash_prefix,
    route, write_msg,
};

pub async fn run(path: PathBuf) -> Result<()> {
    let meta = tokio::fs::metadata(&path)
        .await
        .with_context(|| format!("не удалось открыть {}", path.display()))?;
    anyhow::ensure!(
        meta.is_file(),
        "это папка, а не файл. На этом этапе папки не поддерживаются — запакуйте её в архив"
    );
    let name = path
        .file_name()
        .context("у файла нет имени")?
        .to_string_lossy()
        .into_owned();
    let size = meta.len();
    println!("\nФайл: {name} ({})", fmt_size(size));

    let mut hasher = blake3::Hasher::new();
    hash_prefix(&path, size, &mut hasher, "Считаю контрольную сумму").await?;
    let info = Arc::new(FileInfo {
        name,
        size,
        hash: hasher.finalize().to_hex().to_string(),
    });

    println!("Подключаюсь к сети...");
    let endpoint = bind(true).await?;
    let code = endpoint.id().to_string();
    println!("\nКод для получателя:\n\n  {code}\n");
    if copy_to_clipboard(&code) {
        println!("Код уже скопирован — просто вставьте его брату в мессенджер.");
    }
    println!("Не закрывайте это окно, пока файл не будет получен.");

    while let Some(incoming) = endpoint.accept().await {
        let info = info.clone();
        let path = path.clone();
        tokio::spawn(async move {
            if let Err(e) = serve(incoming, &path, &info).await {
                println!("\nПередача прервалась: {e:#}");
            }
        });
    }
    Ok(())
}

async fn serve(incoming: Incoming, path: &Path, info: &FileInfo) -> Result<()> {
    let conn = incoming.await?;
    println!("\nПолучатель подключился ({}).", route(&conn));
    let (mut send, mut recv) = conn.accept_bi().await?;

    let version = recv.read_u8().await?;
    anyhow::ensure!(
        version == PROTOCOL_VERSION,
        "у получателя другая версия программы ({version})"
    );
    write_msg(&mut send, info).await?;

    // Получатель отвечает позицией, с которой продолжать, или закрывает соединение, если отказался.
    let Ok(offset) = recv.read_u64().await else {
        println!("Получатель отказался от файла или отключился.");
        return Ok(());
    };
    anyhow::ensure!(offset <= info.size, "неверная позиция докачки");
    if offset > 0 {
        println!("Продолжаю с {} (докачка).", fmt_size(offset));
    }

    let mut file = tokio::fs::File::open(path).await?;
    file.seek(SeekFrom::Start(offset)).await?;
    let mut reader = file.take(info.size - offset);
    let mut buf = vec![0u8; CHUNK];
    let mut done = offset;
    let mut progress = Progress::new("Отправка", info.size, offset);
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        send.write_all(&buf[..n]).await?;
        done += n as u64;
        progress.update(done, || route(&conn));
    }
    progress.finish();
    send.finish()?;

    let mut ack = [0u8; 1];
    recv.read_exact(&mut ack)
        .await
        .context("получатель не подтвердил приём")?;
    if ack[0] == 1 {
        println!("✓ Доставлено: получатель проверил файл, всё совпало.");
        println!("Окно можно закрыть.");
    } else {
        println!("✗ Получатель сообщил, что файл пришёл повреждённым.");
    }
    conn.close(0u32.into(), b"done");
    Ok(())
}
