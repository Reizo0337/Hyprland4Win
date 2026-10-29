//! Los iconos reales de las aplicaciones del sistema se extraen y tienen contenido (solo lectura).

use std::time::{Duration, Instant};

use hypr_win::{apps, icons};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

#[test]
fn extracts_real_icons() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    apps::start_indexing();
    let deadline = Instant::now() + Duration::from_secs(20);
    while apps::entries().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let entries = apps::entries();
    assert!(!entries.is_empty());

    let mut ok = 0;
    for e in entries.iter().take(12) {
        if let Some(icon) = icons::from_apps_folder(&e.id) {
            assert!(icon.w > 0 && icon.h > 0);
            // algún píxel visible
            assert!(icon.pixels().iter().any(|p| p >> 24 > 0), "icono vacío: {}", e.name);
            ok += 1;
        }
    }
    println!("{ok} de 12 iconos extraídos");
    assert!(ok >= 6, "demasiados iconos fallaron: {ok}/12");

    // y un .exe por ruta
    let notepad = icons::from_path(r"C:\Windows\System32\notepad.exe");
    assert!(notepad.is_some());
}
