//! Lecturas reales de CPU, memoria, disco y red (solo lectura).

use std::time::Duration;

use hypr_win::sysinfo::Sampler;

#[test]
fn reads_real_system_stats() {
    let mut s = Sampler::default();
    let first = s.sample();
    // memoria y disco salen a la primera
    assert!(first.mem_total_gb > 0.5 && first.mem_total_gb < 4096.0, "memoria total: {}", first.mem_total_gb);
    assert!((0.0..=100.0).contains(&first.mem_pct));
    assert!(first.disk_free_gb > 0.0 && (0.0..=100.0).contains(&first.disk_pct), "disco: {first:?}");
    // CPU y red necesitan una segunda lectura
    std::thread::sleep(Duration::from_millis(1200));
    let second = s.sample();
    println!(
        "CPU {:.0} %, memoria {:.1}/{:.0} GB ({:.0} %), disco {:.0} GB libres ({:.0} %), red ↓{:.0} B/s ↑{:.0} B/s",
        second.cpu, second.mem_used_gb, second.mem_total_gb, second.mem_pct, second.disk_free_gb, second.disk_pct, second.net_down, second.net_up
    );
    assert!((0.0..=100.0).contains(&second.cpu));
    assert!(second.net_down >= 0.0 && second.net_up >= 0.0);
    assert!(second.mem_used_gb <= second.mem_total_gb);
}
