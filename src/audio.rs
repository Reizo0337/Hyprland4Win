//! Volumen del dispositivo de salida predeterminado (Core Audio, API documentada).
//! Requiere COM inicializado en el hilo que llama.

use std::ptr::null;

use windows::core::PWSTR;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, STGM_READ};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Volume {
    /// 0.0 - 1.0
    pub level: f32,
    pub muted: bool,
}

unsafe fn endpoint() -> windows::core::Result<(IMMDevice, IAudioEndpointVolume)> {
    let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
    let dev = en.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
    let vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None)?;
    Ok((dev, vol))
}

pub fn get() -> Option<Volume> {
    unsafe {
        let (_, vol) = endpoint().ok()?;
        Some(Volume { level: vol.GetMasterVolumeLevelScalar().ok()?, muted: vol.GetMute().ok()?.as_bool() })
    }
}

pub fn set_level(level: f32) {
    unsafe {
        if let Ok((_, vol)) = endpoint() {
            let _ = vol.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), null());
        }
    }
}

pub fn set_muted(muted: bool) {
    unsafe {
        if let Ok((_, vol)) = endpoint() {
            let _ = vol.SetMute(muted, null());
        }
    }
}

/// Nombre del dispositivo de salida ("Altavoces (Realtek...)").
pub fn device_name() -> Option<String> {
    unsafe {
        let (dev, _) = endpoint().ok()?;
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let p: PWSTR = PropVariantToStringAlloc(&value).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}
