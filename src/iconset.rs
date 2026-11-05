//! Packs de iconos para los temas. El código dibuja sus glifos con los códigos de *Segoe Fluent Icons*
//! (los que trae Windows); un pack los traduce a su propia fuente de iconos, incrustada en el programa y
//! cargada sin instalar nada. Cada fuente está recortada a los iconos que usa la interfaz
//! (`tools/build_icon_packs.py`), así que ocupa unos pocos KB.
//!
//! Licencias: Phosphor, Tabler y Fluent UI System Icons (MIT) y Lucide (ISC); los textos están en `assets/icons`.

use std::ops::Deref;

use crate::gfx::Font;
use crate::iconset_data as d;

pub struct Pack {
    /// Valor de `icons` en el tema y en `[bar]`.
    pub key: &'static str,
    pub label: &'static str,
    pub style: &'static str,
    family: &'static str,
    font: &'static [u8],
    map: &'static [(char, char)],
    /// Iconos de estado de la barra y del panel, por nombre (`wifi-3`, `battery-50`...).
    status: &'static [(&'static str, char)],
}

pub static PACKS: [Pack; 7] = [
    Pack { key: "phosphor", label: "Phosphor", style: "Trazo regular", family: d::PHOSPHOR_FAMILY, font: include_bytes!("../assets/icons/phosphor.ttf"), map: d::PHOSPHOR_MAP, status: d::PHOSPHOR_STATUS },
    Pack { key: "phosphor-bold", label: "Phosphor Bold", style: "Trazo grueso", family: d::PHOSPHOR_BOLD_FAMILY, font: include_bytes!("../assets/icons/phosphor-bold.ttf"), map: d::PHOSPHOR_BOLD_MAP, status: d::PHOSPHOR_BOLD_STATUS },
    Pack { key: "phosphor-fill", label: "Phosphor Fill", style: "Rellenos", family: d::PHOSPHOR_FILL_FAMILY, font: include_bytes!("../assets/icons/phosphor-fill.ttf"), map: d::PHOSPHOR_FILL_MAP, status: d::PHOSPHOR_FILL_STATUS },
    Pack { key: "lucide", label: "Lucide", style: "Trazo fino", family: d::LUCIDE_FAMILY, font: include_bytes!("../assets/icons/lucide.ttf"), map: d::LUCIDE_MAP, status: d::LUCIDE_STATUS },
    Pack { key: "fluent", label: "Fluent", style: "Estilo Windows 11", family: d::FLUENT_FAMILY, font: include_bytes!("../assets/icons/fluent.ttf"), map: d::FLUENT_MAP, status: d::FLUENT_STATUS },
    Pack { key: "fluent-fill", label: "Fluent Fill", style: "Windows 11, rellenos", family: d::FLUENT_FILL_FAMILY, font: include_bytes!("../assets/icons/fluent-fill.ttf"), map: d::FLUENT_FILL_MAP, status: d::FLUENT_FILL_STATUS },
    Pack { key: "tabler", label: "Tabler", style: "Trazo redondeado", family: d::TABLER_FAMILY, font: include_bytes!("../assets/icons/tabler.ttf"), map: d::TABLER_MAP, status: d::TABLER_STATUS },
];

/// Tamaño relativo de los glifos de un pack frente a los de Segoe a igual `size`.
const PACK_SCALE: f32 = 1.45;

/// Nombre que se muestra para "sin pack".
pub const SYSTEM_LABEL: &str = "Windows";

pub fn find(key: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.key.eq_ignore_ascii_case(key.trim()))
}

/// Nombre legible del pack con esa clave (`""` = los de Windows).
pub fn label(key: &str) -> &'static str {
    find(key).map_or(SYSTEM_LABEL, |p| p.label)
}

impl Pack {
    /// Glifo del icono de estado `name`, si el pack lo tiene.
    pub fn status_glyph(&self, name: &str) -> Option<char> {
        self.status.iter().find(|(n, _)| *n == name).map(|&(_, g)| g)
    }

    /// Glifo del pack equivalente al de Segoe `segoe`, si lo tiene.
    pub fn glyph(&self, segoe: char) -> Option<char> {
        self.map.iter().find(|(s, _)| *s == segoe).map(|&(_, g)| g)
    }
}

/// Fuente de glifos de la interfaz: la del pack elegido o, si no hay (o no carga), la de Windows.
pub struct IconFont {
    font: Font,
    pack: Option<&'static Pack>,
}

impl IconFont {
    /// `key` vacío o desconocido = *Segoe Fluent Icons*. `size` en px.
    pub fn new(key: &str, size: f32) -> Option<IconFont> {
        if let Some(pack) = find(key) {
            // los glifos de los packs ocupan menos de su cuadrado que los de Segoe, que ya vienen compensados
            if let Some(font) = Font::from_memory(pack.family, pack.font, size * PACK_SCALE) {
                return Some(IconFont { font, pack: Some(pack) });
            }
        }
        Font::with_fallbacks(&["Segoe Fluent Icons", "Segoe MDL2 Assets"], size, false).map(|font| IconFont { font, pack: None })
    }

    /// Dibuja el glifo de Segoe `ch` con esta fuente, centrado en `r` por su forma.
    pub fn draw(&self, c: &mut crate::gfx::Canvas, ch: char, r: crate::gfx::FRect, color: u32) {
        let g = match self.pack {
            Some(p) => p.glyph(ch).unwrap_or('\u{FFFD}'),
            None => ch,
        };
        c.glyph(&self.font, &g.to_string(), r, color);
    }
}

thread_local! {
    /// Fuentes de los iconos de estado por pack y tamaño (crear una fuente de GDI+ cuesta; se repinta a menudo).
    static STATUS_FONTS: std::cell::RefCell<Vec<(&'static str, i32, Font)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Dibuja el icono de estado `name` del pack `key` dentro de `r` (alfa y color en `argb`). Devuelve `false`
/// si no hay pack (o no tiene ese icono): quien llama dibuja entonces su glifo vectorial de siempre.
pub fn draw_status(c: &mut crate::gfx::Canvas, key: &str, name: &str, r: crate::gfx::FRect, argb: u32) -> bool {
    let Some(pack) = find(key) else { return false };
    let Some(glyph) = pack.status_glyph(name) else { return false };
    let size = r.w.max(r.h).round() as i32;
    STATUS_FONTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.iter().any(|(k, s, _)| *k == pack.key && *s == size) {
            let Some(font) = Font::from_memory(pack.family, pack.font, size as f32 * STATUS_SCALE) else { return false };
            cache.push((pack.key, size, font));
        }
        let Some((_, _, font)) = cache.iter().find(|(k, s, _)| *k == pack.key && *s == size) else { return false };
        c.glyph(font, &glyph.to_string(), r, argb);
        true
    })
}

/// Tamaño de los iconos de estado respecto al cuadrado en el que se dibujan.
const STATUS_SCALE: f32 = 1.0;

impl Deref for IconFont {
    type Target = Font;
    fn deref(&self) -> &Font {
        &self.font
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Todos los glifos de Segoe que usa el código.
    fn used_glyphs() -> Vec<char> {
        let mut v: Vec<char> = Vec::new();
        for src in [include_str!("menu.rs"), include_str!("installer.rs"), include_str!("widget_draw.rs"), include_str!("settings.rs")] {
            let mut rest = src;
            while let Some(i) = rest.find("\\u{E") {
                let tail = &rest[i + 3..];
                if let Some(end) = tail.find('}') {
                    if let Some(c) = u32::from_str_radix(&tail[..end], 16).ok().and_then(char::from_u32) {
                        v.push(c);
                    }
                }
                rest = &rest[i + 4..];
            }
        }
        v.sort_unstable();
        v.dedup();
        v
    }

    #[test]
    fn every_pack_has_every_glyph_the_ui_uses() {
        let used = used_glyphs();
        assert!(used.len() > 30, "no se leyeron los glifos: {used:?}");
        for p in &PACKS {
            for &g in &used {
                assert!(p.glyph(g).is_some(), "el pack {} no tiene el glifo U+{:04X}", p.key, g as u32);
            }
        }
    }

    /// Caja (x0, y0, x1, y1) de los píxeles visibles.
    fn ink(c: &crate::gfx::Canvas) -> Option<(i32, i32, i32, i32)> {
        let mut b: Option<(i32, i32, i32, i32)> = None;
        for (i, &p) in c.pixels().iter().enumerate() {
            if p >> 24 > 24 {
                let (x, y) = (i as i32 % c.w, i as i32 / c.w);
                b = Some(b.map_or((x, y, x, y), |(a, b2, c2, d)| (a.min(x), b2.min(y), c2.max(x), d.max(y))));
            }
        }
        b
    }

    #[test]
    fn glyphs_are_centered_and_never_clipped_in_every_pack() {
        let used = used_glyphs();
        let mut keys: Vec<&str> = PACKS.iter().map(|p| p.key).collect();
        keys.push(""); // los de Windows
        for key in keys {
            let font = IconFont::new(key, 22.0).unwrap();
            for &g in &used {
                let mut c = crate::gfx::Canvas::new(60, 60).unwrap();
                font.draw(&mut c, g, crate::gfx::FRect::new(10.0, 10.0, 40.0, 40.0), 0xFFFFFFFF);
                let Some((x0, y0, x1, y1)) = ink(&c) else { panic!("{key} U+{:04X}: no dibuja nada", g as u32) };
                // el centro de la forma cae en el centro del hueco (30, 30), con un píxel de margen por el suavizado
                let (cx, cy) = ((x0 + x1 + 1) as f32 / 2.0, (y0 + y1 + 1) as f32 / 2.0);
                assert!((cx - 30.0).abs() <= 1.5 && (cy - 30.0).abs() <= 1.5, "{key} U+{:04X}: descentrado ({cx}, {cy})", g as u32);
                assert!(x0 > 0 && y0 > 0 && x1 < 59 && y1 < 59, "{key} U+{:04X}: se sale del lienzo", g as u32);
            }
        }
    }

    #[test]
    fn every_pack_has_every_status_icon() {
        for p in &PACKS {
            for name in ["wifi-3", "wifi-2", "wifi-1", "wifi-0", "wifi-off", "speaker-3", "speaker-1", "speaker-0", "speaker-off", "battery-100", "battery-75", "battery-50", "battery-25", "battery-0", "battery-charging", "bluetooth", "lock", "check"] {
                assert!(p.status_glyph(name).is_some(), "{}: falta {name}", p.key);
            }
        }
    }

    #[test]
    fn status_icons_draw_with_a_pack_and_decline_without_one() {
        let mut c = crate::gfx::Canvas::new(40, 40).unwrap();
        let r = crate::gfx::FRect::new(8.0, 8.0, 24.0, 24.0);
        assert!(!draw_status(&mut c, "", "wifi-3", r, 0xFFFFFFFF), "sin pack deja dibujar al glifo vectorial");
        assert!(c.pixels().iter().all(|&p| p >> 24 == 0));
        assert!(!draw_status(&mut c, "lucide", "no-existe", r, 0xFFFFFFFF));
        for p in &PACKS {
            let mut c = crate::gfx::Canvas::new(40, 40).unwrap();
            assert!(draw_status(&mut c, p.key, "wifi-3", r, 0xFFFFFFFF), "{}", p.key);
            assert!(c.pixels().iter().any(|&px| px >> 24 != 0), "{} no dibujó el Wi-Fi", p.key);
        }
    }

    #[test]
    fn keys_are_unique_and_lookup_ignores_case() {
        for (i, a) in PACKS.iter().enumerate() {
            assert!(PACKS.iter().skip(i + 1).all(|b| a.key != b.key));
            assert_eq!(find(&a.key.to_uppercase()).map(|p| p.key), Some(a.key));
        }
        assert!(find("").is_none() && find("nada").is_none());
        assert_eq!(label(""), SYSTEM_LABEL);
    }

    #[test]
    fn the_embedded_fonts_load_and_draw() {
        for p in &PACKS {
            let f = IconFont::new(p.key, 20.0).unwrap_or_else(|| panic!("no carga {}", p.key));
            assert!(f.pack.is_some(), "{} cayó a la fuente de Windows", p.key);
            let mut c = crate::gfx::Canvas::new(40, 40).unwrap();
            f.draw(&mut c, '\u{E713}', crate::gfx::FRect::new(0.0, 0.0, 40.0, 40.0), 0xFFFFFFFF);
            assert!(c.pixels().iter().any(|&px| px >> 24 != 0), "{} no dibujó nada", p.key);
        }
    }
}
