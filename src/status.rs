//! Estado del sistema que muestra la barra: Wi-Fi, volumen y batería.

use crate::{audio, power, wifi};

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub wifi: wifi::Status,
    pub volume: Option<audio::Volume>,
    pub battery: Option<power::Battery>,
}

impl Status {
    /// Arcos de Wi-Fi encendidos (0-3) según la señal; `None` si no hay adaptador.
    pub fn wifi_lit(&self) -> Option<u8> {
        if !self.wifi.present {
            return None;
        }
        Some(if self.wifi.ssid.is_none() { 0 } else { lit_from_signal(self.wifi.signal) })
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
    Status { wifi: wifi::status(), volume: audio::get(), battery: power::battery() }
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
        s.wifi.ssid = Some("x".into());
        s.wifi.signal = 80;
        assert_eq!(s.wifi_lit(), Some(3));
    }
}
