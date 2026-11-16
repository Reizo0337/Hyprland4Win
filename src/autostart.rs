//! Inicio automático con Windows: un valor en `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! (la vía documentada para programas de usuario; no necesita administrador y se ve y se desactiva también
//! desde Configuración > Aplicaciones > Inicio). El comando lleva la ruta del `config.toml` en uso, porque
//! al iniciar sesión el directorio actual no es el de siempre, y `--autostart`, que hace esperar a que el
//! escritorio (la barra de tareas) exista antes de empezar a gestionar ventanas.

use std::path::Path;
use std::time::{Duration, Instant};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "hypr-win";

// ---------- línea de comandos (pura) ----------

/// Cita un argumento al estilo de Windows si hace falta (espacios, comillas o vacío).
pub fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    // las comillas internas se escapan; las barras invertidas finales se duplican para no escapar la comilla de cierre
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// Comando que se guarda en `Run`: `"<exe>" ["<config>"] --autostart`.
pub fn command_line(exe: &Path, config: Option<&Path>) -> String {
    let mut s = quote(&exe.to_string_lossy());
    if let Some(c) = config {
        s.push(' ');
        s.push_str(&quote(&c.to_string_lossy()));
    }
    s.push_str(" --autostart");
    s
}

// ---------- registro ----------

fn utf16_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(|c| c.to_le_bytes()).collect()
}

fn open(key: &str, access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut hk = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, access, &mut hk).ok().ok()? };
    Some(hk)
}

fn exists_in(key: &str, name: &str) -> bool {
    let Some(hk) = open(key, KEY_QUERY_VALUE) else { return false };
    let n = HSTRING::from(name);
    let (mut ty, mut size) = (REG_VALUE_TYPE(0), 0u32);
    let found = unsafe { RegQueryValueExW(hk, PCWSTR(n.as_ptr()), None, Some(&mut ty), None, Some(&mut size)) }.is_ok();
    unsafe {
        let _ = RegCloseKey(hk);
    }
    found
}

fn set_in(key: &str, name: &str, command: &str) -> Result<(), String> {
    let hk = open(key, KEY_SET_VALUE).ok_or("no se pudo abrir la clave de inicio del registro")?;
    let n = HSTRING::from(name);
    let r = unsafe { RegSetValueExW(hk, PCWSTR(n.as_ptr()), None, REG_SZ, Some(&utf16_bytes(command))) };
    unsafe {
        let _ = RegCloseKey(hk);
    }
    r.ok().map_err(|e| e.to_string())
}

fn remove_in(key: &str, name: &str) -> Result<(), String> {
    let Some(hk) = open(key, KEY_SET_VALUE) else { return Ok(()) };
    let n = HSTRING::from(name);
    let r = unsafe { RegDeleteValueW(hk, PCWSTR(n.as_ptr())) };
    unsafe {
        let _ = RegCloseKey(hk);
    }
    // que no existiera ya es el resultado que se quería
    if exists_in(key, name) { r.ok().map_err(|e| e.to_string()) } else { Ok(()) }
}

// ---------- API ----------

/// ¿Está activado el inicio con Windows?
pub fn is_enabled() -> bool {
    exists_in(RUN_KEY, VALUE)
}

/// Activa el inicio con Windows para el exe actual (y el `config.toml` en uso, si lo hay).
pub fn enable() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let config = crate::config::path();
    set_in(RUN_KEY, VALUE, &command_line(&exe, config.as_deref()))
}

pub fn disable() -> Result<(), String> {
    remove_in(RUN_KEY, VALUE)
}

/// Con `--autostart`: espera (hasta `max`) a que el escritorio esté listo, es decir, a que exista la barra de
/// tareas de Windows. Si el programa arranca antes, no encuentra las ventanas del escritorio ni la barra que oculta.
pub fn wait_for_shell(max: Duration) {
    let start = Instant::now();
    while start.elapsed() < max {
        if unsafe { FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) }.is_ok() {
            // un respiro más: el Explorador sigue cargando iconos y la bandeja
            std::thread::sleep(Duration::from_millis(1500));
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn arguments_are_quoted_like_windows_expects() {
        assert_eq!(quote("simple"), "simple");
        assert_eq!(quote(r"C:\Program Files\x.exe"), r#""C:\Program Files\x.exe""#);
        assert_eq!(quote(""), r#""""#);
        assert_eq!(quote(r#"di "hola""#), r#""di \"hola\"""#);
        // las barras finales se duplican para no escapar la comilla de cierre
        assert_eq!(quote(r"C:\mi carpeta\"), r#""C:\mi carpeta\\""#);
        assert_eq!(quote(r"C:\sin\espacios"), r"C:\sin\espacios");
    }

    #[test]
    fn the_run_command_has_exe_config_and_the_autostart_flag() {
        let exe = PathBuf::from(r"D:\Hyprland Win\target\release\hypr-win.exe");
        let cfg = PathBuf::from(r"D:\Hyprland Win\config.toml");
        assert_eq!(command_line(&exe, Some(&cfg)), r#""D:\Hyprland Win\target\release\hypr-win.exe" "D:\Hyprland Win\config.toml" --autostart"#);
        assert_eq!(command_line(Path::new(r"C:\h\hypr-win.exe"), None), r"C:\h\hypr-win.exe --autostart");
    }

    #[test]
    fn registry_values_can_be_set_found_and_removed() {
        // clave propia de prueba: no se toca el `Run` real
        let key = r"Software\hypr-win-test-autostart";
        let created = unsafe {
            let mut hk = HKEY::default();
            let r = windows::Win32::System::Registry::RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &HSTRING::from(key),
                None,
                PCWSTR::null(),
                windows::Win32::System::Registry::REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                None,
                &mut hk,
                None,
            );
            if r.is_ok() {
                let _ = RegCloseKey(hk);
            }
            r.is_ok()
        };
        assert!(created, "no se pudo crear la clave de prueba");
        assert!(!exists_in(key, "x"));
        set_in(key, "x", r#""C:\a b\h.exe" --autostart"#).unwrap();
        assert!(exists_in(key, "x"));
        set_in(key, "x", "otro").unwrap(); // sobrescribir
        assert!(exists_in(key, "x"));
        remove_in(key, "x").unwrap();
        assert!(!exists_in(key, "x"));
        remove_in(key, "x").unwrap(); // borrar algo que no existe no es un error
        assert!(remove_in(r"Software\no-existe-hypr-win", "x").is_ok());
        unsafe {
            let _ = windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key));
        }
    }

    #[test]
    fn utf16_values_are_nul_terminated() {
        assert_eq!(utf16_bytes("a"), vec![b'a', 0, 0, 0]);
        assert_eq!(utf16_bytes("ñ").len(), 4);
    }
}
