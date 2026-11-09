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

/// Literal TOML para una cadena.
pub fn lit_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Literal TOML para una lista de cadenas en una sola línea.
pub fn lit_array(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|s| lit_str(s)).collect::<Vec<_>>().join(", "))
}

/// Pone `key = literal` dentro de `[section]` conservando el resto del texto (comentarios incluidos).
/// Si la clave no existe se añade bajo la cabecera; si la sección no existe, al final.
pub fn set_toml_value(text: &str, section: &str, key: &str, literal: &str) -> Result<String, String> {
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let header = format!("[{section}]");
    let Some(start) = lines.iter().position(|l| l.trim() == header) else {
        let mut out = text.trim_end().to_string();
        out.push_str(&format!("{nl}{nl}{header}{nl}{key} = {literal}{nl}"));
        return Ok(out);
    };
    let end = lines.iter().enumerate().skip(start + 1).find(|(_, l)| l.trim_start().starts_with('[')).map_or(lines.len(), |(i, _)| i);
    let found = (start + 1..end).find(|&i| {
        let t = lines[i].trim_start();
        t.strip_prefix(key).is_some_and(|r| r.starts_with([' ', '\t', '=']) && r.trim_start().starts_with('='))
    });
    match found {
        Some(i) => {
            let line = lines[i].clone();
            let trimmed = line.trim_start();
            let indent = &line[..line.len() - trimmed.len()];
            let after_key = trimmed[key.len()..].trim_start();
            let after_eq = after_key[1..].trim_start();
            // longitud del valor antiguo: cadena entre comillas, lista en una línea o palabra hasta espacio/#
            let value_len = if let Some(rest) = after_eq.strip_prefix('"') {
                let mut escaped = false;
                let mut n = None;
                for (j, c) in rest.char_indices() {
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        n = Some(j + 2);
                        break;
                    }
                }
                n.ok_or("cadena sin cerrar en config.toml")?
            } else if after_eq.starts_with('[') {
                after_eq.find(']').map(|j| j + 1).ok_or("lista de varias líneas: edítala a mano")?
            } else {
                after_eq.find(|c: char| c.is_whitespace() || c == '#').unwrap_or(after_eq.len())
            };
            lines[i] = format!("{indent}{key} = {literal}{}", &after_eq[value_len..]);
        }
        None => lines.insert(start + 1, format!("{key} = {literal}")),
    }
    let mut out = lines.join(nl);
    if text.ends_with('\n') {
        out.push_str(nl);
    }
    Ok(out)
}

/// Ruta de `config.toml`; si no había archivo, lo crea junto al ejecutable con los valores por defecto.
fn path_or_create() -> Result<std::path::PathBuf, String> {
    if let Some(p) = path() {
        return Ok(p);
    }
    let p = std::env::current_exe().map_err(|e| e.to_string())?.parent().ok_or("sin carpeta")?.join("config.toml");
    if !p.is_file() {
        std::fs::write(&p, DEFAULT_TOML).map_err(|e| e.to_string())?;
    }
    let _ = PATH.set(p.clone());
    Ok(p)
}

/// Guarda `key = literal` de `[section]` en `config.toml` (solo si el resultado sigue siendo válido).
pub fn save_value(section: &str, key: &str, literal: &str) -> Result<(), String> {
    let path = path_or_create()?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let updated = set_toml_value(&text, section, key, literal)?;
    Config::parse(&updated).map_err(|e| format!("el resultado no sería válido: {e}"))?;
    std::fs::write(&path, updated).map_err(|e| e.to_string())
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
    /// Alt+Tab propio de hypr-win en lugar del de Windows.
    pub alt_tab: bool,
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
            alt_tab: true,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Bar {
    /// Nombre de un tema (predefinido o del usuario) que sustituye a los colores de abajo; vacío = usar estos.
    pub theme: String,
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
    /// Pack de iconos de menús, avisos y widgets: "phosphor", "phosphor-bold", "phosphor-fill", "lucide" o vacío
    /// (los de Windows). Un tema con `theme = ...` trae el suyo.
    pub icons: String,
    /// Estilo de los iconos de las aplicaciones en el dock: "original", "tile", "mono" o "accent".
    pub app_icons: String,
    /// Estilo de la barra: "bar" (franja continua, por defecto) o "bubbles" (grupos en burbujas separadas).
    pub style: String,
    /// Cursor de flecha: "classic", "contrast", "accent", "soft" o vacío (los de Windows). Un tema lo define.
    pub cursor: String,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            theme: String::new(),
            height: 28,
            background: "#1e1e20".into(),
            foreground: "#f5f5f7".into(),
            dim: "#8e8e93".into(),
            accent: "#0a84ff".into(),
            accent_text: "#ffffff".into(),
            font: "Segoe UI Variable Text".into(),
            font_size: 10,
            opacity: 80,
            icons: String::new(),
            app_icons: String::new(),
            style: "bar".into(),
            cursor: String::new(),
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

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Wallpaper {
    /// Carpeta con tus fondos; vacío = `Imágenes\Wallpapers`.
    pub dir: String,
    /// Cambiar de fondo automáticamente cada N minutos (0 = no).
    pub interval_minutes: u32,
    /// fill | fit | stretch | tile | center | span
    pub fit: String,
    /// Al cambiar de tema, generar y aplicar el fondo del tema.
    pub follow_theme: bool,
    /// Aplicar también la imagen a la pantalla de bloqueo (Win+L).
    pub lock_screen: bool,
}

impl Default for Wallpaper {
    fn default() -> Self {
        Self { dir: String::new(), interval_minutes: 0, fit: "fill".into(), follow_theme: false, lock_screen: true }
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
    #[serde(default)]
    pub wallpaper: Wallpaper,
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
        let mut cfg: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        if !cfg.bar.theme.trim().is_empty() {
            match crate::theme::find(&cfg.bar.theme) {
                Some(t) => crate::theme::apply_to_bar(&t, &mut cfg.bar),
                None => eprintln!("aviso: no se encontró el tema «{}»; se usan los colores de [bar]", cfg.bar.theme),
            }
        }
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
        let wp = &self.wallpaper;
        if crate::wallpaper::Fit::parse(&wp.fit).is_none() {
            return Err("wallpaper.fit debe ser fill, fit, stretch, tile, center o span".into());
        }
        if wp.interval_minutes > 1440 {
            return Err("wallpaper.interval_minutes debe estar entre 0 y 1440".into());
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
