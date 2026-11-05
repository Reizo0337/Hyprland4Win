//! Música para el widget: las sesiones multimedia de Windows (`GlobalSystemMediaTransportControls`, API
//! documentada de WinRT). Muestra lo que suena en cualquier reproductor y permite pausar y cambiar de pista.

use std::sync::Mutex;
use std::time::Duration;

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::widget_draw::Media;

static STATE: Mutex<Option<Media>> = Mutex::new(None);
static STARTED: std::sync::Once = std::sync::Once::new();

pub fn latest() -> Option<Media> {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn session() -> Option<Session> {
    Manager::RequestAsync().ok()?.join().ok()?.GetCurrentSession().ok()
}

/// Guarda la carátula en un archivo temporal (el nombre depende del tema y artista, así que se reutiliza).
fn save_cover(s: &Session, title: &str, artist: &str) -> Option<std::path::PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (title, artist).hash(&mut h);
    let path = std::env::temp_dir().join(format!("hypr-win-cover-{:016x}.img", h.finish()));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
        return Some(path);
    }
    let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    std::fs::write(&path, buf).ok()?;
    Some(path)
}

/// Lee lo que suena ahora (sin sesión: `None`).
pub fn poll_once() -> Option<Media> {
    let s = session()?;
    let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let title = props.Title().ok()?.to_string();
    let artist = props.Artist().ok()?.to_string();
    let playing = s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? == Status::Playing;
    let thumb = save_cover(&s, &title, &artist);
    Some(Media { title, artist, playing, thumb })
}

/// Arranca (una sola vez) el sondeo en segundo plano cada 2 s; `on_change` se llama cuando algo cambia.
pub fn start(on_change: fn()) {
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("hypr-win-media".into()).spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            loop {
                let now = poll_once();
                let changed = {
                    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
                    let changed = *g != now;
                    *g = now;
                    changed
                };
                if changed {
                    on_change();
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    });
}

fn control(f: fn(&Session) -> windows::core::Result<bool>) {
    let _ = std::thread::Builder::new().name("hypr-win-media-ctl".into()).spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        if let Some(s) = session() {
            let _ = f(&s);
        }
    });
}

pub fn toggle() {
    control(|s| s.TryTogglePlayPauseAsync()?.join());
}

pub fn previous() {
    control(|s| s.TrySkipPreviousAsync()?.join());
}

pub fn next() {
    control(|s| s.TrySkipNextAsync()?.join());
}
