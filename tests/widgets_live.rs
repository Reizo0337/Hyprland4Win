//! Widgets de escritorio reales: se abren unas ventanas translúcidas detrás de todo durante un par de segundos.
//! `APPDATA` se redirige a una carpeta temporal, así que no toca tu disposición ni tus notas.

use std::time::{Duration, Instant};

use hypr_win::config::Config;
use hypr_win::widget_draw::{Kind, Size};
use hypr_win::{widgets, window};
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowExW, GetWindowLongW, GetWindowThreadProcessId, IsWindowVisible, PeekMessageW, TranslateMessage, GWL_EXSTYLE, MSG, PM_REMOVE,
    WS_EX_TOPMOST,
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
            widgets::tick();
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn windows_of_class() -> Vec<HWND> {
    let mut v = Vec::new();
    let class = HSTRING::from(widgets::CLASS_NAME);
    let mut prev: Option<HWND> = None;
    unsafe {
        while let Ok(h) = FindWindowExW(None, prev, &class, None) {
            if h.0.is_null() {
                break;
            }
            // otra instancia de hypr-win (la tuya, en marcha) también tiene ventanas de esta clase
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == std::process::id() {
                v.push(h);
            }
            prev = Some(h);
        }
    }
    v
}

#[test]
fn widgets_open_persist_hide_peek_and_close() {
    let dir = std::env::temp_dir().join(format!("hypr-win-widgets-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_var("APPDATA", &dir);
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());

    let cfg = Config::default_config();
    let mons: Vec<_> = window::monitors().iter().map(|m| (m.id, m.rect)).collect();
    widgets::create(&cfg.bar, &mons).expect("crear los widgets");
    assert!(widgets::list().is_empty());

    let clock = widgets::add(Kind::Clock, Size::Small).unwrap();
    let _sys = widgets::add(Kind::System, Size::Medium).unwrap();
    let notes = widgets::add(Kind::Notes, Size::Medium).unwrap();
    assert!(widgets::add(Kind::System, Size::Small).is_err(), "el tamaño no existe para ese widget");
    widgets::set_note("Prueba de nota\ncon dos líneas");
    pump(1500);

    // tres ventanas visibles, cada una con su identificador
    let wins = windows_of_class();
    assert_eq!(wins.len(), 3, "debería haber 3 ventanas de widget");
    assert!(wins.iter().all(|&h| unsafe { IsWindowVisible(h).as_bool() }));
    assert!(wins.iter().all(|&h| unsafe { GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0 }), "viven detrás, no encima");

    // la disposición y la nota quedaron guardadas
    let saved = std::fs::read_to_string(dir.join(r"hypr-win\widgets.toml")).unwrap();
    assert_eq!(saved.matches("[[widgets]]").count(), 3, "{saved}");
    assert!(std::fs::read_to_string(dir.join(r"hypr-win\notes.txt")).unwrap().contains("dos líneas"));
    assert!(widgets::note().contains("Prueba de nota"));

    // el monitor desaparece: los widgets se ocultan SIN moverse ni cambiar de monitor, y vuelven donde estaban
    let before = widgets::list();
    assert!(before.iter().all(|p| p.mon_id != 0), "cada widget pertenece a un monitor concreto");
    widgets::set_monitors(&[]);
    pump(200);
    assert!(windows_of_class().iter().all(|&h| unsafe { !IsWindowVisible(h).as_bool() }), "sin monitor no se muestran");
    assert_eq!(widgets::list(), before, "la disposición no cambia al desconectar el monitor");
    widgets::set_monitors(&mons);
    pump(300);
    assert!(windows_of_class().iter().all(|&h| unsafe { IsWindowVisible(h).as_bool() }), "al volver el monitor reaparecen");
    assert_eq!(widgets::list(), before);

    // quitar uno
    widgets::remove(clock);
    pump(200);
    assert_eq!(windows_of_class().len(), 2);
    assert!(widgets::list().iter().all(|w| w.id != clock));

    // ocultar todos
    widgets::set_visible(false);
    pump(200);
    assert!(windows_of_class().iter().all(|&h| unsafe { !IsWindowVisible(h).as_bool() }));
    assert!(!widgets::visible());

    // "echar un vistazo": aparecen por encima aunque estén ocultos, y se vuelven a esconder
    widgets::toggle_peek();
    pump(300);
    let peeking = windows_of_class();
    assert!(peeking.iter().all(|&h| unsafe { IsWindowVisible(h).as_bool() && GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }));
    widgets::toggle_peek();
    pump(200);
    assert!(windows_of_class().iter().all(|&h| unsafe { !IsWindowVisible(h).as_bool() }));

    // se vuelven a mostrar y se restablecen las posiciones
    widgets::set_visible(true);
    widgets::reset_positions();
    pump(300);
    assert!(windows_of_class().iter().all(|&h| unsafe { IsWindowVisible(h).as_bool() }));
    widgets::remove(notes);
    pump(100);
    assert_eq!(windows_of_class().len(), 1);
    println!("widgets: abrir, guardar, quitar, ocultar, vistazo y restablecer funcionan");
}
