//! Instalación de aplicaciones desde una consola, con `winget` o, si está instalado, Chocolatey.

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

pub struct Pkg {
    pub name: &'static str,
    /// Identificador en winget.
    pub id: &'static str,
    /// Identificador en Chocolatey.
    pub choco: &'static str,
    /// Dominio del fabricante, para descargar su favicon como icono.
    pub site: &'static str,
    pub section: &'static str,
}

const fn p(section: &'static str, name: &'static str, id: &'static str, choco: &'static str, site: &'static str) -> Pkg {
    Pkg { name, id, choco, site, section }
}

/// Selección de programas habituales.
pub const CURATED: &[Pkg] = &[
    p("Navegadores", "Google Chrome", "Google.Chrome", "googlechrome", "chrome.com"),
    p("Navegadores", "Firefox", "Mozilla.Firefox", "firefox", "firefox.com"),
    p("Navegadores", "Brave", "Brave.Brave", "brave", "brave.com"),
    p("Desarrollo", "Visual Studio Code", "Microsoft.VisualStudioCode", "vscode", "code.visualstudio.com"),
    p("Desarrollo", "Git", "Git.Git", "git", "git-scm.com"),
    p("Desarrollo", "Node.js (LTS)", "OpenJS.NodeJS.LTS", "nodejs-lts", "nodejs.org"),
    p("Desarrollo", "Python 3", "Python.Python.3.12", "python", "python.org"),
    p("Desarrollo", "Windows Terminal", "Microsoft.WindowsTerminal", "microsoft-windows-terminal", "microsoft.com"),
    p("Desarrollo", "Docker Desktop", "Docker.DockerDesktop", "docker-desktop", "docker.com"),
    p("Comunicación", "Discord", "Discord.Discord", "discord", "discord.com"),
    p("Comunicación", "Telegram", "Telegram.TelegramDesktop", "telegram", "telegram.org"),
    p("Comunicación", "Zoom", "Zoom.Zoom", "zoom", "zoom.us"),
    p("Comunicación", "Slack", "SlackTechnologies.Slack", "slack", "slack.com"),
    p("Multimedia", "Spotify", "Spotify.Spotify", "spotify", "spotify.com"),
    p("Multimedia", "VLC", "VideoLAN.VLC", "vlc", "videolan.org"),
    p("Multimedia", "OBS Studio", "OBSProject.OBSStudio", "obs-studio", "obsproject.com"),
    p("Utilidades", "7-Zip", "7zip.7zip", "7zip", "7-zip.org"),
    p("Utilidades", "PowerToys", "Microsoft.PowerToys", "powertoys", "microsoft.com"),
    p("Utilidades", "Notepad++", "Notepad++.Notepad++", "notepadplusplus", "notepad-plus-plus.org"),
    p("Utilidades", "Obsidian", "Obsidian.Obsidian", "obsidian", "obsidian.md"),
    p("Utilidades", "Everything", "voidtools.Everything", "everything", "voidtools.com"),
    p("Utilidades", "ShareX", "ShareX.ShareX", "sharex", "getsharex.com"),
    p("Juegos", "Steam", "Valve.Steam", "steam", "steampowered.com"),
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Manager {
    Winget,
    Choco,
}

/// Deja solo caracteres seguros para meter en una línea de comandos (el texto lo escribe el usuario).
pub fn sanitize_arg(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '_' | '+' | '#')).collect::<String>().trim().to_string()
}

const WINGET_FLAGS: &str = "--accept-package-agreements --accept-source-agreements";

pub fn install_id_command(id: &str) -> String {
    format!("winget install --id {} -e {WINGET_FLAGS}", sanitize_arg(id))
}

/// Instalar por nombre o parte del id (winget lo resuelve; si es ambiguo, lo explica en la consola).
pub fn install_query_command(query: &str) -> String {
    format!("winget install \"{}\" {WINGET_FLAGS}", sanitize_arg(query))
}

pub fn search_command(query: &str) -> String {
    format!("winget search \"{}\"", sanitize_arg(query))
}

pub fn upgrade_all_command() -> String {
    format!("winget upgrade --all {WINGET_FLAGS}")
}

pub fn choco_install_command(id: &str) -> String {
    format!("choco install {} -y", sanitize_arg(id))
}

pub fn choco_install_query_command(query: &str) -> String {
    format!("choco install \"{}\" -y", sanitize_arg(query))
}

pub fn choco_search_command(query: &str) -> String {
    format!("choco search \"{}\"", sanitize_arg(query))
}

pub fn choco_upgrade_all_command() -> String {
    "choco upgrade all -y".into()
}

/// Ruta de `winget.exe` (en el PATH o en WindowsApps).
pub fn winget_path() -> Option<String> {
    which("winget.exe").or_else(|| {
        let p = std::path::Path::new(&std::env::var_os("LOCALAPPDATA")?).join(r"Microsoft\WindowsApps\winget.exe");
        p.is_file().then(|| p.to_string_lossy().into_owned())
    })
}

/// Ruta de `choco.exe` (en el PATH o en la carpeta de Chocolatey).
pub fn choco_path() -> Option<String> {
    which("choco.exe").or_else(|| {
        let p = std::path::Path::new(&std::env::var_os("ProgramData")?).join(r"chocolatey\bin\choco.exe");
        p.is_file().then(|| p.to_string_lossy().into_owned())
    })
}

fn which(exe: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(exe)).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

/// ¿Está `winget` disponible? (Instalador de aplicaciones de Microsoft.)
pub fn winget_available() -> bool {
    winget_path().is_some()
}

/// ¿Está Chocolatey instalado?
pub fn choco_available() -> bool {
    choco_path().is_some()
}

/// Gestor que se usa para la lista de programas: winget si está; si no, Chocolatey.
pub fn preferred() -> Option<Manager> {
    if winget_available() {
        Some(Manager::Winget)
    } else if choco_available() {
        Some(Manager::Choco)
    } else {
        None
    }
}

/// Ejecuta `command` en una consola nueva que se queda abierta al terminar (`cmd /k`).
pub fn run_in_console(title: &str, command: &str) {
    let title = sanitize_arg(title);
    let params = HSTRING::from(format!("/c start \"{title}\" cmd /k {command}"));
    unsafe {
        ShellExecuteW(None, w!("open"), w!("cmd.exe"), &params, PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Igual, pero como administrador (aparece el aviso de UAC): Chocolatey lo necesita para instalar.
pub fn run_in_console_elevated(command: &str) {
    let params = HSTRING::from(format!("/k {command}"));
    unsafe {
        ShellExecuteW(None, w!("runas"), w!("cmd.exe"), &params, PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Ejecuta un programa sin ventana (bloquear, suspender, apagar...).
pub fn run_hidden(file: &str, params: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(file), &HSTRING::from(params), PCWSTR::null(), SW_HIDE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_are_sanitized() {
        assert_eq!(sanitize_arg("vlc & calc | del *"), "vlc  calc  del");
        assert_eq!(sanitize_arg("\"; rm -rf /"), "rm -rf");
        assert_eq!(sanitize_arg("Notepad++"), "Notepad++");
    }

    #[test]
    fn commands_are_well_formed() {
        assert_eq!(
            install_id_command("Git.Git"),
            "winget install --id Git.Git -e --accept-package-agreements --accept-source-agreements"
        );
        assert!(install_query_command("vlc & calc").starts_with("winget install \"vlc  calc\""));
        assert_eq!(search_command("code"), "winget search \"code\"");
        assert!(upgrade_all_command().starts_with("winget upgrade --all"));
        assert_eq!(choco_install_command("googlechrome"), "choco install googlechrome -y");
        assert_eq!(choco_search_command("git & x"), "choco search \"git  x\"");
        assert_eq!(choco_upgrade_all_command(), "choco upgrade all -y");
        assert_eq!(choco_install_query_command("vlc; calc"), "choco install \"vlc calc\" -y");
    }

    #[test]
    fn curated_ids_are_unique_and_safe() {
        for field in [|p: &Pkg| p.id, |p: &Pkg| p.choco, |p: &Pkg| p.name] {
            let mut v: Vec<&str> = CURATED.iter().map(field).collect();
            v.sort_unstable();
            let n = v.len();
            v.dedup();
            assert_eq!(v.len(), n);
        }
        assert!(CURATED.iter().all(|p| sanitize_arg(p.id) == p.id && p.id.contains('.')));
        assert!(CURATED.iter().all(|p| sanitize_arg(p.choco) == p.choco && !p.choco.is_empty()));
        assert!(CURATED.iter().all(|p| p.site.contains('.') && !p.site.contains('/')));
    }
}
