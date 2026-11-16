//! Cursores por tema. Cada tema puede traer un pack de `.cur`/`.ani` incluido en el binario (`assets/cursors`)
//! o un cursor de flecha generado con sus colores. Los archivos se escriben en `%APPDATA%\hypr-win\cursors\<estilo>`
//! y se apuntan desde `HKCU\Control Panel\Cursors`, que es donde Windows guarda el esquema del usuario. Después
//! `SPI_SETCURSORS` lo recarga para todas las apps. Al salir (o al quitar el estilo) se restauran los valores
//! originales.

use std::path::PathBuf;
use std::sync::Mutex;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
    KEY_WRITE, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPIF_SENDCHANGE, SPI_SETCURSORS};

use crate::config::{parse_color, Bar};

const SIZE: i32 = 32;

/// Claves de `HKCU\Control Panel\Cursors` que cambiamos.
const KEYS: [&str; 4] = ["Arrow", "Hand", "IBeam", "AppStarting"];

/// Una pieza de un pack: (clave del registro, nombre de archivo, contenido).
type Entry = (&'static str, &'static str, &'static [u8]);

/// Packs de cursores incluidos en el binario (`assets/cursors`). Licencias: ver `LICENSE` de cada carpeta.
/// Rosé Pine y macOS (ful1e5) son GPL-3.0; los Capitaine (sainnhe) no tienen licencia verificada.
const ROSEPINE: [Entry; 4] = [
    ("Arrow", "Default.cur", include_bytes!("../assets/cursors/rosepine/Default.cur")),
    ("Hand", "Link.cur", include_bytes!("../assets/cursors/rosepine/Link.cur")),
    ("IBeam", "IBeam.cur", include_bytes!("../assets/cursors/rosepine/IBeam.cur")),
    ("AppStarting", "Busy.ani", include_bytes!("../assets/cursors/rosepine/Busy.ani")),
];
const MACOS_REGULAR: [Entry; 4] = [
    ("Arrow", "Pointer.cur", include_bytes!("../assets/cursors/macos-regular/Pointer.cur")),
    ("Hand", "Link.cur", include_bytes!("../assets/cursors/macos-regular/Link.cur")),
    ("IBeam", "Text.cur", include_bytes!("../assets/cursors/macos-regular/Text.cur")),
    ("AppStarting", "Busy.ani", include_bytes!("../assets/cursors/macos-regular/Busy.ani")),
];
const MACOS_WHITE: [Entry; 4] = [
    ("Arrow", "Pointer.cur", include_bytes!("../assets/cursors/macos-white/Pointer.cur")),
    ("Hand", "Link.cur", include_bytes!("../assets/cursors/macos-white/Link.cur")),
    ("IBeam", "Text.cur", include_bytes!("../assets/cursors/macos-white/Text.cur")),
    ("AppStarting", "Busy.ani", include_bytes!("../assets/cursors/macos-white/Busy.ani")),
];
const CAPITAINE_GRUVBOX: [Entry; 4] = [
    ("Arrow", "Normal.cur", include_bytes!("../assets/cursors/capitaine-gruvbox/Normal.cur")),
    ("Hand", "Link.cur", include_bytes!("../assets/cursors/capitaine-gruvbox/Link.cur")),
    ("IBeam", "Text.cur", include_bytes!("../assets/cursors/capitaine-gruvbox/Text.cur")),
    ("AppStarting", "Working.ani", include_bytes!("../assets/cursors/capitaine-gruvbox/Working.ani")),
];
const CAPITAINE_NORD: [Entry; 4] = [
    ("Arrow", "Normal.cur", include_bytes!("../assets/cursors/capitaine-nord/Normal.cur")),
    ("Hand", "Link.cur", include_bytes!("../assets/cursors/capitaine-nord/Link.cur")),
    ("IBeam", "Text.cur", include_bytes!("../assets/cursors/capitaine-nord/Text.cur")),
    ("AppStarting", "Working.ani", include_bytes!("../assets/cursors/capitaine-nord/Working.ani")),
];

/// Nombre del pack -> sus piezas.
const PACKS: [(&str, &[Entry]); 5] = [
    ("rosepine", &ROSEPINE),
    ("macos-regular", &MACOS_REGULAR),
    ("macos-white", &MACOS_WHITE),
    ("capitaine-gruvbox", &CAPITAINE_GRUVBOX),
    ("capitaine-nord", &CAPITAINE_NORD),
];

/// Estilos de cursor válidos en un tema: los generados con los colores del tema y los packs incluidos.
pub const STYLES: [&str; 10] =
    ["classic", "contrast", "accent", "soft", "katana", "rosepine", "macos-regular", "macos-white", "capitaine-gruvbox", "capitaine-nord"];

/// Valores originales de `KEYS` (tipo y datos, o `None` si no existían) antes de nuestro primer cambio.
/// `Some` mientras tengamos cursores propios aplicados.
type Saved = Vec<(&'static str, Option<RegValue>)>;
static ORIGINAL: Mutex<Option<Saved>> = Mutex::new(None);

/// Colores (COLORREF) de un estilo generado. `None` si el estilo no existe o es un pack.
pub fn colors(style: &str, bg: u32, fg: u32, dim: u32, accent: u32, accent_text: u32) -> Option<(u32, u32)> {
    match style {
        "classic" => Some((fg, bg)),
        "contrast" => Some((bg, fg)),
        "accent" => Some((accent, accent_text)),
        "soft" => Some((dim, bg)),
        "katana" => Some((fg, bg)), // la hoja; el resto de colores los pone `katana_pixels`
        _ => None,
    }
}

/// Flecha clásica en una rejilla de 32 px; la punta está en (1, 1), que es el punto de acción.
const ARROW: [(f32, f32); 7] = [(1.0, 1.0), (1.0, 23.0), (6.0, 18.0), (10.0, 27.0), (14.0, 25.0), (10.0, 16.0), (18.0, 16.0)];

/// Distancia de un punto a un segmento.
fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.0 + t * dx - p.0, a.1 + t * dy - p.1);
    (cx * cx + cy * cy).sqrt()
}

/// Regla par-impar: ¿está el punto dentro del polígono?
fn inside(p: (f32, f32), poly: &[(f32, f32)]) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
        j = i;
    }
    c
}

/// Píxeles BGRA (alfa sin premultiplicar), de arriba abajo: relleno, contorno de 1 px y transparente fuera.
pub fn arrow_pixels(fill: u32, outline: u32) -> Vec<u32> {
    let argb = |c: u32, a: u32| -> u32 {
        let (r, g, b) = (c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff);
        (a << 24) | (r << 16) | (g << 8) | b
    };
    let mut out = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let d = (0..ARROW.len())
                .map(|i| seg_dist(p, ARROW[i], ARROW[(i + 1) % ARROW.len()]))
                .fold(f32::MAX, f32::min);
            let px = if inside(p, &ARROW) {
                if d < 1.0 { argb(outline, 255) } else { argb(fill, 255) }
            } else if d < 1.0 {
                argb(outline, ((1.0 - d) * 255.0) as u32)
            } else {
                0
            };
            out.push(px);
        }
    }
    out
}

/// Katana en diagonal con la punta en (1, 1): hoja (`blade`), guarda (`guard`) y empuñadura (`grip`), con
/// contorno de 1 px. Mismo formato de píxeles que [`arrow_pixels`].
pub fn katana_pixels(blade: u32, guard: u32, grip: u32, outline: u32) -> Vec<u32> {
    let argb = |c: u32, a: u32| -> u32 {
        let (r, g, b) = (c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff);
        (a << 24) | (r << 16) | (g << 8) | b
    };
    // eje de la espada: desde la punta hacia abajo a la derecha
    let (dx, dy) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    let at = |s: f32, off: f32| (1.0 + dx * s - dy * off, 1.0 + dy * s + dx * off);
    let part = |s0: f32, s1: f32, w0: f32, w1: f32| vec![at(s0, -w0), at(s1, -w1), at(s1, w1), at(s0, w0)];
    let parts: [(Vec<(f32, f32)>, u32); 3] = [
        // hoja: se ensancha en los primeros píxeles y sigue recta
        (vec![at(0.0, 0.0), at(6.0, -2.0), at(26.0, -2.0), at(26.0, 2.0), at(6.0, 2.0)], blade),
        (part(26.0, 28.5, 4.8, 4.8), guard),
        (part(28.5, 41.0, 2.0, 2.0), grip),
    ];
    let mut out = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let mut px = 0;
            let mut near = f32::MAX;
            for (poly, fill) in &parts {
                let d = (0..poly.len()).map(|i| seg_dist(p, poly[i], poly[(i + 1) % poly.len()])).fold(f32::MAX, f32::min);
                near = near.min(d);
                if inside(p, poly) {
                    px = if d < 0.9 { argb(outline, 255) } else { argb(*fill, 255) };
                }
            }
            if px == 0 && near < 1.0 {
                px = argb(outline, ((1.0 - near) * 255.0) as u32);
            }
            out.push(px);
        }
    }
    out
}

/// Archivo `.cur` (formato ICO con tipo 2) de 32x32 con transparencia por canal alfa y punta en (0, 0).
pub fn cur_bytes(pixels: &[u32]) -> Vec<u8> {
    let image_len: u32 = 40 + (SIZE * SIZE * 4) as u32 + (SIZE as u32 * 4); // cabecera + píxeles + máscara AND
    let mut v = Vec::with_capacity(22 + image_len as usize);
    // ICONDIR: reservado, tipo 2 (cursor), 1 imagen
    v.extend_from_slice(&[0, 0, 2, 0, 1, 0]);
    // ICONDIRENTRY: 32x32, sin paleta, reservado, punta (x, y), tamaño, desplazamiento
    v.extend_from_slice(&[SIZE as u8, SIZE as u8, 0, 0]);
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&0u16.to_le_bytes());
    v.extend_from_slice(&image_len.to_le_bytes());
    v.extend_from_slice(&22u32.to_le_bytes());
    // BITMAPINFOHEADER: la altura incluye la máscara AND (el doble)
    v.extend_from_slice(&40u32.to_le_bytes());
    v.extend_from_slice(&SIZE.to_le_bytes());
    v.extend_from_slice(&(SIZE * 2).to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&32u16.to_le_bytes());
    v.extend_from_slice(&[0u8; 24]); // compresión, tamaños, resoluciones y colores: sin usar
    // píxeles: el DIB va de abajo arriba
    for y in (0..SIZE).rev() {
        for x in 0..SIZE {
            v.extend_from_slice(&pixels[(y * SIZE + x) as usize].to_le_bytes());
        }
    }
    // máscara AND: ceros (la transparencia viene del alfa)
    v.extend_from_slice(&vec![0u8; (SIZE as usize) * 4]);
    v
}

fn cursors_root() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("hypr-win").join("cursors"))
}

/// Valor del registro: tipo y datos.
type RegValue = (u32, Vec<u8>);

/// Lee un valor del registro (tipo y datos). `None` si no existe.
unsafe fn read_value(hk: HKEY, name: &str) -> Option<RegValue> {
    let n = HSTRING::from(name);
    let mut ty = REG_VALUE_TYPE(0);
    let mut size = 0u32;
    RegQueryValueExW(hk, PCWSTR(n.as_ptr()), None, Some(&mut ty), None, Some(&mut size)).ok().ok()?;
    let mut data = vec![0u8; size as usize];
    RegQueryValueExW(hk, PCWSTR(n.as_ptr()), None, Some(&mut ty), Some(data.as_mut_ptr()), Some(&mut size)).ok().ok()?;
    data.truncate(size as usize);
    Some((ty.0, data))
}

/// Escribe (o borra si `value` es `None`) un valor del registro.
unsafe fn write_value(hk: HKEY, name: &str, value: Option<&RegValue>) {
    let n = HSTRING::from(name);
    match value {
        Some((ty, data)) => {
            let _ = RegSetValueExW(hk, PCWSTR(n.as_ptr()), None, REG_VALUE_TYPE(*ty), Some(data));
        }
        None => {
            let _ = RegDeleteValueW(hk, PCWSTR(n.as_ptr()));
        }
    }
}

/// Abre `HKCU\Control Panel\Cursors`. Quien lo abre debe cerrarlo con `RegCloseKey`.
unsafe fn open_cursors_key() -> Result<HKEY, String> {
    let mut hk = HKEY::default();
    RegOpenKeyExW(HKEY_CURRENT_USER, w!("Control Panel\\Cursors"), None, KEY_READ | KEY_WRITE, &mut hk)
        .ok()
        .map_err(|e| e.to_string())?;
    Ok(hk)
}

/// Recarga el esquema de cursores en todas las apps (lo mismo que hace el Panel de control).
fn broadcast() {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETCURSORS, 0, None, SPIF_SENDCHANGE);
    }
}

/// Escribe los archivos del estilo y los apunta en el registro.
fn write_scheme(style: &str, files: &[(&str, &str, Vec<u8>)]) -> Result<(), String> {
    let dir = cursors_root().ok_or("no se encontró APPDATA")?.join(style);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut paths = Vec::new();
    for (key, file, bytes) in files {
        let p = dir.join(file);
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        // REG_SZ: texto UTF-16 terminado en nulo
        let text: Vec<u16> = p.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let raw: Vec<u8> = text.iter().flat_map(|c| c.to_le_bytes()).collect();
        paths.push((*key, (REG_SZ.0, raw)));
    }
    unsafe {
        let hk = open_cursors_key()?;
        if let Ok(mut orig) = ORIGINAL.lock() {
            if orig.is_none() {
                *orig = Some(KEYS.iter().map(|k| (*k, read_value(hk, k))).collect());
            }
        }
        for (key, value) in &paths {
            write_value(hk, key, Some(value));
        }
        let _ = RegCloseKey(hk);
    }
    broadcast();
    Ok(())
}

/// Aplica el cursor del estilo de `bar.cursor`. Vacío = cursores de Windows (equivale a `restore`).
pub fn apply(bar: &Bar) -> Result<(), String> {
    let style = bar.cursor.trim();
    if style.is_empty() {
        restore();
        return Ok(());
    }
    if let Some((_, entries)) = PACKS.iter().find(|(n, _)| *n == style) {
        let files: Vec<(&str, &str, Vec<u8>)> = entries.iter().map(|(k, f, b)| (*k, *f, b.to_vec())).collect();
        return write_scheme(style, &files);
    }
    let c = |s: &str| parse_color(s);
    if style == "katana" {
        let (fg, bg, dim, accent) = (c(&bar.foreground)?, c(&bar.background)?, c(&bar.dim)?, c(&bar.accent)?);
        let files = [("Arrow", "Normal.cur", cur_bytes(&katana_pixels(fg, dim, accent, bg)))];
        return write_scheme(style, &files);
    }
    let (fill, outline) = colors(
        style,
        c(&bar.background)?,
        c(&bar.foreground)?,
        c(&bar.dim)?,
        c(&bar.accent)?,
        c(&bar.accent_text)?,
    )
    .ok_or_else(|| format!("estilo de cursor desconocido: {style}"))?;
    let files = [("Arrow", "Normal.cur", cur_bytes(&arrow_pixels(fill, outline)))];
    write_scheme(style, &files)
}

/// Devuelve los cursores originales del usuario si antes aplicamos los nuestros.
pub fn restore() {
    let Some(saved) = ORIGINAL.lock().ok().and_then(|mut g| g.take()) else { return };
    unsafe {
        if let Ok(hk) = open_cursors_key() {
            for (key, value) in &saved {
                write_value(hk, key, value.as_ref());
            }
            let _ = RegCloseKey(hk);
        }
    }
    broadcast();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_has_colors_and_unknown_is_none() {
        for s in STYLES.iter().filter(|s| !PACKS.iter().any(|(p, _)| p == *s)) {
            assert!(colors(s, 0, 0xffffff, 0x888888, 0xff0000, 0xffffff).is_some(), "{s}");
        }
        assert!(colors("raro", 0, 0, 0, 0, 0).is_none());
    }

    #[test]
    fn every_pack_is_listed_as_a_style() {
        for (name, entries) in PACKS {
            assert!(STYLES.contains(&name), "{name} falta en STYLES");
            assert!(entries.len() == 4 && entries.iter().all(|(_, _, b)| !b.is_empty()), "{name}");
        }
    }

    #[test]
    fn arrow_has_tip_filled_and_transparent_corner() {
        let px = arrow_pixels(0x0000ff, 0x000000);
        assert_eq!(px.len(), (SIZE * SIZE) as usize);
        // interior de la flecha: relleno rojo (COLORREF 0x0000ff = rojo) con alfa completo
        assert_eq!(px[(10 * SIZE + 3) as usize] & 0x00ff_ffff, 0x00ff_0000);
        assert_eq!(px[(10 * SIZE + 3) as usize] >> 24, 255);
        // esquina inferior derecha: transparente
        assert_eq!(px[(31 * SIZE + 31) as usize] >> 24, 0);
    }

    #[test]
    fn katana_has_blade_guard_grip_and_a_clear_corner() {
        let px = katana_pixels(0xffffff, 0x00ffff, 0x0000ff, 0);
        assert_eq!(px.len(), (SIZE * SIZE) as usize);
        let at = |x: i32, y: i32| px[(y * SIZE + x) as usize];
        assert!(at(1, 1) >> 24 > 0, "la punta debe estar en (1, 1)");
        assert_eq!(at(10, 10) & 0x00ff_ffff, 0x00ff_ffff, "hoja"); // COLORREF blanco
        assert_eq!(at(28, 28) & 0x00ff_ffff, 0x00ff_0000, "empuñadura roja");
        assert_eq!(at(31, 0) >> 24, 0);
        assert_eq!(at(0, 31) >> 24, 0);
    }

    #[test]
    fn generated_cursor_file_has_valid_layout() {
        let bytes = cur_bytes(&arrow_pixels(0x0000ff, 0));
        assert_eq!(&bytes[0..4], &[0, 0, 2, 0]);
        assert_eq!(bytes.len(), 22 + 40 + 4096 + 128);
        // la imagen empieza en 22 y su cabecera mide 40 bytes
        assert_eq!(u32::from_le_bytes(bytes[14..18].try_into().unwrap()) as usize, bytes.len() - 22);
        assert_eq!(u32::from_le_bytes(bytes[22..26].try_into().unwrap()), 40);
    }

    /// A mano: `cargo test dump_katana -- --ignored` escribe `%TEMP%\katana.png` (ampliado 10x sobre un fondo gris).
    #[test]
    #[ignore]
    fn dump_katana() {
        let px = katana_pixels(0xd6e4ec, 0x4cb8d8, 0x2e10c8, 0x0d0b0e);
        let k = 10;
        let mut c = crate::gfx::Canvas::new(SIZE * k, SIZE * k).unwrap();
        let mut out = Vec::new();
        for y in 0..SIZE * k {
            for x in 0..SIZE * k {
                let p = px[((y / k) * SIZE + x / k) as usize];
                let a = (p >> 24) as f32 / 255.0;
                let ch = |v: u32| (((v & 0xff) as f32) * a + 90.0 * (1.0 - a)) as u32;
                out.push(0xFF00_0000 | ch(p >> 16) << 16 | ch(p >> 8) << 8 | ch(p));
            }
        }
        c.fill_from(&out);
        assert!(c.save_png(&std::env::temp_dir().join("katana.png").to_string_lossy()));
    }
}
