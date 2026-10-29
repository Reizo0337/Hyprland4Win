//! Prueba de integración: el índice de aplicaciones del sistema se construye (solo lectura).

use std::time::{Duration, Instant};

use hypr_win::{apps, search};

#[test]
fn indexes_system_apps() {
    apps::start_indexing();
    let deadline = Instant::now() + Duration::from_secs(20);
    while apps::entries().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let entries = apps::entries();
    assert!(!entries.is_empty(), "shell:AppsFolder no devolvió aplicaciones");
    assert!(entries.iter().all(|e| !e.name.is_empty() && !e.id.is_empty()));
    println!("{} aplicaciones; primeras: {:?}", entries.len(), entries.iter().take(5).map(|e| &e.name).collect::<Vec<_>>());

    // la búsqueda encuentra una entrada real a partir de su propio nombre
    let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    let hit = search::rank(&names[0], &names, 5);
    assert!(hit.contains(&0));
}
