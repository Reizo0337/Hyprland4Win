//! Cambio de ventana (Alt+Tab propio) real: la lista aparece pasado un instante, se puede cambiar la elegida y
//! se oculta al terminar. Un toque rápido (terminar antes del retardo) no muestra nada.

use std::time::{Duration, Instant};

use hypr_win::config::Config;
use hypr_win::layout::Rect;
use hypr_win::switcher::{self, ItemSpec};
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowExW, GetWindowRect, IsWindowVisible, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};
use windows::Win32::Foundation::RECT;

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

fn window() -> HWND {
    unsafe { FindWindowExW(None, None, &HSTRING::from(switcher::CLASS_NAME), None).expect("la ventana del cambio de ventana") }
}

fn spec(title: &str) -> ItemSpec {
    ItemSpec { title: title.into(), name: title.into(), exe: None, aumid: None, minimized: false }
}

#[test]
fn the_switcher_shows_after_a_moment_and_hides_when_done() {
    let cfg = Config::default_config();
    switcher::create(&cfg.bar).expect("crear el cambio de ventana");
    let w = window();
    let mon = Rect::new(0, 0, 1920, 1080);
    let items = vec![spec("Visual Studio Code"), spec("Explorador de archivos"), spec("Brave"), spec("Terminal")];

    // un toque rápido: se termina antes del retardo y no llega a mostrarse
    switcher::begin(items.clone(), 1, mon);
    pump(30);
    switcher::end();
    pump(250);
    assert!(!unsafe { IsWindowVisible(w).as_bool() }, "un Alt+Tab rápido no enseña la lista");

    // manteniendo Alt: aparece, centrada en el monitor
    switcher::begin(items, 1, mon);
    assert!(!unsafe { IsWindowVisible(w).as_bool() }, "al principio no se ve");
    pump(350);
    assert!(unsafe { IsWindowVisible(w).as_bool() }, "pasado el retardo, la lista aparece");
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(w, &mut r);
    }
    let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    assert!((cx - 960).abs() <= 2 && (cy - 540).abs() <= 2, "centrada en el monitor, está en ({cx}, {cy})");
    assert!(r.right - r.left > 300 && r.bottom - r.top > 100);

    // cambiar la elegida no la cierra
    switcher::select(2);
    switcher::select(3);
    pump(50);
    assert!(unsafe { IsWindowVisible(w).as_bool() });

    // terminar la oculta
    switcher::end();
    pump(100);
    assert!(!unsafe { IsWindowVisible(w).as_bool() });
    println!("cambio de ventana: retardo, centrado, selección y cierre funcionan");
}
