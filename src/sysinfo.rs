//! Estadísticas del sistema para el widget "Sistema": CPU, memoria, disco y red. Solo APIs documentadas
//! de Win32 (`GetSystemTimes`, `GlobalMemoryStatusEx`, `GetDiskFreeSpaceExW`, `GetIfTable2`).

use std::time::Instant;

use windows::core::w;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::widget_draw::Stats;

const GB: f64 = 1024.0 * 1024.0 * 1024.0;
/// IF_TYPE_SOFTWARE_LOOPBACK y IF_TYPE_TUNNEL: no cuentan como tráfico de red real.
const SKIP_TYPES: [u32; 2] = [24, 131];

fn ft(f: FILETIME) -> u64 {
    (f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64
}

/// Porcentaje de CPU ocupada entre dos lecturas (idle, kernel, user); el tiempo de kernel incluye el de reposo.
pub fn cpu_percent(prev: (u64, u64, u64), now: (u64, u64, u64)) -> f32 {
    let idle = now.0.saturating_sub(prev.0) as f64;
    let total = (now.1.saturating_sub(prev.1) + now.2.saturating_sub(prev.2)) as f64;
    if total <= 0.0 {
        return 0.0;
    }
    (((total - idle) / total) * 100.0).clamp(0.0, 100.0) as f32
}

/// Bytes por segundo entre dos lecturas del contador de una interfaz.
pub fn rate(prev: u64, now: u64, secs: f64) -> f64 {
    if secs <= 0.0 {
        return 0.0;
    }
    now.saturating_sub(prev) as f64 / secs
}

#[derive(Default)]
pub struct Sampler {
    cpu: Option<(u64, u64, u64)>,
    net: Option<(u64, u64, Instant)>,
}

fn cpu_times() -> Option<(u64, u64, u64)> {
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()? };
    Some((ft(idle), ft(kernel), ft(user)))
}

/// Suma de bytes recibidos y enviados por las interfaces activas (sin loopback ni túneles).
fn net_octets() -> Option<(u64, u64)> {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).0 != 0 || table.is_null() {
            return None;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let (mut rx, mut tx) = (0u64, 0u64);
        for r in rows {
            // IfOperStatusUp = 1
            if r.OperStatus.0 == 1 && !SKIP_TYPES.contains(&r.Type) {
                rx += r.InOctets;
                tx += r.OutOctets;
            }
        }
        FreeMibTable(table as *const _);
        Some((rx, tx))
    }
}

impl Sampler {
    /// Lee las estadísticas actuales; la CPU y la red necesitan dos lecturas (la primera da 0).
    pub fn sample(&mut self) -> Stats {
        let mut s = Stats::default();

        if let Some(now) = cpu_times() {
            if let Some(prev) = self.cpu {
                s.cpu = cpu_percent(prev, now);
            }
            self.cpu = Some(now);
        }

        let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        if unsafe { GlobalMemoryStatusEx(&mut mem) }.is_ok() {
            let total = mem.ullTotalPhys as f64;
            let used = total - mem.ullAvailPhys as f64;
            s.mem_pct = mem.dwMemoryLoad as f32;
            s.mem_total_gb = (total / GB) as f32;
            s.mem_used_gb = (used / GB) as f32;
        }

        let (mut free, mut total) = (0u64, 0u64);
        if unsafe { GetDiskFreeSpaceExW(w!("C:\\"), Some(&mut free), Some(&mut total), None) }.is_ok() && total > 0 {
            s.disk_free_gb = (free as f64 / GB) as f32;
            s.disk_pct = ((total - free) as f64 / total as f64 * 100.0) as f32;
        }

        if let Some((rx, tx)) = net_octets() {
            let now = Instant::now();
            if let Some((prx, ptx, t0)) = self.net {
                let secs = now.duration_since(t0).as_secs_f64();
                s.net_down = rate(prx, rx, secs);
                s.net_up = rate(ptx, tx, secs);
            }
            self.net = Some((rx, tx, now));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percentage_from_deltas() {
        // 1000 de tiempo total, 750 en reposo -> 25 % ocupada
        assert_eq!(cpu_percent((0, 0, 0), (750, 800, 200)), 25.0);
        assert_eq!(cpu_percent((100, 200, 100), (100, 200, 100)), 0.0); // sin avance
        assert_eq!(cpu_percent((0, 0, 0), (0, 500, 500)), 100.0);
        assert_eq!(cpu_percent((10, 10, 10), (5, 5, 5)), 0.0); // el contador nunca retrocede: sin pánico
    }

    #[test]
    fn network_rate() {
        assert_eq!(rate(1000, 3000, 2.0), 1000.0);
        assert_eq!(rate(5, 5, 1.0), 0.0);
        assert_eq!(rate(10, 5, 1.0), 0.0);
        assert_eq!(rate(0, 100, 0.0), 0.0);
    }
}
