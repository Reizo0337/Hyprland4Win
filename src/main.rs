// Sin ventana de consola: al arrancar solo con Windows no debe abrirse ningún terminal. Si lo lanzas desde uno,
// `attach_parent_console` vuelve a conectar la salida de texto a ese terminal.
#![windows_subsystem = "windows"]

use std::path::PathBuf;

use hypr_win::config::Config;
use hypr_win::{app, apps, bar, cursor, dock, keybinds, launcher, panel, toast, widgets, window};
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
            hypr_win::config::set_path(std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone()));
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
    // `--autostart=on|off`: activa o desactiva el inicio con Windows y sale
    if let Some(v) = std::env::args().find_map(|a| a.strip_prefix("--autostart=").map(str::to_string)) {
        if let Some(path) = find_config() {
            hypr_win::config::set_path(std::fs::canonicalize(&path).unwrap_or(path));
        }
        match v.as_str() {
            "on" => hypr_win::autostart::enable()?,
            "off" => hypr_win::autostart::disable()?,
            _ => return Err("--autostart debe ser on u off".into()),
        }
        println!("inicio con Windows: {}", if v == "on" { "activado" } else { "desactivado" });
        return Ok(());
    }
    // lanzado por el inicio de sesión: esperar a que el escritorio esté listo
    if std::env::args().any(|a| a == "--autostart") {
        hypr_win::autostart::wait_for_shell(std::time::Duration::from_secs(30));
    }
    // si nos lanzó una actualización: esperar al proceso anterior y borrar su exe
    hypr_win::updater::on_startup();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // COM en el hilo de la interfaz: audio (Core Audio) e iconos del shell
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let cfg = load_config()?;
    if let Err(e) = cursor::apply(&cfg.bar) {
        eprintln!("aviso: no se pudo aplicar el cursor del tema: {e}");
    }
    let hide_taskbar = cfg.general.hide_taskbar;
    let check_updates = cfg.updates.check;
    let block_start = cfg.general.block_start_menu;
    let alt_tab = cfg.general.alt_tab;
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
    let main_hwnd: HWND = bar::create_all(&cfg.bar, cfg.general.workspaces, &mons)?;
    launcher::create(&cfg.bar)?;
    panel::create(&cfg.bar)?;
    hypr_win::switcher::create(&cfg.bar)?;
    if let Some(m) = mons.first() {
        toast::create(&cfg.bar, m.rect, bar::height_px())?;
    }
    apps::start_indexing();
    // Las barras se registran primero para que el área de trabajo de cada monitor ya las excluya.
    bar::register_all();

    let dock_cfg = cfg.dock.clone();
    let cfg_workspaces = cfg.general.workspaces;
    let bar_cfg = cfg.bar.clone();
    app::install(app::build(cfg, window::monitors())?);
    {
        let mons: Vec<_> = window::monitors().iter().map(|m| (m.id, m.rect)).collect();
        widgets::create(&bar_cfg, &mons)?;
        unsafe {
            SetTimer(Some(main_hwnd), bar::TIMER_WIDGETS, 1000, None);
        }
    }
    app::start_animated();
    hypr_win::updater::check_on_start(check_updates);
    if dock_cfg.enabled {
        let mut mons: Vec<_> = window::monitors().iter().map(|m| (m.id, m.rect)).collect();
        mons.truncate(cfg_workspaces);
        dock::create(&bar_cfg, &dock_cfg, &mons)?;
    }

    let binds = app::bindings();
    for f in keybinds::register_all(main_hwnd, &binds) {
        eprintln!("aviso: no se pudo registrar el atajo {f}");
    }
    let key_hook = keybinds::install_hook(main_hwnd, &binds, block_start)
        .map_err(|e| eprintln!("aviso: hook de teclado no disponible, los atajos con Super no funcionarán: {e}"))
        .ok();
    keybinds::set_alt_tab(alt_tab);
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
    cursor::restore();
    if let Some(prev) = restore_min_animate {
        window::set_min_animate(prev);
    }
    bar::unregister_all();
    if hide_taskbar {
        window::set_taskbar_visible(true);
    }
    Ok(())
}

/// Conecta `println!`/`eprintln!` y Ctrl+C a la consola desde la que se lanzó (si hay una). Devuelve si la hay.
fn attach_parent_console() -> bool {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS).is_ok() }
}

/// Un error fatal sin consola (inicio automático) no puede perderse: se escribe en `%APPDATA%\hypr-win\hypr-win.log`.
fn log_fatal(msg: &str) {
    let Some(dir) = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("hypr-win")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let line = format!("[{}] error: {msg}\n", hypr_win::updater::unix_secs());
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("hypr-win.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn main() {
    let has_console = attach_parent_console();
    if let Err(e) = run() {
        eprintln!("error: {e}");
        log_fatal(&e);
        if !has_console {
            // sin terminal nadie vería el motivo: se muestra un aviso
            use windows::core::HSTRING;
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            unsafe {
                MessageBoxW(None, &HSTRING::from(format!("{e}\n\nMás detalles en %APPDATA%\\hypr-win\\hypr-win.log")), &HSTRING::from("hypr-win no pudo iniciarse"), MB_OK | MB_ICONERROR);
            }
        }
        std::process::exit(1);
    }
}
