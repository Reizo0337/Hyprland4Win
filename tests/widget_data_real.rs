//! Datos reales de los widgets: tiempo (Open-Meteo, necesita red) y música (sesiones multimedia de Windows,
//! solo lectura). Las pruebas de red se omiten sin conexión.

use hypr_win::{media, weather};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

#[test]
fn weather_for_a_real_city() {
    let Some(w) = weather::fetch("Madrid") else {
        eprintln!("sin red o servicio no disponible: se omite");
        return;
    };
    println!("tiempo en {}: {}° (código {}), {} días de previsión", w.city, w.temp, w.code, w.days.len());
    assert!(w.city.contains("Madrid"));
    assert!((-40..=55).contains(&w.temp), "temperatura absurda: {}", w.temp);
    assert_eq!(w.days.len(), 3);
    assert!(w.days.iter().all(|d| d.lo <= d.hi && !d.label.is_empty()));
}

#[test]
fn unknown_city_gives_nothing() {
    // sin red también da None, así que solo importa que no entre en pánico
    assert!(weather::fetch("zzqxjkvv ciudad inexistente").is_none());
}

#[test]
fn media_session_can_be_read() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    // no se comprueba el contenido: depende de si hay algo sonando
    match media::poll_once() {
        Some(m) => println!("suena algo: reproduciendo={}, con carátula={}", m.playing, m.thumb.is_some()),
        None => println!("no hay ninguna sesión multimedia activa"),
    }
}
