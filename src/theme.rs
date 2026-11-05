//! Temas: colores, fuente y translucidez de toda la interfaz. Hay temas predefinidos y los del usuario se
//! guardan como archivos TOML en `%APPDATA%\hypr-win\themes`. Lógica pura salvo la lectura/escritura de
//! archivos.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{parse_color, Bar};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeDef {
    pub name: String,
    pub background: String,
    pub foreground: String,
    /// Textos secundarios y elementos apagados.
    pub dim: String,
    pub accent: String,
    /// Texto sobre el color de acento.
    pub accent_text: String,
    /// Familia de fuente; vacío = la de la configuración.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Opacidad de la barra (20-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<u8>,
    /// Pack de iconos (ver `iconset`); vacío o ausente = los de Windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<String>,
    /// Estilo de los iconos de las aplicaciones del dock: original, tile, mono o accent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_icons: Option<String>,
    /// Fondo de escritorio asociado: "gradient", "solid" o la ruta de una imagen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
}

fn def(name: &str, bg: &str, fg: &str, dim: &str, accent: &str, accent_text: &str) -> ThemeDef {
    ThemeDef {
        name: name.into(),
        background: bg.into(),
        foreground: fg.into(),
        dim: dim.into(),
        accent: accent.into(),
        accent_text: accent_text.into(),
        font: None,
        opacity: None,
        icons: None,
        app_icons: None,
        wallpaper: Some("gradient".into()),
    }
}

/// Temas incluidos.
pub fn builtins() -> Vec<ThemeDef> {
    // (tema, pack de iconos): trazo fino para los sobrios, relleno para los más vivos
    let themed = |t: ThemeDef, icons: &str| ThemeDef { icons: Some(icons.into()), ..t };
    let styled = |t: ThemeDef, app_icons: &str| ThemeDef { app_icons: Some(app_icons.into()), ..t };
    vec![
        themed(def("Apple Oscuro", "#1e1e20", "#f5f5f7", "#8e8e93", "#0a84ff", "#ffffff"), "phosphor"),
        themed(def("Apple Claro", "#f2f2f7", "#1d1d1f", "#86868b", "#007aff", "#ffffff"), "fluent"),
        styled(themed(def("Catppuccin Mocha", "#1e1e2e", "#cdd6f4", "#7f849c", "#89b4fa", "#1e1e2e"), "phosphor-fill"), "tile"),
        styled(themed(def("Catppuccin Latte", "#eff1f5", "#4c4f69", "#8c8fa1", "#1e66f5", "#ffffff"), "phosphor-fill"), "tile"),
        styled(themed(def("Nord", "#2e3440", "#eceff4", "#7b88a1", "#88c0d0", "#2e3440"), "lucide"), "mono"),
        styled(themed(def("Dracula", "#282a36", "#f8f8f2", "#6272a4", "#bd93f9", "#282a36"), "phosphor-bold"), "tile"),
        styled(themed(def("Gruvbox Oscuro", "#282828", "#ebdbb2", "#928374", "#fabd2f", "#282828"), "phosphor-bold"), "mono"),
        themed(def("Tokyo Night", "#1a1b26", "#c0caf5", "#565f89", "#7aa2f7", "#1a1b26"), "tabler"),
        styled(themed(def("Rosé Pine", "#191724", "#e0def4", "#6e6a86", "#c4a7e7", "#191724"), "fluent-fill"), "tile"),
        def("Solarized Oscuro", "#002b36", "#93a1a1", "#586e75", "#268bd2", "#fdf6e3"),
        styled(themed(def("Everforest", "#2d353b", "#d3c6aa", "#859289", "#a7c080", "#2d353b"), "lucide"), "tile"),
        styled(themed(def("Grafito", "#111111", "#eeeeee", "#888888", "#ffffff", "#000000"), "phosphor-bold"), "mono"),
    ]
}

/// Carpeta de los temas del usuario.
pub fn user_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join(r"hypr-win\themes"))
}

/// Nombre apto para un archivo de Windows.
pub fn file_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    s.trim().trim_end_matches('.').to_string()
}

/// Temas del usuario (los archivos `.toml` de la carpeta; los que no se pueden leer se ignoran).
pub fn user_themes() -> Vec<ThemeDef> {
    let Some(dir) = user_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<ThemeDef> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
        .filter_map(|p| toml::from_str::<ThemeDef>(&std::fs::read_to_string(p).ok()?).ok())
        .filter(|t| validate(t).is_ok())
        .collect();
    v.sort_by_key(|t| t.name.to_lowercase());
    v
}

/// Todos los temas: los predefinidos y los del usuario (uno del usuario con el mismo nombre sustituye al predefinido).
pub fn all() -> Vec<ThemeDef> {
    let user = user_themes();
    let mut out: Vec<ThemeDef> = builtins().into_iter().filter(|b| !user.iter().any(|u| u.name.eq_ignore_ascii_case(&b.name))).collect();
    out.extend(user);
    out
}

pub fn find(name: &str) -> Option<ThemeDef> {
    all().into_iter().find(|t| t.name.eq_ignore_ascii_case(name.trim()))
}

pub fn is_builtin(name: &str) -> bool {
    builtins().iter().any(|b| b.name.eq_ignore_ascii_case(name))
}

pub fn validate(t: &ThemeDef) -> Result<(), String> {
    if t.name.trim().is_empty() || file_name(&t.name).is_empty() {
        return Err("el tema necesita un nombre".into());
    }
    for c in [&t.background, &t.foreground, &t.dim, &t.accent, &t.accent_text] {
        parse_color(c)?;
    }
    if t.opacity.is_some_and(|o| !(20..=100).contains(&o)) {
        return Err("la opacidad debe estar entre 20 y 100".into());
    }
    if let Some(i) = t.icons.as_deref().filter(|i| !i.is_empty()) {
        if crate::iconset::find(i).is_none() {
            return Err(format!("pack de iconos desconocido: {i}"));
        }
    }
    if let Some(s) = t.app_icons.as_deref().filter(|s| !s.is_empty()) {
        if !crate::icons::APP_STYLES.iter().any(|(k, _, _)| k.eq_ignore_ascii_case(s)) {
            return Err(format!("estilo de iconos de aplicación desconocido: {s}"));
        }
    }
    Ok(())
}

/// Guarda un tema del usuario. Devuelve la ruta del archivo.
pub fn save_user(t: &ThemeDef) -> Result<PathBuf, String> {
    validate(t)?;
    let dir = user_dir().ok_or("no se encontró la carpeta de datos")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.toml", file_name(&t.name)));
    let text = toml::to_string_pretty(t).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn delete_user(name: &str) {
    if let Some(dir) = user_dir() {
        let _ = std::fs::remove_file(dir.join(format!("{}.toml", file_name(name))));
    }
}

/// Aplica el tema a la configuración de la barra (los valores que el tema no define se conservan).
pub fn apply_to_bar(t: &ThemeDef, bar: &mut Bar) {
    bar.background = t.background.clone();
    bar.foreground = t.foreground.clone();
    bar.dim = t.dim.clone();
    bar.accent = t.accent.clone();
    bar.accent_text = t.accent_text.clone();
    if let Some(f) = t.font.as_ref().filter(|f| !f.trim().is_empty()) {
        bar.font = f.clone();
    }
    if let Some(o) = t.opacity {
        bar.opacity = o;
    }
    // los iconos son parte del tema: uno sin pack vuelve a los de Windows
    bar.icons = t.icons.clone().unwrap_or_default();
    bar.app_icons = t.app_icons.clone().unwrap_or_default();
}

/// Tema equivalente a la configuración actual de la barra.
pub fn from_bar(name: &str, bar: &Bar) -> ThemeDef {
    ThemeDef {
        name: name.into(),
        background: bar.background.clone(),
        foreground: bar.foreground.clone(),
        dim: bar.dim.clone(),
        accent: bar.accent.clone(),
        accent_text: bar.accent_text.clone(),
        font: Some(bar.font.clone()),
        opacity: Some(bar.opacity),
        icons: Some(bar.icons.clone()),
        app_icons: Some(bar.app_icons.clone()),
        wallpaper: Some("gradient".into()),
    }
}

/// "abc", "#ABC", "aabbcc" -> "#aabbcc"; `None` si no es un color hexadecimal.
pub fn normalize_hex(input: &str) -> Option<String> {
    let s = input.trim().trim_start_matches('#');
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match s.len() {
        3 => Some(format!("#{}", s.chars().flat_map(|c| [c, c]).collect::<String>().to_lowercase())),
        6 => Some(format!("#{}", s.to_lowercase())),
        _ => None,
    }
}

fn luminance(colorref: u32) -> f32 {
    let ch = |v: u32| {
        let c = (v & 0xff) as f32 / 255.0;
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(colorref) + 0.7152 * ch(colorref >> 8) + 0.0722 * ch(colorref >> 16)
}

/// Relación de contraste WCAG entre dos colores (COLORREF): 1 (igual) a 21 (negro sobre blanco).
pub fn contrast_ratio(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Texto legible sobre `accent`: blanco si llega a 3:1 (mínimo WCAG para elementos de interfaz y texto
/// grande, lo que usan los azules de Apple) y, si no, negro.
pub fn readable_on(accent: &str) -> String {
    match parse_color(accent) {
        Ok(c) if contrast_ratio(c, 0xFFFFFF) >= 3.0 => "#ffffff".into(),
        Ok(_) => "#000000".into(),
        Err(_) => "#ffffff".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_valid_unique_and_readable() {
        let all = builtins();
        assert!(all.len() >= 10);
        let mut names: Vec<String> = all.iter().map(|t| t.name.to_lowercase()).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n);
        for t in &all {
            validate(t).unwrap_or_else(|e| panic!("{}: {e}", t.name));
            // el texto principal debe leerse bien sobre el fondo, y el del acento sobre el acento
            let (bg, fg) = (parse_color(&t.background).unwrap(), parse_color(&t.foreground).unwrap());
            assert!(contrast_ratio(bg, fg) >= 4.5, "{}: poco contraste texto/fondo", t.name);
            let (ac, at) = (parse_color(&t.accent).unwrap(), parse_color(&t.accent_text).unwrap());
            assert!(contrast_ratio(ac, at) >= 3.0, "{}: poco contraste acento", t.name);
        }
    }

    #[test]
    fn theme_files_round_trip_through_toml() {
        let mut t = def("Mío", "#101010", "#fafafa", "#777777", "#ff8800", "#000000");
        t.font = Some("Consolas".into());
        t.opacity = Some(70);
        let text = toml::to_string_pretty(&t).unwrap();
        assert!(text.contains("name = \"Mío\"") && text.contains("opacity = 70"));
        assert_eq!(toml::from_str::<ThemeDef>(&text).unwrap(), t);
        // opcionales ausentes
        let min = "name='x'\nbackground='#000000'\nforeground='#ffffff'\ndim='#888888'\naccent='#0a84ff'\naccent_text='#ffffff'\n";
        let m: ThemeDef = toml::from_str(min).unwrap();
        assert!(m.font.is_none() && m.opacity.is_none() && m.wallpaper.is_none());
    }

    #[test]
    fn validation_rejects_bad_themes() {
        let mut t = builtins().remove(0);
        t.accent = "azul".into();
        assert!(validate(&t).is_err());
        let mut t = builtins().remove(0);
        t.opacity = Some(5);
        assert!(validate(&t).is_err());
        let mut t = builtins().remove(0);
        t.name = " ".into();
        assert!(validate(&t).is_err());
    }

    #[test]
    fn applying_a_theme_overrides_only_what_it_defines() {
        let mut bar = Bar::default();
        let font = bar.font.clone();
        let nord = builtins().into_iter().find(|t| t.name == "Nord").unwrap();
        apply_to_bar(&nord, &mut bar);
        assert_eq!(bar.background, "#2e3440");
        assert_eq!(bar.font, font); // el tema no define fuente
        let mut with_font = nord.clone();
        with_font.font = Some("Consolas".into());
        with_font.opacity = Some(55);
        apply_to_bar(&with_font, &mut bar);
        assert_eq!((bar.font.as_str(), bar.opacity), ("Consolas", 55));
    }

    #[test]
    fn icon_packs_belong_to_the_theme() {
        // los temas incluidos que traen pack usan uno que existe
        for t in builtins() {
            if let Some(i) = t.icons.as_deref() {
                assert!(crate::iconset::find(i).is_some(), "{}: pack {i} desconocido", t.name);
            }
        }
        let mut bar = Bar::default();
        let nord = builtins().into_iter().find(|t| t.name == "Nord").unwrap();
        apply_to_bar(&nord, &mut bar);
        assert_eq!(bar.icons, "lucide");
        // un tema sin pack devuelve los iconos de Windows
        let solarized = builtins().into_iter().find(|t| t.name == "Solarized Oscuro").unwrap();
        apply_to_bar(&solarized, &mut bar);
        assert_eq!(bar.icons, "");
        // viaja en el archivo del tema y se valida
        let mut t = nord;
        t.icons = Some("phosphor-fill".into());
        let back: ThemeDef = toml::from_str(&toml::to_string_pretty(&t).unwrap()).unwrap();
        assert_eq!(back.icons.as_deref(), Some("phosphor-fill"));
        assert!(validate(&back).is_ok());
        t.icons = Some("no-existe".into());
        assert!(validate(&t).is_err());
        t.icons = Some(String::new());
        assert!(validate(&t).is_ok());
        // un archivo antiguo, sin el campo, sigue leyéndose
        // estilo de iconos de aplicación
        let mut s = builtins().remove(0);
        s.app_icons = Some("tile".into());
        assert!(validate(&s).is_ok());
        s.app_icons = Some("raro".into());
        assert!(validate(&s).is_err());
        s.app_icons = None;
        let mut bar = Bar::default();
        let mocha = builtins().into_iter().find(|t| t.name == "Catppuccin Mocha").unwrap();
        apply_to_bar(&mocha, &mut bar);
        assert_eq!(bar.app_icons, "tile");
        apply_to_bar(&s, &mut bar);
        assert_eq!(bar.app_icons, "");
        assert!(toml::from_str::<ThemeDef>("name='x'\nbackground='#000000'\nforeground='#ffffff'\ndim='#888888'\naccent='#0a84ff'\naccent_text='#ffffff'\n").unwrap().icons.is_none());
    }

    #[test]
    fn hex_colors_are_normalized() {
        assert_eq!(normalize_hex("#ABC").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex("1E1E2E").as_deref(), Some("#1e1e2e"));
        assert_eq!(normalize_hex("  #0a84ff ").as_deref(), Some("#0a84ff"));
        assert_eq!(normalize_hex("#12345"), None);
        assert_eq!(normalize_hex("rojo"), None);
    }

    #[test]
    fn readable_text_color_is_chosen_by_contrast() {
        assert_eq!(readable_on("#ffffff"), "#000000");
        assert_eq!(readable_on("#000000"), "#ffffff");
        assert_eq!(readable_on("#0a84ff"), "#ffffff");
        assert_eq!(readable_on("#fabd2f"), "#000000");
        assert!((contrast_ratio(0xFFFFFF, 0x000000) - 21.0).abs() < 0.01);
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("Mi: tema/1"), "Mi tema1");
        assert_eq!(file_name("***"), "");
    }
}
