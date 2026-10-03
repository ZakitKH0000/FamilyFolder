//! Этап 1: проверка, что два компьютера находят друг друга через интернет
//! и передают файл любого размера — с докачкой и проверкой целостности.
//!
//! Запуск двойным щелчком (меню) или перетаскиванием файла на программу (сразу отправка).

mod common;
mod recv;
mod send;

use std::path::PathBuf;

use anyhow::Result;

use common::prompt;

#[tokio::main]
async fn main() {
    println!("Общая папка — проверка связи (этап 1)\n");
    let result = match std::env::args_os().nth(1) {
        Some(path) => send::run(PathBuf::from(path)).await,
        None => menu().await,
    };
    if let Err(e) = result {
        println!("\nОшибка: {e:#}");
    }
    let _ = prompt("\nНажмите Enter, чтобы закрыть окно.").await;
}

async fn menu() -> Result<()> {
    println!("1 — Отправить файл");
    println!("2 — Получить файл\n");
    loop {
        match prompt("Ваш выбор (1 или 2): ").await?.as_str() {
            "1" => {
                let path = prompt("Перетащите файл в это окно и нажмите Enter:\n> ").await?;
                return send::run(PathBuf::from(path.trim_matches(['"', '\'']))).await;
            }
            "2" => {
                let code = prompt(
                    "Вставьте код от отправителя (Ctrl+V или правой кнопкой мыши) и нажмите Enter:\n> ",
                )
                .await?;
                return recv::run(code).await;
            }
            _ => println!("Введите 1 или 2."),
        }
    }
}
