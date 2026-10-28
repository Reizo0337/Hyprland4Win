use std::path::PathBuf;

use hypr_win::config::Config;
use hypr_win::{app, bar, keybinds, window};
use windows::core::BOOL;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, PostMessageW, TranslateMessage, MSG, WM_CLOSE};

fn find_config() -> Option<PathBuf> {
    if let Some(arg) = std::env::args().nth(1) {
        return Some(PathBuf::from(arg));
    }
    let beside_exe = std::env::current_exe().ok()?.parent()?.join("config.toml");
    [beside_exe, PathBuf::from("config.toml")].into_iter().find(|p| p.is_file())
}

fn load_config() -> Result<Config, String> {
    match find_config() {
        Some(path) => {
            println!("config: {}", path.display());
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            Config::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
        }
        None => {
            println!("config: valores por defecto embebidos");
            Ok(Config::default_config())
        }
    }
}

unsafe extern "system" fn ctrl_handler(_: u32) -> BOOL {
    // Ctrl+C / cierre de consola: cerramos limpio para restaurar las ventanas.
    let _ = PostMessageW(Some(bar::hwnd()), WM_CLOSE, Default::default(), Default::default());
    BOOL(1)
}

fn run() -> Result<(), String> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let cfg = load_config()?;
    let bar_hwnd: HWND = bar::create(&cfg.bar, cfg.general.workspaces)?;
    // La barra se registra primero para que el área de trabajo ya la excluya.
    bar::register_appbar(bar_hwnd);

    app::install(app::build(cfg)?);

    let binds = app::bindings();
    for f in keybinds::register_all(bar_hwnd, &binds) {
        eprintln!("aviso: no se pudo registrar el atajo {f}");
    }
    let hooks = window::install_hooks(bar_hwnd);
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true);
    }
    app::adopt_existing();
    println!("hypr-win activo. Super+Shift+E (o Ctrl+C) para salir.");

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    window::remove_hooks(hooks);
    keybinds::unregister_all(bar_hwnd, app::binding_count());
    app::shutdown();
    bar::unregister_appbar(bar_hwnd);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
