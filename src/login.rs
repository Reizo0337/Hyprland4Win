//! Inicio de sesión de Windows. El diseño (campo de contraseña, botones, usuario) lo dibuja Windows y no se
//! puede sustituir con APIs seguras y documentadas. Lo que sí se puede controlar es:
//!
//! * la imagen (la de la pantalla de bloqueo, que es la que se ve al iniciar sesión; ver `wallpaper`), y
//! * la directiva de grupo documentada «Mostrar fondo de inicio de sesión nítido»
//!   (`DisableAcrylicBackgroundOnLogon`), que quita el desenfoque del fondo. Es una directiva de equipo
//!   (HKLM), así que cambiarla exige administrador y Windows muestra su aviso de UAC.

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};

use crate::installer::{self, Outcome};

const POLICY_KEY: &str = r"HKLM\SOFTWARE\Policies\Microsoft\Windows\System";
const VALUE: &str = "DisableAcrylicBackgroundOnLogon";

/// ¿Está activa la directiva que quita el desenfoque? (`None` si no está definida.)
pub fn clear_background() -> Option<bool> {
    let mut data = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Policies\Microsoft\Windows\System"),
            PCWSTR(windows::core::HSTRING::from(VALUE).as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    (r.0 == 0).then_some(data != 0)
}

/// Argumentos de `reg.exe` para activar (`add`) o quitar (`delete`) la directiva.
pub fn reg_args(on: bool) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
    if on {
        s(&["add", POLICY_KEY, "/v", VALUE, "/t", "REG_DWORD", "/d", "1", "/f"])
    } else {
        s(&["delete", POLICY_KEY, "/v", VALUE, "/f"])
    }
}

/// Activa o quita la directiva con permisos de administrador (aviso de UAC). Bloquea hasta que termina.
pub fn set_clear_background(on: bool) -> Outcome {
    let reg = std::env::var_os("SystemRoot").map(|r| std::path::Path::new(&r).join(r"System32\reg.exe"));
    match reg.filter(|p| p.is_file()) {
        Some(p) => installer::run_elevated(&p.to_string_lossy(), &reg_args(on)),
        None => Outcome::Failed("no se encontró reg.exe".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_arguments_target_the_documented_policy() {
        let on = reg_args(true);
        assert_eq!(on[0], "add");
        assert!(on.contains(&r"HKLM\SOFTWARE\Policies\Microsoft\Windows\System".to_string()));
        assert!(on.windows(2).any(|w| w == ["/v", "DisableAcrylicBackgroundOnLogon"]));
        assert!(on.windows(2).any(|w| w == ["/d", "1"]) && on.last().map(String::as_str) == Some("/f"));
        let off = reg_args(false);
        assert_eq!(off[0], "delete");
        assert!(!off.contains(&"/d".to_string()));
    }

    #[test]
    fn reading_the_policy_never_panics() {
        // solo lectura: devuelve Some(..) si hay directiva y None si no
        let _ = clear_background();
    }
}
