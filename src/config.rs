//! Carga y validación de `config.toml`.

use serde::Deserialize;

use crate::keybinds::{self, Action, Binding};
use crate::layout::LayoutKind;

const DEFAULT_TOML: &str = include_str!("../config.toml");

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct General {
    pub layout: String,
    pub workspaces: usize,
    pub gaps_in: i32,
    pub gaps_out: i32,
    pub master_ratio: f32,
}

impl Default for General {
    fn default() -> Self {
        Self {
            layout: "dwindle".into(),
            workspaces: 9,
            gaps_in: 8,
            gaps_out: 8,
            master_ratio: 0.55,
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
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            height: 32,
            background: "#1e1e2e".into(),
            foreground: "#cdd6f4".into(),
            dim: "#6c7086".into(),
            accent: "#89b4fa".into(),
            accent_text: "#1e1e2e".into(),
            font: "Segoe UI".into(),
            font_size: 10,
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
        let b = &self.bar;
        if !(16..=200).contains(&b.height) {
            return Err("bar.height debe estar entre 16 y 200".into());
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
