//! Полный сценарий на двух «компьютерах» в одном процессе (настоящая сеть iroh).
//! Запуск: cargo test -p obshaya-core --test flow -- --nocapture

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use obshaya_core::{AutoAccept, Engine, ItemView, Settings};

fn write(path: &Path, data: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

fn data(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| ((i * 31 + seed as usize * 7 + i / 1000) % 251) as u8).collect()
}

async fn wait_for<T>(what: &str, secs: u64, mut f: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            println!("  ✓ {what} ({:.1} с)", start.elapsed().as_secs_f64());
            return v;
        }
        if start.elapsed() > Duration::from_secs(secs) {
            panic!("не дождался: {what}");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn find_in(e: &Engine, item: &str, state: &str) -> Option<ItemView> {
    e.snapshot().incoming.into_iter().find(|i| i.item == item && i.state == state)
}

fn by_id_in(e: &Engine, id: &str, state: &str) -> Option<ItemView> {
    e.snapshot().incoming.into_iter().find(|i| i.id == id && i.state == state)
}

fn find_out(e: &Engine, item: &str, pred: impl Fn(&ItemView) -> bool) -> Option<ItemView> {
    e.snapshot().outgoing.into_iter().find(|i| i.item == item && pred(i))
}

fn online(e: &Engine) -> bool {
    e.snapshot().peers.iter().any(|p| p.online)
}

fn rename(e: &Engine, name: &str) {
    let s = Settings { device_name: name.into(), ..e.settings() };
    e.set_settings(s).unwrap();
}

async fn start(data: &Path, folder: &Path) -> Engine {
    let (e, rx) = Engine::start_in(data.to_path_buf(), Some(folder.to_path_buf())).await.unwrap();
    // События окну здесь не нужны, но канал должен жить.
    std::mem::forget(rx);
    e
}

async fn receive(b: &Engine, item: &str, secs: u64) -> ItemView {
    let inc = wait_for(&format!("предложение «{item}»"), secs, || find_in(b, item, "new")).await;
    b.accept(&inc.id).unwrap();
    wait_for(&format!("«{item}» получен"), secs, || by_id_in(b, &inc.id, "done")).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn full_flow() {
    unsafe {
        std::env::set_var("OBSHAYA_NO_TRASH", "1");
        std::env::set_var("OBSHAYA_QUIET_CHANGED_MS", "3000");
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let (da, fa): (PathBuf, PathBuf) = (root.join("a-data"), root.join("A Общая"));
    let (db, fb): (PathBuf, PathBuf) = (root.join("b-data"), root.join("B Общая"));
    let a = start(&da, &fa).await;
    let b = start(&db, &fb).await;
    rename(&a, "Компьютер А");
    rename(&b, "Компьютер Б");

    println!("1. Приглашение");
    let code = a.create_invite();
    let inviter = b.join(&code).await.expect("присоединение");
    assert_eq!(inviter, "Компьютер А");
    wait_for("связь в обе стороны", 90, || (online(&a) && online(&b)).then_some(())).await;
    println!("     маршрут: {}", a.snapshot().peers[0].route);
    assert!(b.join(&code).await.is_err(), "код одноразовый");

    println!("2. Новый файл");
    let photo = data(3_000_000, 1);
    write(&fa.join("фото.jpg"), &photo);
    let inc = wait_for("предложение у Б", 60, || find_in(&b, "фото.jpg", "new")).await;
    assert!(!inc.is_update);
    assert_eq!(inc.peer, "Компьютер А");
    assert_eq!(inc.total, photo.len() as u64);
    b.accept(&inc.id).unwrap();
    wait_for("получено", 60, || by_id_in(&b, &inc.id, "done")).await;
    assert_eq!(std::fs::read(fb.join("фото.jpg")).unwrap(), photo);
    wait_for("у А «доставлено»", 30, || find_out(&a, "фото.jpg", |o| o.state == "delivered")).await;

    println!("3. Полученный файл не уходит обратно");
    tokio::time::sleep(Duration::from_secs(7)).await;
    assert!(a.snapshot().incoming.is_empty(), "эхо: {:?}", a.snapshot().incoming.iter().map(|i| &i.item).collect::<Vec<_>>());

    println!("4. Новая версия");
    let photo2 = data(2_500_000, 2);
    write(&fa.join("фото.jpg"), &photo2);
    let inc = wait_for("новая версия у Б", 60, || find_in(&b, "фото.jpg", "new")).await;
    assert!(inc.is_update);
    b.accept(&inc.id).unwrap();
    wait_for("новая версия получена", 60, || by_id_in(&b, &inc.id, "done")).await;
    assert_eq!(std::fs::read(fb.join("фото.jpg")).unwrap(), photo2);
    assert_eq!(std::fs::read(fb.join(".obshaya/old/фото.jpg")).unwrap(), photo, "старая версия в корзине");

    println!("5. Папка целиком");
    write(&fa.join("Папка/1.txt"), b"one");
    write(&fa.join("Папка/вложенная/2.txt"), b"two");
    let inc = wait_for("предложение папки", 60, || find_in(&b, "Папка", "new")).await;
    assert!(inc.is_folder);
    assert_eq!(inc.files, 2);
    b.accept(&inc.id).unwrap();
    wait_for("папка получена", 60, || by_id_in(&b, &inc.id, "done")).await;
    assert_eq!(std::fs::read(fb.join("Папка/вложенная/2.txt")).unwrap(), b"two");

    println!("6. Конфликт: оба изменили один файл");
    write(&fa.join("заметки.txt"), b"base");
    receive(&b, "заметки.txt", 60).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    write(&fb.join("заметки.txt"), b"version B");
    write(&fa.join("заметки.txt"), b"version A");
    let inc_b = wait_for("версия А у Б", 60, || find_in(&b, "заметки.txt", "new")).await;
    let inc_a = wait_for("версия Б у А", 60, || find_in(&a, "заметки.txt", "new")).await;
    b.accept(&inc_b.id).unwrap();
    a.accept(&inc_a.id).unwrap();
    let done_b = wait_for("Б разложил", 60, || by_id_in(&b, &inc_b.id, "done")).await;
    let done_a = wait_for("А разложил", 60, || by_id_in(&a, &inc_a.id, "done")).await;
    assert_eq!(done_b.conflicts.len(), 1);
    assert_eq!(done_a.conflicts.len(), 1);
    assert_eq!(std::fs::read(fb.join("заметки.txt")).unwrap(), b"version B");
    assert_eq!(std::fs::read(fb.join("заметки (версия от Компьютер А).txt")).unwrap(), b"version A");
    assert_eq!(std::fs::read(fa.join("заметки.txt")).unwrap(), b"version A");
    assert_eq!(std::fs::read(fa.join("заметки (версия от Компьютер Б).txt")).unwrap(), b"version B");

    println!("7. Отказ");
    write(&fa.join("ненужное.bin"), &data(1000, 3));
    let inc = wait_for("предложение", 60, || find_in(&b, "ненужное.bin", "new")).await;
    b.decline(&inc.id);
    wait_for("у А «отклонено»", 30, || find_out(&a, "ненужное.bin", |o| o.state == "declined")).await;
    assert!(!fb.join("ненужное.bin").exists());

    println!("8. Получатель был выключен");
    b.shutdown().await;
    drop(b);
    write(&fa.join("пока_выключен.txt"), b"hello");
    wait_for("предложение ждёт", 30, || find_out(&a, "пока_выключен.txt", |o| o.state == "pending")).await;
    let b = start(&db, &fb).await;
    receive(&b, "пока_выключен.txt", 120).await;
    assert_eq!(std::fs::read(fb.join("пока_выключен.txt")).unwrap(), b"hello");

    println!("9. Через облако (отправитель выключился)");
    let cloud = root.join("облако");
    a.cloud_use_local(cloud.clone());
    b.cloud_use_local(cloud.clone());
    b.shutdown().await;
    drop(b);
    let big = data(70 * 1024 * 1024 + 123, 4); // 3 куска по 32 МБ
    write(&fa.join("через облако.bin"), &big);
    let out = wait_for("предложение создано", 60, || find_out(&a, "через облако.bin", |_| true)).await;
    a.upload_now(&out.id);
    wait_for("загружено в облако", 120, || find_out(&a, "через облако.bin", |o| o.cloud == "uploaded")).await;
    a.shutdown().await;
    drop(a);
    let b = start(&db, &fb).await;
    let inc = wait_for("предложение из облака", 90, || find_in(&b, "через облако.bin", "new")).await;
    b.accept(&inc.id).unwrap();
    let done = wait_for("скачано из облака", 120, || by_id_in(&b, &inc.id, "done")).await;
    assert_eq!(done.source, Some(obshaya_core::t!("source.cloud")));
    assert_eq!(std::fs::read(fb.join("через облако.bin")).unwrap(), big);
    let mail = cloud.join("mail");
    wait_for("Б отчитался через облако", 90, || {
        let empty = std::fs::read_dir(&mail).map(|rd| rd.flatten().all(|d| std::fs::read_dir(d.path()).map(|r| r.count() == 0).unwrap_or(true))).unwrap_or(true);
        empty.then_some(())
    })
    .await;
    b.shutdown().await;
    drop(b);
    let a = start(&da, &fa).await;
    wait_for("у А «доставлено» из облака", 90, || find_out(&a, "через облако.bin", |o| o.state == "delivered")).await;
    wait_for("облако очищено", 90, || {
        let n = std::fs::read_dir(cloud.join("data")).map(|r| r.count()).unwrap_or(0);
        (n == 0).then_some(())
    })
    .await;

    println!("10. Третье устройство по коду второго, автоприём");
    let b = start(&db, &fb).await;
    wait_for("связь А и Б", 90, || (online(&a) && online(&b)).then_some(())).await;
    write(&fb.join("старый файл Б.txt"), b"owned by B");
    wait_for("старый файл Б предложен А", 60, || find_in(&a, "старый файл Б.txt", "new")).await;
    let (dc, fc): (PathBuf, PathBuf) = (root.join("c-data"), root.join("C Общая"));
    let c = start(&dc, &fc).await;
    rename(&c, "Компьютер В");
    let mut s = c.settings();
    s.auto_accept = Some(AutoAccept::All);
    c.set_settings(s).unwrap();
    let inviter = c.join(&b.create_invite()).await.expect("третье устройство");
    assert_eq!(inviter, "Компьютер Б");
    wait_for("А видит В", 120, || a.snapshot().peers.iter().any(|p| p.name == "Компьютер В" && p.online).then_some(())).await;
    wait_for("А спрашивают о прежних файлах", 30, || a.snapshot().history_shares.iter()
        .find(|r| r.peer_id == c.device_id()).cloned()).await;
    let request_b = wait_for("Б спрашивают о своих прежних файлах", 30, || b.snapshot().history_shares.iter()
        .find(|r| r.peer_id == c.device_id()).cloned()).await;
    assert_eq!(request_b.added_by, "Компьютер Б", "показываем, кто пригласил");
    assert!(c.snapshot().incoming.is_empty(), "новичку не предложили старые файлы даже с автоприёмом");
    // Перезапуск пригласившего не теряет запрос и не превращает молчание в согласие.
    b.shutdown().await;
    let b = start(&db, &fb).await;
    assert!(b.snapshot().history_shares.iter().any(|r| r.peer_id == c.device_id()));
    b.share_history(&c.device_id(), true).unwrap();
    wait_for("В получил разрешённый старый файл Б", 90, || find_in(&c, "старый файл Б.txt", "done")).await;
    assert_eq!(std::fs::read(fc.join("старый файл Б.txt")).unwrap(), b"owned by B");
    assert!(c.snapshot().incoming.iter().all(|i| i.item != "фото.jpg"),
        "согласие Б не пересылает полученные от А файлы");
    a.share_history(&c.device_id(), false).unwrap();
    // Изменение старого собственного файла также не обходит отказ для нового устройства.
    write(&fa.join("фото.jpg"), b"private new revision");
    wait_for("Б предложена новая версия фото", 90, || find_in(&b, "фото.jpg", "new")).await;
    assert!(a.snapshot().outgoing.iter().all(|o| o.item != "фото.jpg" || o.peer_id != c.device_id()),
        "старые файлы и их новые версии не уходят новичку после отказа");
    assert!(c.snapshot().incoming.iter().all(|i| i.item != "фото.jpg"));
    let note = data(10_000, 5);
    write(&fa.join("для всех.txt"), &note);
    let got = wait_for("В принял сам", 120, || find_in(&c, "для всех.txt", "done")).await;
    assert!(got.auto, "принято без вопроса");
    assert_eq!(std::fs::read(fc.join("для всех.txt")).unwrap(), note);
    let at_b = wait_for("Б спрашивают", 60, || find_in(&b, "для всех.txt", "new")).await;
    assert!(!at_b.auto);
    let outs: Vec<ItemView> = a.snapshot().outgoing.into_iter().filter(|o| o.item == "для всех.txt").collect();
    assert_eq!(outs.len(), 2, "предложено обоим");
    assert_eq!(outs[0].batch, outs[1].batch, "одна карточка на всех получателей");

    println!("    только одному устройству (бросили на него в шторке)");
    a.target(vec!["только для В.txt".into()], vec![c.device_id()]);
    write(&fa.join("только для В.txt"), &note);
    wait_for("В получил своё", 120, || find_in(&c, "только для В.txt", "done")).await;
    let outs: Vec<ItemView> = a.snapshot().outgoing.into_iter().filter(|o| o.item == "только для В.txt").collect();
    assert_eq!(outs.len(), 1, "предложено одному");
    assert_eq!(outs[0].peer, "Компьютер В");
    assert!(b.snapshot().incoming.iter().all(|i| i.item != "только для В.txt"), "Б не спрашивают");

    println!("    сообщения: всем и одному");
    a.send_note("Ссылка: https://example.com/дача", &[]).unwrap();
    for (e, who) in [(&b, "Б"), (&c, "В")] {
        let n = wait_for(&format!("{who} получил сообщение"), 60, || {
            e.snapshot().notes.into_iter().find(|n| !n.outgoing && n.text.contains("example.com"))
        })
        .await;
        assert_eq!(n.peer, "Компьютер А");
        assert!(!n.seen);
    }
    wait_for("А видит доставку обоим", 60, || {
        a.snapshot().notes.into_iter().find(|n| n.outgoing && n.to.len() == 2 && n.to.iter().all(|t| t.delivered))
    })
    .await;
    a.send_note("Пароль от Wi-Fi: дача2026", &[c.device_id()]).unwrap();
    wait_for("В получил своё сообщение", 60, || c.snapshot().notes.into_iter().find(|n| n.text.contains("дача2026"))).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(b.snapshot().notes.iter().all(|n| !n.text.contains("дача2026")), "Б не получает чужое");
    let id = b.snapshot().notes[0].id.clone();
    b.notes_seen(std::slice::from_ref(&id));
    assert!(b.snapshot().notes[0].seen);
    b.dismiss(&id);
    assert!(b.snapshot().notes.is_empty());

    println!("    чаты: семейный, личный, голос вне общей папки");
    a.send_chat("семейный чат", None).unwrap();
    for e in [&b, &c] {
        let n = wait_for("семейный разговор", 30, || e.snapshot().notes.into_iter().find(|n| n.text == "семейный чат")).await;
        assert!(n.group);
    }
    a.send_chat("личный чат В", Some(&c.device_id())).unwrap();
    let n = wait_for("личный разговор", 30, || c.snapshot().notes.into_iter().find(|n| n.text == "личный чат В")).await;
    assert!(!n.group);
    assert!(b.snapshot().notes.iter().all(|n| n.text != "личный чат В"));
    let reply_id = n.id.clone();
    a.send_chat_reply("ответ В", Some(&c.device_id()), Some(&reply_id)).unwrap();
    let reply = wait_for("ответ с цитатой", 30, || c.snapshot().notes.into_iter().find(|n| n.text == "ответ В")).await;
    assert_eq!(reply.reply_to.as_deref(), Some(reply_id.as_str()));
    assert!(a.send_chat_reply("чужой разговор", Some(&b.device_id()), Some(&reply_id)).is_err());
    assert!(a.send_chat_reply("чужой семейный разговор", None, Some(&reply_id)).is_err());
    c.pause(None);
    let voice = data(400_000, 12); // синтетические байты: проверка транспорта, не аудиокодека
    a.send_voice_reply(&voice, "audio/webm", 2500, Some(&c.device_id()), &[128; 48], Some(&reply_id)).unwrap();
    let n = wait_for("метаданные голоса", 30, || c.snapshot().notes.into_iter().find(|n| n.voice.is_some())).await;
    assert!(!n.group);
    assert_eq!(n.reply_to.as_deref(), Some(reply_id.as_str()));
    assert_eq!(n.voice.as_ref().unwrap().waveform, vec![128; 48]);
    assert!(!n.voice_ready, "пауза запрещает скачивание записи");
    assert!(b.snapshot().notes.iter().all(|n| n.voice.is_none()));
    c.shutdown().await;
    let c = start(&dc, &fc).await;
    assert!(c.snapshot().notes.iter().any(|v| v.id == n.id && !v.voice_ready));
    assert!(c.snapshot().notes.iter().any(|v| v.text == "личный чат В"));
    c.resume();
    wait_for("голос доставлен", 30, || c.snapshot().notes.into_iter().find(|v| v.id == n.id && v.voice_ready)).await;
    assert_eq!(c.voice_bytes(&n.id).unwrap(), voice);
    assert!(std::fs::read_dir(&fc).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".audio")));
    wait_for("подтверждение голоса", 30, || a.snapshot().notes.into_iter().find(|v| v.id == n.id && v.to.iter().all(|p| p.delivered))).await;
    assert!(a.send_voice(b"x", "text/html", 1000, None).is_err());
    assert!(c.voice_bytes("../../outside").is_err());
    c.dismiss(&n.id);
    assert!(c.voice_bytes(&n.id).is_err());
    assert!(!dc.join("chat").join("voice").join(format!("{}.audio", n.id)).exists());

    println!("11. Пауза у отправителя");
    a.pause(None);
    wait_for("Б видит паузу А", 30, || b.snapshot().peers.iter().any(|p| p.name == "Компьютер А" && p.paused).then_some(())).await;
    let paused_file = data(2_000_000, 6);
    write(&fa.join("во время паузы.bin"), &paused_file);
    let inc = wait_for("предложение во время паузы", 60, || find_in(&b, "во время паузы.bin", "new")).await;
    b.accept(&inc.id).unwrap();
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert!(by_id_in(&b, &inc.id, "done").is_none(), "на паузе ничего не передаётся");
    a.resume();
    wait_for("после паузы получено", 60, || by_id_in(&b, &inc.id, "done")).await;
    assert_eq!(std::fs::read(fb.join("во время паузы.bin")).unwrap(), paused_file);

    println!("12. Обновление от устройства семьи");
    let installer = root.join("setup-test.exe");
    let fake = data(3_000_000, 7);
    write(&installer, &fake);
    a.set_update_package("99.0.0", installer.clone(), "ПОДПИСЬ").await.unwrap();
    for (e, d) in [(&b, &db), (&c, &dc)] {
        let got = d.join("update").join("setup-99.0.0.exe");
        wait_for("установщик скачан", 90, || got.is_file().then_some(())).await;
        assert_eq!(std::fs::read(&got).unwrap(), fake);
        assert_eq!(std::fs::read_to_string(d.join("update").join("setup-99.0.0.exe.sig")).unwrap(), "ПОДПИСЬ");
        e.set_update_package("99.0.0", got, "ПОДПИСЬ").await.unwrap();
        assert_eq!(e.update_ready().map(|u| u.0).as_deref(), Some("99.0.0"));
        assert_eq!(e.snapshot().update.as_deref(), Some("99.0.0"));
    }
    for e in [&a, &b, &c] {
        e.shutdown().await;
    }
    println!("Готово: все сценарии прошли");
}
