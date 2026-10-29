use std::path::PathBuf;

use hypr_win::config::Config;
use hypr_win::{app, apps, bar, dock, keybinds, launcher, panel, window};
use windows::core::BOOL;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, PostMessageW, TranslateMessage, SetTimer, MSG, WM_CLOSE};

fn find_config() -> Option<PathBuf> {
    if let Some(arg) = std::env::args().skip(1).find(|a| !a.starts_with("--")) {
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
    // Recuperación: si hypr-win se cerró a la fuerza con la barra de tareas oculta.
    if std::env::args().any(|a| a == "--restore-taskbar") {
        window::set_taskbar_visible(true);
        println!("barra de tareas restaurada");
        return Ok(());
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // COM en el hilo de la interfaz: audio (Core Audio) e iconos del shell
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let cfg = load_config()?;
    let hide_taskbar = cfg.general.hide_taskbar;
    let block_start = cfg.general.block_start_menu;
    // Opcional: sin la animación de minimizar/maximizar de Windows (no se guarda; se restaura al salir).
    let restore_min_animate = if cfg.general.disable_windows_animations {
        let prev = window::min_animate();
        window::set_min_animate(false);
        prev
    } else {
        None
    };
    // Un workspace visible por monitor: se gestionan como mucho tantos monitores como workspaces.
    let mut mons = window::monitors();
    mons.truncate(cfg.general.workspaces);
    println!("monitores gestionados: {}", mons.len());
    let bars = bar::create_all(&cfg.bar, cfg.general.workspaces, &mons)?;
    let main_hwnd: HWND = bars[0];
    launcher::create(&cfg.bar)?;
    panel::create(&cfg.bar)?;
    apps::start_indexing();
    // Las barras se registran primero para que el área de trabajo de cada monitor ya las excluya.
    bar::register_all();

    let dock_cfg = cfg.dock.clone();
    let cfg_workspaces = cfg.general.workspaces;
    let bar_cfg = cfg.bar.clone();
    app::install(app::build(cfg, window::monitors())?);
    if dock_cfg.enabled {
        let mut rects: Vec<_> = window::monitors().iter().map(|m| m.rect).collect();
        rects.truncate(cfg_workspaces);
        dock::create(&bar_cfg, &dock_cfg, &rects)?;
    }

    let binds = app::bindings();
    for f in keybinds::register_all(main_hwnd, &binds) {
        eprintln!("aviso: no se pudo registrar el atajo {f}");
    }
    let key_hook = keybinds::install_hook(main_hwnd, &binds, block_start)
        .map_err(|e| eprintln!("aviso: hook de teclado no disponible, los atajos con Super no funcionarán: {e}"))
        .ok();
    let hooks = window::install_hooks(main_hwnd);
    unsafe {
        let _ = SetConsoleCtrlHandler(Some(ctrl_handler), true);
    }
    if hide_taskbar {
        window::set_taskbar_visible(false);
        unsafe {
            SetTimer(Some(main_hwnd), bar::TIMER_TASKBAR, 1000, None);
        }
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

    if let Some(h) = key_hook {
        keybinds::remove_hook(h);
    }
    window::remove_hooks(hooks);
    keybinds::unregister_all(main_hwnd, app::binding_count());
    app::shutdown();
    if let Some(prev) = restore_min_animate {
        window::set_min_animate(prev);
    }
    bar::unregister_all();
    if hide_taskbar {
        window::set_taskbar_visible(true);
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
