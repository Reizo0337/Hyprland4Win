//! Estado del sistema que muestra la barra: Wi-Fi, volumen y batería.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use crate::{audio, bluetooth, power, wifi};

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub wifi: wifi::Status,
    pub volume: Option<audio::Volume>,
    pub battery: Option<power::Battery>,
    pub bluetooth: bluetooth::Status,
}

impl Status {
    /// Arcos de Wi-Fi encendidos (0-3) según la señal; `None` si no hay adaptador.
    pub fn wifi_lit(&self) -> Option<u8> {
        if !self.wifi.present {
            return None;
        }
        Some(match (&self.wifi.ssid, self.wifi.connected) {
            (Some(_), _) => lit_from_signal(self.wifi.signal),
            (None, true) => 3, // conectado pero Windows oculta la señal (sin permiso de ubicación)
            (None, false) => 0,
        })
    }
}

/// Señal 0-100 -> arcos encendidos.
pub fn lit_from_signal(signal: u32) -> u8 {
    match signal {
        67.. => 3,
        34..=66 => 2,
        _ => 1,
    }
}

/// Lee el estado actual (requiere COM en el hilo que llama; unos pocos milisegundos).
pub fn poll() -> Status {
    // Bluetooth cuesta más (WinRT): se relee cada 3 sondeos (~9 s) y al abrir el panel
    let n = TICK.fetch_add(1, Ordering::Relaxed);
    let cached = BT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let bt = match cached {
        Some(c) if !n.is_multiple_of(3) => c,
        _ => refresh_bluetooth(),
    };
    Status { wifi: wifi::status(), volume: audio::get(), battery: power::battery(), bluetooth: bt }
}

static TICK: AtomicU32 = AtomicU32::new(0);
static BT: Mutex<Option<bluetooth::Status>> = Mutex::new(None);

/// Vuelve a leer Bluetooth ahora y actualiza la caché.
pub fn refresh_bluetooth() -> bluetooth::Status {
    let s = bluetooth::status();
    *BT.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_levels() {
        assert_eq!(lit_from_signal(100), 3);
        assert_eq!(lit_from_signal(67), 3);
        assert_eq!(lit_from_signal(50), 2);
        assert_eq!(lit_from_signal(10), 1);
    }

    #[test]
    fn wifi_lit_states() {
        let mut s = Status::default();
        assert_eq!(s.wifi_lit(), None); // sin adaptador
        s.wifi.present = true;
        assert_eq!(s.wifi_lit(), Some(0)); // sin conexión
        s.wifi.connected = true;
        assert_eq!(s.wifi_lit(), Some(3)); // conectado, señal oculta
        s.wifi.ssid = Some("x".into());
        s.wifi.signal = 80;
        assert_eq!(s.wifi_lit(), Some(3));
    }
}
