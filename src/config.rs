//! Carga y validación de `config.toml`.

use serde::Deserialize;

use crate::keybinds::{self, Action, Binding};
use crate::layout::LayoutKind;

const DEFAULT_TOML: &str = include_str!("../config.toml");

static PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Recuerda el archivo de configuración cargado (para poder editarlo desde el menú).
pub fn set_path(p: std::path::PathBuf) {
    let _ = PATH.set(p);
}

/// Archivo de configuración en uso, si se cargó uno.
pub fn path() -> Option<std::path::PathBuf> {
    PATH.get().cloned()
}

/// Sustituye el atajo `old` por `new` en el texto de `config.toml` (ambos tal como aparecen entre comillas).
pub fn replace_bind(text: &str, old: &str, new: &str) -> Result<String, String> {
    let needle = format!("\"{old}\"");
    match text.matches(&needle).count() {
        1 => Ok(text.replacen(&needle, &format!("\"{new}\""), 1)),
        0 => Err("no se encontró el atajo en config.toml (edítalo a mano)".into()),
        _ => Err("el atajo aparece varias veces en config.toml (edítalo a mano)".into()),
    }
}

/// Aplica el cambio de un atajo en el archivo de configuración. Si no había archivo, crea
/// `config.toml` junto al ejecutable a partir de los valores por defecto.
pub fn save_bind_change(old: &str, new: &str) -> Result<(), String> {
    let path = match path() {
        Some(p) => p,
        None => {
            let p = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("sin carpeta")?.join("config.toml");
            if !p.is_file() {
                std::fs::write(&p, DEFAULT_TOML).map_err(|e| e.to_string())?;
            }
            let _ = PATH.set(p.clone());
            p
        }
    };
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated = replace_bind(&text, old, new)?;
    // no se guarda nada que no se pueda volver a cargar
    Config::parse(&updated).map_err(|e| format!("el resultado no sería válido: {e}"))?;
    std::fs::write(&path, updated).map_err(|e| e.to_string())
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct General {
    pub layout: String,
    pub workspaces: usize,
    pub gaps_in: i32,
    pub gaps_out: i32,
    pub master_ratio: f32,
    /// Duración de las animaciones en ms (0 = sin animaciones).
    pub animation_ms: u32,
    /// El foco sigue al ratón (sin hacer clic).
    pub focus_follows_mouse: bool,
    /// Desactiva (solo mientras corre hypr-win) la animación de minimizar/maximizar de Windows.
    pub disable_windows_animations: bool,
    /// Oculta la barra de tareas de Windows (ShowWindow sobre `Shell_TrayWnd`) mientras corre.
    pub hide_taskbar: bool,
    /// Traga la tecla Win sola para que no abra el menú Inicio.
    pub block_start_menu: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            layout: "dwindle".into(),
            workspaces: 9,
            gaps_in: 8,
            gaps_out: 8,
            master_ratio: 0.55,
            animation_ms: 180,
            focus_follows_mouse: true,
            disable_windows_animations: false,
            hide_taskbar: false,
            block_start_menu: false,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Bar {
    pub height: i32,
    pub background: String,
    pub foreground: String,
    pub dim: String,
    pub accent: String,
    pub accent_text: String,
    pub font: String,
    pub font_size: i32,
    /// Opacidad de la barra (20-100 %); el lanzador usa una fija.
    pub opacity: u8,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            height: 28,
            background: "#1e1e20".into(),
            foreground: "#f5f5f7".into(),
            dim: "#8e8e93".into(),
            accent: "#0a84ff".into(),
            accent_text: "#ffffff".into(),
            font: "Segoe UI Variable Text".into(),
            font_size: 10,
            opacity: 80,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Dock {
    pub enabled: bool,
    /// Tamaño de los iconos en px a 96 DPI.
    pub icon_size: i32,
    /// Escala máxima del icono bajo el cursor (1.0 = sin magnificación).
    pub magnification: f32,
    /// Apps fijadas, por nombre (búsqueda difusa en la lista de aplicaciones).
    pub pinned: Vec<String>,
    /// Mostrar también las apps abiertas que no están fijadas.
    pub show_running: bool,
    /// Se oculta cuando hay ventanas en el monitor y aparece al dejar el ratón en el borde inferior.
    pub autohide: bool,
}

impl Default for Dock {
    fn default() -> Self {
        Self {
            enabled: true,
            icon_size: 36,
            magnification: 1.5,
            pinned: ["Explorador", "Edge", "Terminal", "Configuración", "Calculadora"].map(String::from).to_vec(),
            show_running: true,
            autohide: true,
        }
    }
}

#[derive(Deserialize)]
struct OnlyBinds {
    binds: Vec<String>,
}

fn default_binds() -> Vec<String> {
    toml::from_str::<OnlyBinds>(DEFAULT_TOML).map(|b| b.binds).unwrap_or_default()
}

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub bar: Bar,
    #[serde(default)]
    pub dock: Dock,
    #[serde(default = "default_binds")]
    pub binds: Vec<String>,
}

/// "#rrggbb" -> valor COLORREF (0x00BBGGRR).
pub fn parse_color(s: &str) -> Result<u32, String> {
    let hex = s.trim().strip_prefix('#').unwrap_or(s.trim());
    if hex.len() != 6 {
        return Err(format!("color inválido: '{s}' (usa #rrggbb)"));
    }
    let v = u32::from_str_radix(hex, 16).map_err(|_| format!("color inválido: '{s}'"))?;
    let (r, g, b) = ((v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff);
    Ok(r | (g << 8) | (b << 16))
}

impl Config {
    pub fn default_config() -> Config {
        Self::parse(DEFAULT_TOML).expect("el config.toml embebido debe ser válido")
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn layout_kind(&self) -> LayoutKind {
        LayoutKind::parse(&self.general.layout).unwrap_or(LayoutKind::Dwindle)
    }

    pub fn validate(&self) -> Result<(), String> {
        let g = &self.general;
        LayoutKind::parse(&g.layout)?;
        if !(1..=9).contains(&g.workspaces) {
            return Err("general.workspaces debe estar entre 1 y 9".into());
        }
        if !(0..=200).contains(&g.gaps_in) || !(0..=200).contains(&g.gaps_out) {
            return Err("los gaps deben estar entre 0 y 200".into());
        }
        if !(0.1..=0.9).contains(&g.master_ratio) {
            return Err("general.master_ratio debe estar entre 0.1 y 0.9".into());
        }
        if g.animation_ms > 1000 {
            return Err("general.animation_ms debe estar entre 0 y 1000".into());
        }
        let d = &self.dock;
        if !(24..=96).contains(&d.icon_size) {
            return Err("dock.icon_size debe estar entre 24 y 96".into());
        }
        if !(1.0..=2.5).contains(&d.magnification) {
            return Err("dock.magnification debe estar entre 1.0 y 2.5".into());
        }
        let b = &self.bar;
        if !(16..=200).contains(&b.height) {
            return Err("bar.height debe estar entre 16 y 200".into());
        }
        if !(20..=100).contains(&b.opacity) {
            return Err("bar.opacity debe estar entre 20 y 100".into());
        }
        if !(6..=48).contains(&b.font_size) {
            return Err("bar.font_size debe estar entre 6 y 48".into());
        }
        for c in [&b.background, &b.foreground, &b.dim, &b.accent, &b.accent_text] {
            parse_color(c)?;
        }
        self.bindings().map(|_| ())
    }

    /// Atajos ya parseados; comprueba que los workspaces referenciados existan.
    pub fn bindings(&self) -> Result<Vec<Binding>, String> {
        let mut out = Vec::with_capacity(self.binds.len());
        for s in &self.binds {
            let b = keybinds::parse_binding(s)?;
            if let Action::Workspace(n) | Action::MoveToWorkspace(n) = b.action {
                if n >= self.general.workspaces {
                    return Err(format!("'{s}' apunta a un workspace inexistente"));
                }
            }
            out.push(b);
        }
        Ok(out)
    }
}
