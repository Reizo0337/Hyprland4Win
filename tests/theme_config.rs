//! Edición de config.toml conservando comentarios y resolución de temas.

use hypr_win::config::{lit_array, lit_str, set_toml_value, Config};
use hypr_win::theme;

const SAMPLE: &str = "# cabecera\nbinds = [\n    \"Super+Q, close\",\n]\n\n[general]\ngaps_in = 8            # px entre ventanas\ngaps_out = 8\n\n[bar]\n# Estilo\nheight = 28            # alto\nbackground = \"#1e1e20\"  # fondo\nfont = \"Segoe UI\"\n\n[dock]\nicon_size = 36\npinned = [\"A\", \"B\"]   # fijadas\n";

#[test]
fn replaces_values_and_keeps_comments_and_other_lines() {
    let out = set_toml_value(SAMPLE, "general", "gaps_in", "12").unwrap();
    assert!(out.contains("gaps_in = 12            # px entre ventanas"), "{out}");
    assert!(out.contains("gaps_out = 8\n"));
    assert_eq!(out.matches("height = 28").count(), 1);
    // un valor con # dentro de comillas no se confunde con un comentario
    let out = set_toml_value(SAMPLE, "bar", "background", &lit_str("#101010")).unwrap();
    assert!(out.contains("background = \"#101010\"  # fondo"), "{out}");
    // listas en una línea
    let out = set_toml_value(SAMPLE, "dock", "pinned", &lit_array(&["Edge".into(), "Terminal".into()])).unwrap();
    assert!(out.contains("pinned = [\"Edge\", \"Terminal\"]   # fijadas"), "{out}");
    assert!(out.ends_with('\n'));
}

#[test]
fn only_touches_the_requested_section() {
    // "height" existe en [bar]; en [general] se añade aparte sin tocar la de [bar]
    let out = set_toml_value(SAMPLE, "general", "height", "5").unwrap();
    assert!(out.contains("[general]\nheight = 5\n") && out.contains("[bar]\n# Estilo\nheight = 28"));
}

#[test]
fn adds_missing_keys_and_sections() {
    let out = set_toml_value(SAMPLE, "bar", "theme", &lit_str("Nord")).unwrap();
    assert!(out.contains("[bar]\ntheme = \"Nord\"\n# Estilo"));
    let out = set_toml_value(SAMPLE, "wallpaper", "dir", &lit_str("C:\\Fondos")).unwrap();
    assert!(out.ends_with("[wallpaper]\ndir = \"C:\\\\Fondos\"\n"), "{out}");
}

#[test]
fn key_prefixes_do_not_match_longer_keys() {
    let text = "[general]\ngaps_inner = 3\ngaps_in = 8\n";
    let out = set_toml_value(text, "general", "gaps_in", "9").unwrap();
    assert_eq!(out, "[general]\ngaps_inner = 3\ngaps_in = 9\n");
}

#[test]
fn crlf_files_are_preserved() {
    let text = "[general]\r\ngaps_in = 8\r\n";
    assert_eq!(set_toml_value(text, "general", "gaps_in", "1").unwrap(), "[general]\r\ngaps_in = 1\r\n");
}

#[test]
fn edits_on_the_real_config_stay_valid() {
    let real = include_str!("../config.toml");
    for (sec, key, lit) in [("general", "gaps_in", "20"), ("bar", "opacity", "60"), ("dock", "icon_size", "48"), ("bar", "theme", "\"Nord\"")] {
        let out = set_toml_value(real, sec, key, lit).unwrap();
        Config::parse(&out).unwrap_or_else(|e| panic!("{sec}.{key}: {e}"));
    }
}

#[test]
fn a_named_theme_overrides_the_bar_colors() {
    let real = include_str!("../config.toml");
    let nord = set_toml_value(real, "bar", "theme", "\"Nord\"").unwrap();
    let cfg = Config::parse(&nord).unwrap();
    assert_eq!(cfg.bar.background, "#2e3440");
    assert_eq!(cfg.bar.accent, "#88c0d0");
    // el tamaño de fuente y la altura siguen siendo los de [bar]
    // (`config.toml` lo edita el usuario: se compara con lo que él tenga en [bar])
    let plain = set_toml_value(real, "bar", "theme", "\"\"").unwrap();
    let plain_cfg = Config::parse(&plain).unwrap();
    assert_eq!(cfg.bar.height, plain_cfg.bar.height);
    assert_eq!(cfg.bar.font_size, plain_cfg.bar.font_size);
    // sin tema, valen los colores de [bar]; con el tema Nord son los de Nord
    assert_ne!(plain_cfg.bar.background, cfg.bar.background);
    // un tema desconocido no impide arrancar
    let unknown = set_toml_value(real, "bar", "theme", "\"No existe\"").unwrap();
    assert!(Config::parse(&unknown).is_ok());
}

#[test]
fn user_theme_files_are_saved_loaded_and_override_builtins() {
    let name = "HyprWin Tema de Prueba";
    struct Cleanup;
    impl Drop for Cleanup {
        fn drop(&mut self) {
            theme::delete_user("HyprWin Tema de Prueba");
        }
    }
    let _guard = Cleanup;
    let mut t = theme::builtins().remove(0);
    t.name = name.into();
    t.accent = "#ff8800".into();
    t.accent_text = theme::readable_on(&t.accent);
    let path = theme::save_user(&t).unwrap();
    assert!(path.is_file());
    let found = theme::find(name).expect("el tema guardado se encuentra");
    assert_eq!(found.accent, "#ff8800");
    assert!(theme::all().iter().any(|x| x.name == name));
    // un tema de usuario con el nombre de uno predefinido lo sustituye
    let mut over = theme::builtins().into_iter().find(|b| b.name == "Nord").unwrap();
    assert!(theme::is_builtin("Nord"));
    over.accent = "#ff0000".into();
    over.name = name.into();
    theme::save_user(&over).unwrap();
    assert_eq!(theme::find(name).unwrap().accent, "#ff0000");
    theme::delete_user(name);
    assert!(theme::find(name).is_none());
}
