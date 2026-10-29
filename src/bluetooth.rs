//! Bluetooth: estado de la radio y control de encendido (WinRT `Windows.Devices.Radios`) y
//! dispositivos conectados (API Win32 de Bluetooth). Requiere COM inicializado en el hilo.

use std::mem::size_of;

use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::Devices::Bluetooth::{
    BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindNextDevice, BLUETOOTH_DEVICE_INFO,
    BLUETOOTH_DEVICE_SEARCH_PARAMS,
};
use windows::Win32::Foundation::HANDLE;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Hay un adaptador Bluetooth.
    pub present: bool,
    pub on: bool,
    /// Nombres de los dispositivos conectados ahora.
    pub connected: Vec<String>,
}

fn radio() -> Option<Radio> {
    let radios = Radio::GetRadiosAsync().ok()?.join().ok()?;
    radios.into_iter().find(|r| r.Kind().ok() == Some(RadioKind::Bluetooth))
}

pub fn status() -> Status {
    let Some(r) = radio() else { return Status::default() };
    let on = r.State().ok() == Some(RadioState::On);
    Status { present: true, on, connected: if on { connected_devices() } else { Vec::new() } }
}

pub fn set_radio(on: bool) -> Result<(), String> {
    let r = radio().ok_or("no hay adaptador Bluetooth")?;
    let access = Radio::RequestAccessAsync().and_then(|a| a.join()).map_err(|e| e.message())?;
    if access != RadioAccessStatus::Allowed {
        return Err("Windows no permite controlar el Bluetooth desde esta app".into());
    }
    let state = if on { RadioState::On } else { RadioState::Off };
    match r.SetStateAsync(state).and_then(|a| a.join()).map_err(|e| e.message())? {
        RadioAccessStatus::Allowed => Ok(()),
        _ => Err("Windows rechazó el cambio de Bluetooth".into()),
    }
}

/// Dispositivos emparejados que están conectados ahora (sin lanzar una búsqueda activa).
fn connected_devices() -> Vec<String> {
    let mut out = Vec::new();
    let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: true.into(),
        fReturnRemembered: true.into(),
        fReturnUnknown: false.into(),
        fReturnConnected: true.into(),
        fIssueInquiry: false.into(),
        cTimeoutMultiplier: 0,
        hRadio: HANDLE::default(),
    };
    let mut info = BLUETOOTH_DEVICE_INFO { dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32, ..Default::default() };
    unsafe {
        let Ok(find) = BluetoothFindFirstDevice(&params, &mut info) else { return out };
        loop {
            if info.fConnected.as_bool() {
                let end = info.szName.iter().position(|&c| c == 0).unwrap_or(info.szName.len());
                let name = String::from_utf16_lossy(&info.szName[..end]);
                if !name.is_empty() {
                    out.push(name);
                }
            }
            if BluetoothFindNextDevice(find, &mut info).is_err() {
                break;
            }
        }
        let _ = BluetoothFindDeviceClose(find);
    }
    out
}
