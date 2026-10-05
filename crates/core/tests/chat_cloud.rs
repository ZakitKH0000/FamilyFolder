//! Реальная облачная очередь через папку-транспорт; отправитель выключен при получении.
use std::{path::Path, time::{Duration, Instant}};
use obshaya_core::{Engine, Settings};
async fn start(root: &Path, name: &str) -> Engine {
    let (e, mut rx)=Engine::start_in(root.join(format!("{name}-data")),Some(root.join(format!("{name}-folder")))).await.unwrap();
    tokio::spawn(async move {while rx.recv().await.is_some(){}});
    e.set_settings(Settings {device_name:name.into(), ..e.settings()}).unwrap(); e
}
async fn wait(label: &str, f: impl Fn()->bool) {
    let end=Instant::now()+Duration::from_secs(60);
    while !f(){assert!(Instant::now()<end,"timeout {label}");tokio::time::sleep(Duration::from_millis(250)).await;}
}
#[tokio::test(flavor="multi_thread",worker_threads=4)]
async fn offline_sender_text_voice_reply_and_ack() {
    let tmp=tempfile::tempdir().unwrap(); let r=tmp.path();
    let a=start(r,"A").await; let b=start(r,"B").await;
    b.join(&a.create_invite()).await.unwrap();
    wait("connected",||a.snapshot().peers.iter().any(|p|p.online)).await;
    let aid=a.device_id();let bid=b.device_id();
    let cloud=r.join("cloud");std::fs::create_dir_all(&cloud).unwrap();
    a.cloud_use_local(cloud.clone()); b.cloud_use_local(cloud.clone());
    b.shutdown().await;
    wait("B offline",||a.snapshot().peers.iter().all(|p|!p.online)).await;
    a.send_chat("offline private",Some(&bid)).unwrap();
    let text=a.snapshot().notes.into_iter().find(|n|n.text=="offline private").unwrap();
    let audio=vec![73u8;64_000];
    a.send_voice_reply(&audio,"audio/webm",1400,Some(&bid),&[100;48],Some(&text.id)).unwrap();
    a.send_chat("offline family",None).unwrap();
    wait("complete cloud packets",||a.snapshot().notes.iter().filter(|n|n.outgoing).count()==3 && a.snapshot().notes.iter().filter(|n|n.outgoing).all(|n|n.cloud_ready)).await;
    a.shutdown().await;
    let b=start(r,"B").await;
    wait("received with A off",||b.snapshot().notes.len()==3 && b.snapshot().notes.iter().any(|n|n.voice_ready)).await;
    let s=b.snapshot();
    assert!(!s.notes.iter().find(|n|n.text=="offline private").unwrap().group);
    assert!(s.notes.iter().find(|n|n.text=="offline family").unwrap().group);
    let v=s.notes.iter().find(|n|n.voice.is_some()).unwrap();
    assert_eq!(v.reply_to.as_deref(),Some(text.id.as_str()));assert_eq!(v.voice.as_ref().unwrap().waveform,vec![100;48]);
    assert_eq!(b.voice_bytes(&v.id).unwrap(),audio);
    assert!(s.notes.iter().all(|n|n.peer_id==aid));
    b.dismiss(&v.id); assert_eq!(b.snapshot().notes.len(),2);
    b.shutdown().await;let b=start(r,"B").await;
    tokio::time::sleep(Duration::from_secs(6)).await;assert_eq!(b.snapshot().notes.len(),2,"cloud retry not duplicated");
    let a=start(r,"A").await;
    wait("persistent cloud ACK",||a.snapshot().notes.iter().all(|n|n.to.iter().all(|p|p.delivered))).await;
    tokio::time::sleep(Duration::from_secs(6)).await;assert_eq!(b.snapshot().notes.len(),2,"QUIC retry not duplicated");
    fn bins(p:&Path)->usize {std::fs::read_dir(p).into_iter().flatten().flatten().map(|e|if e.path().is_dir(){bins(&e.path())}else{usize::from(e.path().extension().is_some_and(|x|x=="bin"))}).sum()}
    wait("mail and ACK cleanup",||bins(&cloud)==0).await;
    a.shutdown().await;b.shutdown().await;
}
