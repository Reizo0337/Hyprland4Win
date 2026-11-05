//! Instalación y actualización de programas en segundo plano, sin ventanas de consola. El progreso y el
//! resultado se muestran con notificaciones flotantes (`toast`).
//!
//! winget se ejecuta oculto y con salida capturada. Chocolatey necesita administrador: se lanza oculto con
//! el verbo `runas` (Windows muestra su aviso de UAC, que no se puede ocultar) y se espera a que termine.

use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::packages::{self, Manager};
use crate::toast;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Tiempo máximo de una instalación.
const LIMIT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Install,
    UpgradeAll,
}

#[derive(Clone, Debug)]
pub struct Job {
    pub manager: Manager,
    pub kind: Kind,
    /// Identificador del paquete (solo para `Install`).
    pub id: String,
    /// Nombre que se muestra en las notificaciones.
    pub name: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    Done,
    /// Ya estaba instalado o no hay nada que actualizar.
    UpToDate,
    /// El usuario rechazó el aviso de administrador.
    Cancelled,
    Failed(String),
}

/// Argumentos del gestor (se pasan por separado: no hay shell de por medio).
pub fn args_for(job: &Job) -> Vec<String> {
    let id = packages::sanitize_arg(&job.id);
    let v: Vec<&str> = match (job.manager, job.kind) {
        (Manager::Winget, Kind::Install) => vec![
            "install", "--id", &id, "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity",
        ],
        (Manager::Winget, Kind::UpgradeAll) => {
            vec!["upgrade", "--all", "--silent", "--accept-package-agreements", "--accept-source-agreements", "--disable-interactivity"]
        }
        (Manager::Choco, Kind::Install) => vec!["install", &id, "-y", "--no-progress"],
        (Manager::Choco, Kind::UpgradeAll) => vec!["upgrade", "all", "-y", "--no-progress"],
    };
    v.into_iter().map(String::from).collect()
}

/// Interpreta el código de salida.
pub fn classify(manager: Manager, code: i32) -> Outcome {
    match (manager, code as u32) {
        (_, 0) => Outcome::Done,
        // Chocolatey: correcto pero hace falta reiniciar
        (Manager::Choco, 1641 | 3010) => Outcome::Done,
        // winget: sin actualización aplicable / paquete ya instalado
        (Manager::Winget, 0x8A15_002B | 0x8A15_0061) => Outcome::UpToDate,
        (_, c) => Outcome::Failed(format!("código 0x{c:08X}")),
    }
}

/// Última línea con texto de la salida (sin caracteres de control), recortada para una notificación.
pub fn last_message(out: &str) -> Option<String> {
    out.lines()
        .rev()
        .map(|l| l.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string())
        .find(|l| l.chars().filter(|c| c.is_alphabetic()).count() >= 6)
        .map(|l| if l.chars().count() > 70 { format!("{}…", l.chars().take(69).collect::<String>()) } else { l })
}

/// Lanza el trabajo en un hilo y lo sigue con una notificación.
pub fn start(job: Job) {
    let _ = std::thread::Builder::new().name("hypr-win-installer".into()).spawn(move || {
        let id = toast::next_id();
        let (doing, done) = match job.kind {
            Kind::Install => (format!("Instalando {}", job.name), format!("{} instalado", job.name)),
            Kind::UpgradeAll => ("Actualizando aplicaciones".to_string(), "Aplicaciones actualizadas".to_string()),
        };
        let via = if job.manager == Manager::Choco { "Chocolatey" } else { "winget" };
        toast::upsert(id, '\u{E896}', toast::BLUE, &doing, &format!("en segundo plano · {via}"), None);

        match run(&job) {
            Outcome::Done => {
                toast::upsert(id, '\u{E73E}', toast::GREEN, &done, "Ya aparece en el lanzador", Some(Duration::from_secs(6)));
                crate::apps::start_indexing(); // que el lanzador y el dock vean la novedad
            }
            Outcome::UpToDate => {
                let msg = if job.kind == Kind::Install { "Ya estaba instalado" } else { "No había nada que actualizar" };
                toast::upsert(id, '\u{E73E}', toast::BLUE, &job.name, msg, Some(Duration::from_secs(5)));
            }
            Outcome::Cancelled => {
                toast::upsert(id, '\u{E711}', toast::RED, &format!("{doing}: cancelado"), "No se concedieron permisos de administrador", Some(Duration::from_secs(5)));
            }
            Outcome::Failed(why) => {
                toast::upsert(id, '\u{E783}', toast::RED, &format!("Falló: {}", doing.to_lowercase()), &why, Some(Duration::from_secs(12)));
            }
        }
    });
}

fn run(job: &Job) -> Outcome {
    let args = args_for(job);
    match job.manager {
        Manager::Winget => match packages::winget_path() {
            Some(exe) => run_hidden(job.manager, &exe, &args),
            None => Outcome::Failed("winget no está disponible".into()),
        },
        Manager::Choco => match packages::choco_path() {
            Some(exe) => run_elevated(&exe, &args),
            None => Outcome::Failed("Chocolatey no está instalado".into()),
        },
    }
}

/// Ejecuta sin ventana y con la salida capturada; devuelve el resultado según el código de salida.
fn run_hidden(manager: Manager, exe: &str, args: &[String]) -> Outcome {
    let (code, out) = match run_captured(exe, args) {
        Some(r) => r,
        None => return Outcome::Failed("no se pudo ejecutar el programa".into()),
    };
    match classify(manager, code) {
        Outcome::Failed(c) => Outcome::Failed(last_message(&out).unwrap_or(c)),
        other => other,
    }
}

/// Ejecuta `exe` sin ventana y devuelve (código de salida, salida). Bloquea hasta el límite de tiempo.
pub fn run_captured(exe: &str, args: &[String]) -> Option<(i32, String)> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let mut err = child.stderr.take()?;
    let r_out = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let r_err = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });
    let start = Instant::now();
    let code = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(-1),
            Ok(None) if start.elapsed() > LIMIT => {
                let _ = child.kill();
                break -1;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break -1,
        }
    };
    let mut text = String::from_utf8_lossy(&r_out.join().ok()?).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&r_err.join().ok()?));
    Some((code, text))
}

/// Como administrador y sin ventana (`runas`): Windows pide confirmación con UAC.
pub fn run_elevated(exe: &str, args: &[String]) -> Outcome {
    let file = HSTRING::from(exe);
    let params = HSTRING::from(args.join(" "));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        if let Err(e) = ShellExecuteExW(&mut info) {
            // ERROR_CANCELLED: el usuario rechazó el aviso de UAC
            return if e.code().0 as u32 & 0xFFFF == 1223 { Outcome::Cancelled } else { Outcome::Failed(e.message()) };
        }
        let process = info.hProcess;
        WaitForSingleObject(process, LIMIT.as_millis() as u32);
        let mut code = 0u32;
        let ok = GetExitCodeProcess(process, &mut code).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return Outcome::Failed("no se pudo leer el resultado".into());
        }
        classify(Manager::Choco, code as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(manager: Manager, kind: Kind, id: &str) -> Job {
        Job { manager, kind, id: id.into(), name: id.into() }
    }

    #[test]
    fn winget_arguments_are_silent_and_exact() {
        let a = args_for(&job(Manager::Winget, Kind::Install, "Git.Git"));
        assert_eq!(&a[..4], ["install", "--id", "Git.Git", "-e"]);
        assert!(a.contains(&"--silent".to_string()) && a.contains(&"--disable-interactivity".to_string()));
        assert!(args_for(&job(Manager::Winget, Kind::UpgradeAll, "")).starts_with(&["upgrade".into(), "--all".into()]));
    }

    #[test]
    fn choco_arguments_are_non_interactive() {
        assert_eq!(args_for(&job(Manager::Choco, Kind::Install, "git")), ["install", "git", "-y", "--no-progress"]);
        assert_eq!(args_for(&job(Manager::Choco, Kind::UpgradeAll, "")), ["upgrade", "all", "-y", "--no-progress"]);
    }

    #[test]
    fn ids_are_sanitized_before_reaching_the_command() {
        let a = args_for(&job(Manager::Choco, Kind::Install, "git; calc & x"));
        assert_eq!(a[1], "git calc  x");
        assert!(!a.iter().any(|s| s.contains(';') || s.contains('&')));
    }

    #[test]
    fn exit_codes_are_classified() {
        assert_eq!(classify(Manager::Winget, 0), Outcome::Done);
        assert_eq!(classify(Manager::Choco, 3010), Outcome::Done);
        assert_eq!(classify(Manager::Winget, 0x8A15_002Bu32 as i32), Outcome::UpToDate);
        assert_eq!(classify(Manager::Winget, 0x8A15_0061u32 as i32), Outcome::UpToDate);
        assert_eq!(classify(Manager::Winget, -1978335212), Outcome::Failed("código 0x8A150014".into()));
        assert!(matches!(classify(Manager::Choco, 1), Outcome::Failed(_)));
        // 3010 solo es válido en Chocolatey
        assert!(matches!(classify(Manager::Winget, 3010), Outcome::Failed(_)));
    }

    #[test]
    fn last_message_skips_progress_noise() {
        let out = "  ██████▒▒▒  45%\r\n\x08\x08-\r\nNo se encontró un instalador aplicable\n   \n";
        assert_eq!(last_message(out).as_deref(), Some("No se encontró un instalador aplicable"));
        assert_eq!(last_message("  - \\ | /\n"), None);
        let long = "x".repeat(200);
        assert!(last_message(&long).unwrap().chars().count() <= 70);
    }
}
