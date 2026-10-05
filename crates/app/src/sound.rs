//! Собственный мягкий сигнал, одна точка воспроизведения для шторки и toast.
use std::{collections::HashMap, sync::{Mutex, LazyLock}, time::{Duration, Instant}};
use tauri::{AppHandle, Manager};
use windows::{core::PCWSTR, Win32::{Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC}, UI::Shell::{SHQueryUserNotificationState, QUNS_ACCEPTS_NOTIFICATIONS}}};
static ACTIVE: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static LAST: Mutex<Option<Instant>> = Mutex::new(None);

#[tauri::command]
pub fn chat_presence(window: tauri::WebviewWindow, peer: Option<String>, active: bool) {
    let mut map = ACTIVE.lock().unwrap_or_else(|e|e.into_inner());
    if active { map.insert(window.label().into(), peer); } else { map.remove(window.label()); }
}
fn wav(volume: u8) -> Vec<u8> {
    let rate = 22050u32; let count = (rate as f64 * 0.7) as u32; let size = count*2;
    let mut out = b"RIFF".to_vec(); out.extend((36+size).to_le_bytes()); out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes()); out.extend(1u16.to_le_bytes()); out.extend(1u16.to_le_bytes());
    out.extend(rate.to_le_bytes()); out.extend((rate*2).to_le_bytes()); out.extend(2u16.to_le_bytes()); out.extend(16u16.to_le_bytes());
    out.extend(b"data"); out.extend(size.to_le_bytes());
    for i in 0..count {
        let t=i as f64/rate as f64; let mut sample=0.;
        for (start,hz,gain) in [(0.,1046.5,0.55),(0.11,1568.,0.30),(0.23,1318.5,0.40)] {
            let dt=t-start;
            if dt>=0. { let env=(dt/0.008).min(1.)*(-dt*12.).exp();
                sample += gain*env*((dt*hz*std::f64::consts::TAU).sin()+0.12*(dt*hz*2.*std::f64::consts::TAU).sin()); }
        }
        out.extend(((sample*volume.min(100) as f64/100.*20000.).clamp(-32767.,32767.) as i16).to_le_bytes());
    }
    out
}
fn play(app: &AppHandle, volume: u8) {
    // Тесты всегда беззвучны, даже если забыли отключить настройку.
    if volume==0 || std::env::var_os("OBSHAYA_DATA_DIR").is_some() { return; }
    let _=app; std::thread::spawn(move || {
        let bytes=wav(volume);
        unsafe { let _=PlaySoundW(PCWSTR(bytes.as_ptr().cast()), None, SND_MEMORY|SND_SYNC|SND_NODEFAULT); }
    });
}
pub fn message(app: &AppHandle, n: &obshaya_core::NoteView) {
    let settings=app.state::<crate::AppState>().engine.settings();
    if !settings.message_sound || settings.message_volume==0 { return; }
    if unsafe { SHQueryUserNotificationState() }.is_ok_and(|s|s!=QUNS_ACCEPTS_NOTIFICATIONS) { return; }
    if ACTIVE.lock().unwrap_or_else(|e|e.into_inner()).iter().any(|(label,peer)| {
        app.get_webview_window(label).is_some_and(|w|w.is_visible().unwrap_or(false)&&w.is_focused().unwrap_or(false))
            && if n.group {peer.is_none()} else {peer.as_deref()==Some(n.peer_id.as_str())}
    }) { return; }
    let mut last=LAST.lock().unwrap_or_else(|e|e.into_inner());
    if last.is_some_and(|t|t.elapsed()<Duration::from_secs(2)) {return;}
    *last=Some(Instant::now()); play(app,settings.message_volume);
}
#[tauri::command]
pub fn preview_message_sound(app: AppHandle) { let v=app.state::<crate::AppState>().engine.settings().message_volume; play(&app,v); }
#[cfg(test)] mod tests { use super::*; #[test] fn original_signal_is_bounded_pcm() {
    let a=wav(35); assert_eq!(&a[..4],b"RIFF"); assert_eq!(&a[8..12],b"WAVE");
    assert!(a.len()>30000 && a.len()<32000); assert!(wav(0)[44..].iter().all(|b|*b==0));
} }
