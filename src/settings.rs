//! Registro de ajustes editables desde el menú: qué opciones hay, cómo se muestran y cómo se validan los
//! valores escritos. Lógica pura: guardar y aplicar en vivo lo hacen `config` y `app`.

use crate::config::{lit_array, lit_str, Config};

pub enum Kind {
    Int { min: i32, max: i32, unit: &'static str },
    Float { min: f32, max: f32 },
    Bool,
    Text,
    /// Lista de textos separados por comas.
    List,
}

pub struct Setting {
    pub section: &'static str,
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub hint: &'static str,
    pub kind: Kind,
    /// Cambiarlo exige reiniciar hypr-win.
    pub restart: bool,
}

const fn s(group: &'static str, section: &'static str, key: &'static str, label: &'static str, hint: &'static str, kind: Kind) -> Setting {
    Setting { section, key, label, group, hint, kind, restart: false }
}

const fn restart(mut st: Setting) -> Setting {
    st.restart = true;
    st
}

/// Todos los ajustes (en el orden en que se muestran).
pub const ALL: &[Setting] = &[
    s("Ventanas", "general", "gaps_in", "Hueco entre ventanas", "Separación entre ventanas del mosaico", Kind::Int { min: 0, max: 200, unit: "px" }),
    s("Ventanas", "general", "gaps_out", "Hueco con el borde", "Separación entre las ventanas y el borde de la pantalla", Kind::Int { min: 0, max: 200, unit: "px" }),
    s("Ventanas", "general", "master_ratio", "Ancho de la ventana principal", "Fracción del ancho en el layout master (0,1 - 0,9)", Kind::Float { min: 0.1, max: 0.9 }),
    s("Ventanas", "general", "animation_ms", "Duración de las animaciones", "0 las desactiva", Kind::Int { min: 0, max: 1000, unit: "ms" }),
    s("Ventanas", "general", "focus_follows_mouse", "El foco sigue al ratón", "Enfocar al pasar el ratón, sin hacer clic", Kind::Bool),
    restart(s("Ventanas", "general", "workspaces", "Número de escritorios", "De 1 a 9; antes quita los atajos de los escritorios que sobren", Kind::Int { min: 1, max: 9, unit: "" })),
    s("Sistema", "general", "hide_taskbar", "Ocultar la barra de tareas de Windows", "Se restaura al salir", Kind::Bool),
    s("Sistema", "general", "block_start_menu", "Bloquear el menú Inicio con la tecla Win", "La tecla Win sola no abre Inicio", Kind::Bool),
    s("Barra", "bar", "height", "Alto de la barra", "A 96 DPI", Kind::Int { min: 16, max: 200, unit: "px" }),
    s("Barra", "bar", "font_size", "Tamaño de la letra", "Puntos", Kind::Int { min: 6, max: 48, unit: "pt" }),
    s("Dock", "dock", "icon_size", "Tamaño de los iconos del dock", "A 96 DPI", Kind::Int { min: 24, max: 96, unit: "px" }),
    s("Dock", "dock", "magnification", "Magnificación del dock", "1 = sin magnificar", Kind::Float { min: 1.0, max: 2.5 }),
    s("Dock", "dock", "autohide", "Ocultar el dock automáticamente", "Se oculta cuando hay ventanas", Kind::Bool),
    s("Dock", "dock", "show_running", "Mostrar las apps abiertas en el dock", "Además de las fijadas", Kind::Bool),
    s("Dock", "dock", "pinned", "Apps fijadas en el dock", "Nombres separados por comas", Kind::List),
    restart(s("Dock", "dock", "enabled", "Activar el dock", "", Kind::Bool)),
];

/// Valor actual para mostrarlo ("8 px", "Sí", "Edge, Terminal").
pub fn get(cfg: &Config, st: &Setting) -> String {
    let (g, b, d) = (&cfg.general, &cfg.bar, &cfg.dock);
    let yes = |v: bool| if v { "Sí".to_string() } else { "No".to_string() };
    let unit = |n: i32| match st.kind {
        Kind::Int { unit, .. } if !unit.is_empty() => format!("{n} {unit}"),
        _ => n.to_string(),
    };
    match (st.section, st.key) {
        ("general", "gaps_in") => unit(g.gaps_in),
        ("general", "gaps_out") => unit(g.gaps_out),
        ("general", "master_ratio") => format!("{:.2}", g.master_ratio),
        ("general", "animation_ms") => unit(g.animation_ms as i32),
        ("general", "focus_follows_mouse") => yes(g.focus_follows_mouse),
        ("general", "workspaces") => unit(g.workspaces as i32),
        ("general", "hide_taskbar") => yes(g.hide_taskbar),
        ("general", "block_start_menu") => yes(g.block_start_menu),
        ("bar", "height") => unit(b.height),
        ("bar", "font_size") => unit(b.font_size),
        ("dock", "icon_size") => unit(d.icon_size),
        ("dock", "magnification") => format!("{:.2}", d.magnification),
        ("dock", "autohide") => yes(d.autohide),
        ("dock", "show_running") => yes(d.show_running),
        ("dock", "pinned") => d.pinned.join(", "),
        ("dock", "enabled") => yes(d.enabled),
        _ => String::new(),
    }
}

/// Valor actual de un ajuste booleano.
pub fn bool_value(cfg: &Config, st: &Setting) -> bool {
    get(cfg, st) == "Sí"
}

/// Interpreta lo escrito por el usuario y lo devuelve como literal TOML, o explica qué falla.
pub fn parse(st: &Setting, input: &str) -> Result<String, String> {
    let input = input.trim();
    match &st.kind {
        Kind::Int { min, max, unit } => {
            let n: i32 = input.trim_end_matches(unit).trim().parse().map_err(|_| "escribe un número entero".to_string())?;
            if !(*min..=*max).contains(&n) {
                return Err(format!("debe estar entre {min} y {max}"));
            }
            Ok(n.to_string())
        }
        Kind::Float { min, max } => {
            let n: f32 = input.replace(',', ".").parse().map_err(|_| "escribe un número (p. ej. 0.55)".to_string())?;
            if !(*min..=*max).contains(&n) {
                return Err(format!("debe estar entre {min} y {max}"));
            }
            Ok(format!("{n:.2}").trim_end_matches('0').trim_end_matches('.').to_string())
        }
        Kind::Bool => match input.to_lowercase().as_str() {
            "sí" | "si" | "true" | "1" | "on" | "activado" | "yes" => Ok("true".into()),
            "no" | "false" | "0" | "off" | "desactivado" => Ok("false".into()),
            _ => Err("escribe sí o no".into()),
        },
        Kind::Text => (!input.is_empty()).then(|| lit_str(input)).ok_or_else(|| "no puede estar vacío".to_string()),
        Kind::List => {
            let items: Vec<String> = input.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
            Ok(lit_array(&items))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::set_toml_value;

    fn find(section: &str, key: &str) -> &'static Setting {
        ALL.iter().find(|s| s.section == section && s.key == key).unwrap()
    }

    #[test]
    fn every_setting_reads_a_value_from_the_default_config() {
        let cfg = Config::default_config();
        for st in ALL {
            assert!(!get(&cfg, st).is_empty() || matches!(st.kind, Kind::List), "{}.{} sin valor", st.section, st.key);
        }
        assert_eq!(get(&cfg, find("general", "gaps_in")), "8 px");
        // `config.toml` es el valor por defecto embebido y el usuario lo edita: solo se exige que tenga valor
        assert!(get(&cfg, find("dock", "magnification")).parse::<f32>().is_ok());
        assert_eq!(get(&cfg, find("general", "focus_follows_mouse")), "Sí");
        assert!(!get(&cfg, find("dock", "pinned")).is_empty());
    }

    #[test]
    fn keys_are_unique() {
        let mut v: Vec<(&str, &str)> = ALL.iter().map(|s| (s.section, s.key)).collect();
        v.sort_unstable();
        let n = v.len();
        v.dedup();
        assert_eq!(v.len(), n);
    }

    #[test]
    fn numbers_are_validated_with_ranges_and_units() {
        let gaps = find("general", "gaps_in");
        assert_eq!(parse(gaps, "12").unwrap(), "12");
        assert_eq!(parse(gaps, " 12 px ").unwrap(), "12");
        assert!(parse(gaps, "-1").is_err() && parse(gaps, "999").is_err() && parse(gaps, "mucho").is_err());
        let ratio = find("general", "master_ratio");
        assert_eq!(parse(ratio, "0,6").unwrap(), "0.6");
        assert_eq!(parse(ratio, "0.55").unwrap(), "0.55");
        assert!(parse(ratio, "1.5").is_err());
    }

    #[test]
    fn booleans_and_lists_are_parsed() {
        let b = find("dock", "autohide");
        for yes in ["sí", "Si", "true", "ON"] {
            assert_eq!(parse(b, yes).unwrap(), "true");
        }
        for no in ["no", "False", "0"] {
            assert_eq!(parse(b, no).unwrap(), "false");
        }
        assert!(parse(b, "quizá").is_err());
        let pinned = find("dock", "pinned");
        assert_eq!(parse(pinned, "Edge,  Terminal ,, Chrome").unwrap(), "[\"Edge\", \"Terminal\", \"Chrome\"]");
    }

    /// Cada ajuste, con un valor válido, produce un config.toml que sigue cargando y que devuelve ese valor.
    #[test]
    fn every_setting_round_trips_through_the_real_config_file() {
        let real = include_str!("../config.toml");
        let samples = [
            ("general", "gaps_in", "20", "20 px"),
            ("general", "gaps_out", "0", "0 px"),
            ("general", "master_ratio", "0.7", "0.70"),
            ("general", "animation_ms", "250", "250 ms"),
            ("general", "focus_follows_mouse", "no", "No"),
            ("general", "workspaces", "9", "9"),
            ("general", "hide_taskbar", "no", "No"),
            ("general", "block_start_menu", "sí", "Sí"),
            ("bar", "height", "36", "36 px"),
            ("bar", "font_size", "12", "12 pt"),
            ("dock", "icon_size", "48", "48 px"),
            ("dock", "magnification", "2", "2.00"),
            ("dock", "autohide", "no", "No"),
            ("dock", "show_running", "no", "No"),
            ("dock", "pinned", "Edge, Notepad", "Edge, Notepad"),
            ("dock", "enabled", "no", "No"),
        ];
        assert_eq!(samples.len(), ALL.len(), "falta una muestra para algún ajuste");
        for (section, key, input, shown) in samples {
            let st = find(section, key);
            let literal = parse(st, input).unwrap_or_else(|e| panic!("{section}.{key}: {e}"));
            let text = set_toml_value(real, section, key, &literal).unwrap();
            let cfg = Config::parse(&text).unwrap_or_else(|e| panic!("{section}.{key}={literal}: {e}"));
            assert_eq!(get(&cfg, st), shown, "{section}.{key}");
        }
    }
}

#[cfg(test)]
mod workspace_tests {
    use super::*;
    use crate::config::set_toml_value;

    /// Reducir los escritorios con atajos que apuntan a los que sobran se rechaza en vez de guardar algo roto.
    #[test]
    fn lowering_workspaces_below_the_bindings_is_rejected() {
        let real = include_str!("../config.toml");
        let text = set_toml_value(real, "general", "workspaces", "5").unwrap();
        let err = Config::parse(&text).unwrap_err();
        assert!(err.contains("workspace inexistente"), "{err}");
    }
}
