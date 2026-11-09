//! Una ventana minimizada no se pierde: sigue en el Alt+Tab, vuelve al mosaico al restaurarla y se puede
//! restaurar eligiéndola en el Alt+Tab. Usa una ventana propia (no toca tus ventanas) y entrega los eventos
//! a la aplicación a mano, como haría el hook de Windows.

use hypr_win::config::Config;
use hypr_win::keybinds::{SWITCH_CONFIRM, SWITCH_START};
use hypr_win::{app, window};
use windows::core::w;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, IsIconic, ShowWindow, EVENT_OBJECT_DESTROY, EVENT_OBJECT_SHOW, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, SW_MINIMIZE, SW_RESTORE, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

fn count(list: &[isize], h: HWND) -> usize {
    list.iter().filter(|&&x| x == window::id(h)).count()
}

#[test]
fn minimized_windows_stay_in_alt_tab_and_come_back() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    app::install(app::build(Config::default_config(), window::monitors()).expect("crear la aplicación"));

    let h = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), w!("Static"), w!("hypr-win prueba minimizar"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 120, 120, 420, 320, None, None, None, None)
            .expect("crear la ventana de prueba")
    };
    let id = window::id(h);
    assert!(window::is_manageable(h), "la ventana de prueba debería ser gestionable");

    // se abre: entra en el mosaico y en el Alt+Tab
    app::on_event(EVENT_OBJECT_SHOW, id);
    assert_eq!(count(&app::switch_list(), h), 1, "la ventana abierta sale en el Alt+Tab");

    // se minimiza: sale del mosaico pero NO se pierde
    unsafe {
        let _ = ShowWindow(h, SW_MINIMIZE);
    }
    app::on_event(EVENT_SYSTEM_MINIMIZESTART, id);
    assert!(unsafe { IsIconic(h).as_bool() });
    assert_eq!(count(&app::switch_list(), h), 1, "una ventana minimizada sigue en el Alt+Tab");
    // minimizarla otra vez (dos eventos seguidos) no la duplica
    app::on_event(EVENT_SYSTEM_MINIMIZESTART, id);
    assert_eq!(count(&app::switch_list(), h), 1, "sin duplicados");

    // se restaura a mano: vuelve a gestionarse, una sola vez
    unsafe {
        let _ = ShowWindow(h, SW_RESTORE);
    }
    app::on_event(EVENT_SYSTEM_MINIMIZEEND, id);
    assert!(!unsafe { IsIconic(h).as_bool() });
    assert_eq!(count(&app::switch_list(), h), 1, "restaurada: una sola vez en la lista");

    // se minimiza otra vez y se restaura eligiéndola en el Alt+Tab
    unsafe {
        let _ = ShowWindow(h, SW_MINIMIZE);
    }
    app::on_event(EVENT_SYSTEM_MINIMIZESTART, id);
    assert!(unsafe { IsIconic(h).as_bool() });
    app::on_switch(SWITCH_START);
    app::on_switch(SWITCH_CONFIRM);
    assert!(!unsafe { IsIconic(h).as_bool() }, "elegirla en el Alt+Tab la restaura");
    app::on_event(EVENT_SYSTEM_MINIMIZEEND, id);
    assert_eq!(count(&app::switch_list(), h), 1);

    // se cierra estando minimizada: desaparece de la lista
    unsafe {
        let _ = ShowWindow(h, SW_MINIMIZE);
    }
    app::on_event(EVENT_SYSTEM_MINIMIZESTART, id);
    unsafe {
        let _ = DestroyWindow(h);
    }
    app::on_event(EVENT_OBJECT_DESTROY, id);
    assert_eq!(count(&app::switch_list(), h), 0, "una ventana cerrada no queda en el Alt+Tab");
    println!("minimizadas: siguen en el Alt+Tab, se restauran y se olvidan al cerrarlas");
}
