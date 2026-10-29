//! Lógica del menú: atajos (formato y descripción), edición de config.toml y aplicaciones web reales.

use hypr_win::config::{replace_bind, Config};
use hypr_win::keybinds::{describe, format_keys, parse_binding, parse_keys, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};
use hypr_win::webapp;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

#[test]
fn key_combinations_round_trip() {
    for keys in ["Super+Return", "Super+Shift+Left", "Ctrl+Alt+F5", "Super+1", "Alt+Space", "Super+Ctrl+Shift+Q", "Super+Tab"] {
        let (mods, vk) = parse_keys(keys).unwrap();
        let back = format_keys(mods & !hypr_win::keybinds::MOD_NOREPEAT, vk).unwrap();
        assert_eq!(parse_keys(&back).unwrap(), (mods, vk), "{keys} -> {back}");
    }
    assert_eq!(format_keys(MOD_WIN | MOD_SHIFT, 0x25).unwrap(), "Super+Shift+Left");
    assert_eq!(format_keys(MOD_CONTROL | MOD_ALT, 0x70).unwrap(), "Ctrl+Alt+F1");
    assert_eq!(format_keys(MOD_WIN, 0xBA), None); // tecla sin nombre en la configuración
}

#[test]
fn every_default_binding_has_a_description() {
    let cfg = Config::default_config();
    for b in cfg.bindings().unwrap() {
        assert!(!describe(&b.action).is_empty());
    }
    assert_eq!(describe(&parse_binding("Super+3, workspace, 3").unwrap().action), "Ir al escritorio 3");
    assert_eq!(describe(&parse_binding("Super+Space, menu").unwrap().action), "Menú");
}

#[test]
fn default_menu_binding_and_layout_toggle_do_not_clash() {
    let binds = Config::default_config().bindings().unwrap();
    let mut seen = std::collections::HashSet::new();
    for b in &binds {
        assert!(seen.insert((b.mods, b.vk)), "atajo repetido: {b:?}");
    }
}

#[test]
fn bind_replacement_in_config_text() {
    let text = "binds = [\n    \"Super+Q, close\",\n    \"Super+F, fullscreen\",\n]\n";
    let out = replace_bind(text, "Super+Q, close", "Super+W, close").unwrap();
    assert!(out.contains("\"Super+W, close\"") && !out.contains("\"Super+Q, close\""));
    assert!(replace_bind(text, "Super+X, nada", "Super+Y, nada").is_err());
    let dup = "binds = [\"Super+Q, close\", \"Super+Q, close\"]";
    assert!(replace_bind(dup, "Super+Q, close", "Super+W, close").is_err());
    // el resultado sobre la configuración real sigue siendo válido
    let real = include_str!("../config.toml");
    let changed = replace_bind(real, "Super+Q, close", "Super+X, close").unwrap();
    assert!(Config::parse(&changed).is_ok());
}

/// Crea una aplicación web de verdad (acceso directo en el menú Inicio + favicon por red) y la borra.
#[test]
fn creates_and_removes_a_real_web_app() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    if webapp::browser().is_none() {
        eprintln!("sin Edge/Chrome/Brave: se omite");
        return;
    }
    let name = "HyprWin Prueba Temporal";
    // limpieza previa y posterior, pase lo que pase
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for a in webapp::list().iter().filter(|a| a.name == "HyprWin Prueba Temporal") {
                webapp::remove(a);
            }
        }
    }
    let _guard = Cleanup;

    let lnk = webapp::create(name, "youtube.com").expect("crear la app web");
    assert!(lnk.is_file(), "no existe el acceso directo");
    assert!(webapp::list().iter().any(|a| a.name == name));
    let ico = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join(format!(r"hypr-win\webapps\{name}.ico"));
    let size = std::fs::metadata(&ico).map_or(0, |m| m.len());
    println!("icono (favicon) descargado: {} ({size} bytes)", ico.is_file());
    assert!(size > 100, "el favicon de YouTube debería haberse descargado");

    let app = webapp::list().into_iter().find(|a| a.name == name).unwrap();
    webapp::remove(&app);
    assert!(!lnk.exists(), "no se borró el acceso directo");
}
