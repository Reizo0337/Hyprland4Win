//! Estado de la batería (`GetSystemPowerStatus`).

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    /// Conectado a la corriente.
    pub ac: bool,
}

/// `None` si el equipo no tiene batería.
pub fn battery() -> Option<Battery> {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).ok()? };
    if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 {
        return None;
    }
    Some(Battery { percent: s.BatteryLifePercent, charging: s.BatteryFlag & 8 != 0, ac: s.ACLineStatus == 1 })
}
