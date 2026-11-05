//! Ventana de notificaciones real (se verá una tarjeta de prueba un par de segundos) y ejecución oculta de winget.

use std::time::{Duration, Instant};

use hypr_win::config::Config;
use hypr_win::installer;
use hypr_win::{packages, toast, window};
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, IsWindowVisible, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

fn pump(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    unsafe {
        let mut msg = MSG::default();
        while Instant::now() < end {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn toast_visible() -> bool {
    unsafe { FindWindowW(&HSTRING::from(toast::CLASS_NAME), None).is_ok_and(|h| IsWindowVisible(h).as_bool()) }
}

#[test]
fn toast_window_shows_updates_and_expires() {
    let cfg = Config::default_config();
    let mon = window::monitors().first().expect("hay un monitor").rect;
    toast::create(&cfg.bar, mon, 28).expect("crear la ventana de notificaciones");

    // se publica desde otro hilo, como hace el instalador
    let id = toast::next_id();
    std::thread::spawn(move || toast::upsert(id, '\u{E896}', toast::BLUE, "Instalando Prueba", "en segundo plano · winget", None)).join().unwrap();
    pump(600);
    assert!(toast_visible(), "la notificación no apareció");

    // se actualiza la misma tarjeta (no se duplica) con caducidad
    toast::upsert(id, '\u{E73E}', toast::GREEN, "Prueba instalada", "Ya aparece en el lanzador", Some(Duration::from_millis(700)));
    pump(300);
    assert!(toast_visible());
    pump(1400);
    assert!(!toast_visible(), "la notificación debería haber caducado");

    // las que no caducan se quitan a mano
    toast::upsert(id, '\u{E783}', toast::RED, "Falló", "código 0x00000001", None);
    pump(400);
    assert!(toast_visible());
    toast::dismiss(id);
    pump(400);
    assert!(!toast_visible());
}

#[test]
fn winget_runs_hidden_with_captured_output() {
    let Some(exe) = packages::winget_path() else {
        eprintln!("sin winget: se omite");
        return;
    };
    let (code, out) = installer::run_captured(&exe, &["--version".to_string()]).expect("ejecutar winget");
    println!("winget --version -> código {code}, salida {:?}", out.trim());
    assert_eq!(code, 0);
    assert!(out.trim_start().starts_with('v'), "salida inesperada: {out:?}");
}
