//! Fondo de escritorio: aplicar una imagen (a todos los monitores o a uno) con la interfaz documentada
//! `IDesktopWallpaper`, listar imágenes, generar degradados con los colores del tema y restaurar el fondo
//! original. Requiere COM inicializado en el hilo que llama.

use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Storage::StorageFile;
use windows::System::UserProfile::LockScreen;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
use windows::Win32::UI::Shell::{
    DesktopWallpaper, IDesktopWallpaper, DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL, DWPOS_FIT, DWPOS_SPAN,
    DWPOS_STRETCH, DWPOS_TILE,
};

// ---------- ajuste de la imagen ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    Fill,
    Fit,
    Stretch,
    Tile,
    Center,
    Span,
}

impl Fit {
    pub const ALL: [Fit; 6] = [Fit::Fill, Fit::Fit, Fit::Stretch, Fit::Tile, Fit::Center, Fit::Span];

    pub fn label(self) -> &'static str {
        match self {
            Fit::Fill => "Rellenar",
            Fit::Fit => "Ajustar",
            Fit::Stretch => "Expandir",
            Fit::Tile => "Mosaico",
            Fit::Center => "Centrar",
            Fit::Span => "Abarcar todos los monitores",
        }
    }

    /// Valor en `config.toml`.
    pub fn key(self) -> &'static str {
        match self {
            Fit::Fill => "fill",
            Fit::Fit => "fit",
            Fit::Stretch => "stretch",
            Fit::Tile => "tile",
            Fit::Center => "center",
            Fit::Span => "span",
        }
    }

    pub fn parse(s: &str) -> Option<Fit> {
        Fit::ALL.into_iter().find(|f| f.key().eq_ignore_ascii_case(s.trim()))
    }

    fn to_win(self) -> DESKTOP_WALLPAPER_POSITION {
        match self {
            Fit::Fill => DWPOS_FILL,
            Fit::Fit => DWPOS_FIT,
            Fit::Stretch => DWPOS_STRETCH,
            Fit::Tile => DWPOS_TILE,
            Fit::Center => DWPOS_CENTER,
            Fit::Span => DWPOS_SPAN,
        }
    }

    fn from_win(p: DESKTOP_WALLPAPER_POSITION) -> Option<Fit> {
        Fit::ALL.into_iter().find(|f| f.to_win() == p)
    }
}

// ---------- IDesktopWallpaper ----------

fn api() -> Result<IDesktopWallpaper, String> {
    unsafe { CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL) }.map_err(|e| e.message())
}

fn take_string(p: windows::core::PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as *const _)) };
    s
}

/// Identificadores de los monitores que conoce el escritorio.
pub fn monitor_ids() -> Vec<String> {
    let Ok(api) = api() else { return Vec::new() };
    unsafe {
        let n = api.GetMonitorDevicePathCount().unwrap_or(0);
        (0..n).filter_map(|i| api.GetMonitorDevicePathAt(i).ok()).map(take_string).collect()
    }
}

/// Monitores realmente conectados (el escritorio recuerda también pantallas que ya no están).
/// Devuelve (identificador, ancho, alto).
pub fn connected_monitors() -> Vec<(String, i32, i32)> {
    let Ok(api) = api() else { return Vec::new() };
    monitor_ids()
        .into_iter()
        .filter_map(|id| {
            let r = unsafe { api.GetMonitorRECT(&HSTRING::from(id.as_str())) }.ok()?;
            Some((id, r.right - r.left, r.bottom - r.top))
        })
        .collect()
}

/// Fondo actual de cada monitor (ruta; vacía si no hay imagen).
pub fn current() -> Vec<String> {
    let Ok(api) = api() else { return Vec::new() };
    monitor_ids()
        .iter()
        .map(|id| unsafe { api.GetWallpaper(&HSTRING::from(id.as_str())) }.map(take_string).unwrap_or_default())
        .collect()
}

/// Ajuste actual (Rellenar, Ajustar...).
pub fn fit() -> Option<Fit> {
    Fit::from_win(unsafe { api().ok()?.GetPosition() }.ok()?)
}

pub fn set_fit(fit: Fit) -> Result<(), String> {
    unsafe { api()?.SetPosition(fit.to_win()) }.map_err(|e| e.message())
}

/// Aplica `path` a todos los monitores (`target = None`) o al monitor número `target`.
/// Antes de tocar nada guarda el fondo original, para poder restaurarlo.
pub fn set(path: &str, target: Option<usize>) -> Result<(), String> {
    if !Path::new(path).is_file() {
        return Err(format!("no existe la imagen: {path}"));
    }
    remember_original();
    let api = api()?;
    let file = HSTRING::from(path);
    unsafe {
        match target {
            None => api.SetWallpaper(PCWSTR::null(), &file),
            Some(i) => {
                let id = connected_monitors().get(i).map(|m| m.0.clone()).ok_or("ese monitor no existe")?;
                api.SetWallpaper(&HSTRING::from(id), &file)
            }
        }
    }
    .map_err(|e| {
        if e.code().0 as u32 == 0x8007_0002 {
            "Windows no acepta esa imagen: guárdala en Imágenes (no en AppData) y vuelve a intentarlo".to_string()
        } else {
            e.message()
        }
    })
}

fn original_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join(r"hypr-win\wallpaper-original.txt"))
}

/// Guarda el fondo de cada monitor la primera vez que se cambia (una línea por monitor: `id<TAB>ruta`).
pub fn remember_original() {
    let Some(file) = original_file() else { return };
    if file.exists() {
        return;
    }
    let (ids, paths) = (monitor_ids(), current());
    let text: String = ids.iter().zip(paths).map(|(i, p)| format!("{i}\t{p}\n")).collect();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, text);
}

pub fn has_original() -> bool {
    original_file().is_some_and(|f| f.is_file())
}

/// Vuelve al fondo que había antes de usar hypr-win.
pub fn restore_original() -> Result<(), String> {
    let file = original_file().ok_or("no hay copia del fondo original")?;
    let text = std::fs::read_to_string(&file).map_err(|_| "no hay copia del fondo original".to_string())?;
    let api = api()?;
    let mut restored = 0;
    for line in text.lines() {
        let Some((id, path)) = line.split_once('\t') else { continue };
        if path.is_empty() || !Path::new(path).is_file() {
            continue;
        }
        if unsafe { api.SetWallpaper(&HSTRING::from(id), &HSTRING::from(path)) }.is_ok() {
            restored += 1;
        }
    }
    if restored == 0 {
        return Err("el fondo original ya no existe".into());
    }
    let _ = std::fs::remove_file(file);
    Ok(())
}

// ---------- pantalla de bloqueo ----------

fn lock_original_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join(r"hypr-win\lockscreen-original.txt"))
}

/// "file:///C:/Users/x/Mi%20foto.jpg" -> `C:\Users\x\Mi foto.jpg`.
pub fn file_uri_to_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file:///").or_else(|| uri.strip_prefix("file://"))?;
    let raw = rest.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            let hex = std::str::from_utf8(&raw[i + 1..i + 3]).ok().and_then(|s| u8::from_str_radix(s, 16).ok());
            if let Some(v) = hex {
                bytes.push(v);
                i += 3;
                continue;
            }
        }
        bytes.push(if raw[i] == b'/' { b'\\' } else { raw[i] });
        i += 1;
    }
    String::from_utf8(bytes).ok().filter(|p| !p.is_empty())
}

/// Imagen que tiene ahora la pantalla de bloqueo, si es un archivo (con Spotlight, una de sus imágenes en caché).
pub fn current_lock_image() -> Option<String> {
    let uri = LockScreen::OriginalImageFile().ok()?;
    file_uri_to_path(&uri.AbsoluteUri().ok()?.to_string())
}

/// Guarda la imagen actual de la pantalla de bloqueo la primera vez que se cambia.
fn remember_original_lock() {
    let Some(file) = lock_original_file() else { return };
    if file.exists() {
        return;
    }
    if let Some(img) = current_lock_image() {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(file, img);
    }
}

pub fn has_original_lock() -> bool {
    lock_original_file().is_some_and(|f| f.is_file())
}

/// Pone `path` como imagen de la pantalla de bloqueo (WinRT `LockScreen.SetImageFileAsync`, documentada).
/// Windows pasa entonces de "Windows Spotlight" a "Imagen"; volver a Spotlight se hace en Configuración.
pub fn set_lock_screen(path: &str) -> Result<(), String> {
    if !Path::new(path).is_file() {
        return Err(format!("no existe la imagen: {path}"));
    }
    remember_original_lock();
    let e = |e: windows::core::Error| e.message();
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path)).map_err(e)?.join().map_err(e)?;
    LockScreen::SetImageFileAsync(&file).map_err(e)?.join().map_err(e)
}

/// Vuelve a la imagen de bloqueo que había (como "Imagen"; Spotlight no se puede reactivar por API).
pub fn restore_lock_screen() -> Result<(), String> {
    let file = lock_original_file().ok_or("no hay copia de la pantalla de bloqueo")?;
    let img = std::fs::read_to_string(&file).map_err(|_| "no hay copia de la pantalla de bloqueo".to_string())?;
    set_lock_screen(img.trim())?;
    let _ = std::fs::remove_file(file);
    Ok(())
}

// ---------- imágenes ----------

#[derive(Clone, Debug)]
pub struct Image {
    pub path: PathBuf,
    pub name: String,
    /// De dónde sale ("Mis fondos", "Windows", "Generados").
    pub source: &'static str,
}

const EXTENSIONS: [&str; 4] = ["jpg", "jpeg", "png", "bmp"];

/// Fondos animados que se pueden usar como archivo (GIF animados).
pub fn is_animated_file(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("gif"))
}

/// GIF animados de la carpeta del usuario (y una subcarpeta de profundidad).
pub fn animated_files(configured_dir: &str, limit: usize) -> Vec<Image> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<Image>, limit: usize) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if out.len() >= limit {
                return;
            }
            if p.is_dir() {
                if depth > 0 {
                    walk(&p, depth - 1, out, limit);
                }
            } else if is_animated_file(&p) {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(Image { path: p, name, source: "GIF animado" });
            }
        }
    }
    let mut v = Vec::new();
    if let Some(d) = user_dir(configured_dir) {
        walk(&d, 1, &mut v, limit);
    }
    v
}

pub fn is_image(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn collect(dir: &Path, depth: usize, source: &'static str, out: &mut Vec<Image>, limit: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if out.len() >= limit {
            return;
        }
        if p.is_dir() {
            if depth > 0 {
                collect(&p, depth - 1, source, out, limit);
            }
        } else if is_image(&p) {
            let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(Image { path: p, name, source });
        }
    }
}

/// Carpeta donde se guardan los fondos generados: `Imágenes\Wallpapers\Generados`.
///
/// No puede estar en `AppData`: `IDesktopWallpaper` rechaza ("archivo no encontrado") las imágenes de
/// carpetas propias bajo `AppData`, aunque existan; en `Imágenes` y en `Temp` sí las acepta.
pub fn generated_dir() -> Option<PathBuf> {
    Some(user_dir("")?.join("Generados"))
}

/// Carpeta de imágenes del usuario: la configurada o `Imágenes\Wallpapers`.
pub fn user_dir(configured: &str) -> Option<PathBuf> {
    if !configured.trim().is_empty() {
        return Some(PathBuf::from(configured.trim()));
    }
    Some(PathBuf::from(std::env::var_os("USERPROFILE")?).join(r"Pictures\Wallpapers"))
}

/// Imágenes disponibles: las generadas, las del usuario y las que trae Windows.
pub fn images(configured_dir: &str, limit: usize) -> Vec<Image> {
    let mut v = Vec::new();
    // los generados primero (viven dentro de la carpeta del usuario predeterminada)
    if let Some(d) = generated_dir() {
        collect(&d, 0, "Generados", &mut v, limit);
    }
    if let Some(d) = user_dir(configured_dir) {
        let mut mine = Vec::new();
        collect(&d, 2, "Mis fondos", &mut mine, limit);
        let generated = generated_dir();
        v.extend(mine.into_iter().filter(|i| generated.as_ref().is_none_or(|g| i.path.parent() != Some(g.as_path()))));
    }
    if let Some(win) = std::env::var_os("SystemRoot").map(PathBuf::from) {
        collect(&win.join(r"Web\Wallpaper"), 2, "Windows", &mut v, limit);
        collect(&win.join(r"Web\4K\Wallpaper"), 2, "Windows", &mut v, limit);
    }
    v
}

/// Índice pseudoaleatorio en `0..n` (reloj del sistema; basta para elegir un fondo).
pub fn random_index(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let mut x = nanos ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    (x % n as u64) as usize
}

// ---------- degradados generados ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Gradient,
    Glow,
    Aurora,
    Solid,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Gradient, Kind::Glow, Kind::Aurora, Kind::Solid];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Gradient => "Degradado",
            Kind::Glow => "Resplandor",
            Kind::Aurora => "Aurora",
            Kind::Solid => "Color liso",
        }
    }

    /// Nombre usado en los temas (`wallpaper = "..."`).
    pub fn key(self) -> &'static str {
        match self {
            Kind::Gradient => "gradient",
            Kind::Glow => "glow",
            Kind::Aurora => "aurora",
            Kind::Solid => "solid",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.key().eq_ignore_ascii_case(s.trim()))
    }
}

type Rgb = [f32; 3];

pub(crate) fn unpack(colorref: u32) -> Rgb {
    [(colorref & 0xff) as f32 / 255.0, ((colorref >> 8) & 0xff) as f32 / 255.0, ((colorref >> 16) & 0xff) as f32 / 255.0]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Gira el tono `deg` grados (para los reflejos de la aurora).
pub fn hue_rotate(c: Rgb, deg: f32) -> Rgb {
    let (r, g, b) = (c[0], c[1], c[2]);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d < 1e-6 {
        return c;
    }
    let h = if max == r { ((g - b) / d).rem_euclid(6.0) } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    let h = (h * 60.0 + deg).rem_euclid(360.0) / 60.0;
    let (s, v) = (d / max, max);
    let x = v * s * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let m = v - v * s;
    let (r1, g1, b1) = match h as u32 {
        0 => (v * s, x, 0.0),
        1 => (x, v * s, 0.0),
        2 => (0.0, v * s, x),
        3 => (0.0, x, v * s),
        4 => (x, 0.0, v * s),
        _ => (v * s, 0.0, x),
    };
    [r1 + m, g1 + m, b1 + m]
}

fn pack(c: Rgb, noise: f32) -> u32 {
    let ch = |v: f32| ((v * 255.0 + noise).round().clamp(0.0, 255.0)) as u32;
    0xFF00_0000 | ch(c[0]) << 16 | ch(c[1]) << 8 | ch(c[2])
}

/// Ruido determinista de ±0,5 niveles: evita las bandas visibles en los degradados suaves.
fn dither(x: usize, y: usize) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    (h & 0xff) as f32 / 255.0 - 0.5
}

/// Píxeles ARGB opacos (`w` x `h`) de un fondo generado con los colores del tema (COLORREF).
pub fn render(kind: Kind, w: usize, h: usize, bg: u32, accent: u32) -> Vec<u32> {
    render_at(kind, w, h, bg, accent, 0.0)
}

/// Como [`render`] pero en el instante `t` (segundos) de la animación: las manchas de luz se desplazan
/// con movimientos lentos y periódicos. Con `t = 0` coincide exactamente con el fondo estático.
pub fn render_at(kind: Kind, w: usize, h: usize, bg: u32, accent: u32, t: f32) -> Vec<u32> {
    let (bg, accent) = (unpack(bg), unpack(accent));
    let (fw, fh) = (w as f32, h as f32);
    let blob = |x: f32, y: f32, cx: f32, cy: f32, r: f32| {
        let d = ((x - cx * fw).powi(2) + (y - cy * fh).powi(2)).sqrt() / (r * fw.max(fh));
        (1.0 - d.clamp(0.0, 1.0)).powi(2)
    };
    let tints = [accent, hue_rotate(accent, 60.0), hue_rotate(accent, -55.0)];
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32, y as f32);
            let c = match kind {
                Kind::Solid => bg,
                Kind::Gradient => {
                    let u = ((px / fw + py / fh) / 2.0 + 0.25 * (t * 0.35).sin()).clamp(0.0, 1.0);
                    let u = u * u * (3.0 - 2.0 * u); // suavizado
                    mix(bg, mix(bg, accent, 0.5), u)
                }
                Kind::Glow => {
                    let (cx, cy) = (0.25 + 0.2 * (t * 0.3).sin(), 0.2 + 0.15 * (t * 0.23).sin());
                    mix(bg, accent, blob(px, py, cx, cy, 0.95) * 0.6)
                }
                Kind::Aurora => {
                    let mut c = bg;
                    for (i, (tint, (cx, cy, r))) in tints.iter().zip([(0.2, 0.25, 0.6), (0.8, 0.3, 0.55), (0.55, 0.9, 0.65)]).enumerate() {
                        let (sx, sy) = (i as f32 * 2.1, i as f32 * 1.3);
                        // desplazamiento nulo en t = 0: el primer fotograma es el fondo estático
                        let (cx, cy) = (cx + 0.14 * ((t * 0.31 + sx).sin() - sx.sin()), cy + 0.12 * ((t * 0.27 + sy).cos() - sy.cos()));
                        let k = blob(px, py, cx, cy, r) * 0.55;
                        c = [c[0] + (tint[0] - bg[0]) * k, c[1] + (tint[1] - bg[1]) * k, c[2] + (tint[2] - bg[2]) * k];
                    }
                    c
                }
            };
            out.push(pack(c, if kind == Kind::Solid { 0.0 } else { dither(x, y) }));
        }
    }
    out
}

/// ¿Es un fondo con movimiento? (el color liso no se mueve).
pub fn is_animatable(kind: Kind) -> bool {
    kind != Kind::Solid
}

/// Genera el fondo y lo guarda como PNG en la carpeta de generados. Devuelve la ruta.
pub fn generate(kind: Kind, theme_name: &str, bg: u32, accent: u32) -> Result<PathBuf, String> {
    let dir = generated_dir().ok_or("no se encontró la carpeta de datos")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (w, h) = (1920usize, 1080usize);
    let mut canvas = crate::gfx::Canvas::new(w as i32, h as i32).ok_or("no se pudo crear la imagen")?;
    canvas.fill_from(&render(kind, w, h, bg, accent));
    let name = crate::theme::file_name(&format!("{} - {}", theme_name, kind.label()));
    let path = dir.join(format!("{name}.png"));
    if canvas.save_png(&path.to_string_lossy()) {
        Ok(path)
    } else {
        Err(format!("no se pudo guardar la imagen en {}", dir.display()))
    }
}

// ---------- escena samurái como imagen fija ----------

/// Resolución de la imagen de bloqueo: la del monitor principal, entre 1280x720 y 3840x2160.
pub fn still_resolution(monitor: Option<(i32, i32)>) -> (i32, i32) {
    let (w, h) = monitor.unwrap_or((1920, 1080));
    (w.clamp(1280, 3840), h.clamp(720, 2160))
}

/// Un fotograma de la escena samurái (`samurai.rs`) a la resolución dada: se calcula en pequeño y se amplía sin
/// suavizar, igual que el fondo animado, para que conserve el aspecto pixel art.
pub fn render_samurai_still(w: i32, h: i32, bg: u32, accent: u32) -> Option<crate::gfx::Canvas> {
    let (sw, sh) = crate::samurai::scene_size(w, h);
    let mut small = crate::gfx::Canvas::new(sw, sh)?;
    // t = 6 s: estrellas, niebla y pétalos bien repartidos
    small.fill_from(&crate::samurai::render(sw as usize, sh as usize, 6.0, bg, accent));
    let mut big = crate::gfx::Canvas::new(w, h)?;
    big.draw_scaled(&small, false);
    Some(big)
}

/// Guarda la escena samurái como PNG en la carpeta de generados. Devuelve la ruta.
pub fn generate_samurai(bg: u32, accent: u32) -> Result<PathBuf, String> {
    let dir = generated_dir().ok_or("no se encontró la carpeta de datos")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let monitor = connected_monitors().first().map(|m| (m.1, m.2));
    let (w, h) = still_resolution(monitor);
    let canvas = render_samurai_still(w, h, bg, accent).ok_or("no se pudo crear la imagen")?;
    let path = dir.join(format!("{}.png", crate::theme::file_name("Bushidō - Samurái")));
    if canvas.save_png(&path.to_string_lossy()) {
        Ok(path)
    } else {
        Err(format!("no se pudo guardar la imagen en {}", dir.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A mano: `cargo test dump_lock_image -- --ignored` escribe `%TEMP%\lock_samurai.png`.
    #[test]
    #[ignore]
    fn dump_lock_image() {
        let c = render_samurai_still(1920, 1080, 0x0d0b0e, 0x2e10c8).unwrap();
        assert!(c.save_png(&std::env::temp_dir().join("lock_samurai.png").to_string_lossy()));
    }

    #[test]
    fn the_lock_image_resolution_is_clamped() {
        assert_eq!(still_resolution(None), (1920, 1080));
        assert_eq!(still_resolution(Some((2560, 1440))), (2560, 1440));
        assert_eq!(still_resolution(Some((800, 600))), (1280, 720));
        assert_eq!(still_resolution(Some((7680, 4320))), (3840, 2160));
    }

    #[test]
    fn the_samurai_still_has_the_right_size_and_real_content() {
        let c = render_samurai_still(1280, 720, 0x0d0b0e, 0x2e10c8).expect("lienzo");
        assert_eq!((c.w, c.h), (1280, 720));
        let px = c.pixels();
        assert!(px.iter().all(|p| p >> 24 == 0xff), "opaca");
        let distinct: std::collections::HashSet<u32> = px.iter().copied().collect();
        assert!(distinct.len() > 500, "una escena, no un color liso: {}", distinct.len());
        // ampliada sin suavizar: bloques de 4x4 idénticos (1280/4 = 320 columnas del lienzo pequeño)
        assert_eq!(px[0], px[1]);
        assert_eq!(px[0], px[3]);
        assert_eq!(px[3], px[3 + 1280 * 3]);
    }

    const BG: u32 = 0x201e1e; // #1e1e20
    const ACCENT: u32 = 0xff840a; // #0a84ff

    fn px(v: &[u32], w: usize, x: usize, y: usize) -> (u32, u32, u32) {
        let p = v[y * w + x];
        ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff)
    }

    #[test]
    fn gradient_goes_from_the_background_toward_the_accent() {
        let (w, h) = (64, 36);
        let v = render(Kind::Gradient, w, h, BG, ACCENT);
        assert_eq!(v.len(), w * h);
        assert!(v.iter().all(|p| p >> 24 == 0xff));
        let (tl, br) = (px(&v, w, 0, 0), px(&v, w, w - 1, h - 1));
        assert!(tl.0.abs_diff(0x1e) <= 1 && tl.2.abs_diff(0x20) <= 1, "esquina inicial {tl:?}");
        assert!(br.2 > tl.2 + 40, "la esquina final debería tirar hacia el azul: {br:?}");
    }

    #[test]
    fn solid_is_flat() {
        let v = render(Kind::Solid, 8, 8, BG, ACCENT);
        assert!(v.iter().all(|&p| p == v[0]));
        assert_eq!(v[0], 0xFF1E1E20);
    }

    #[test]
    fn glow_and_aurora_light_up_the_corner_areas() {
        let (w, h) = (96, 54);
        let glow = render(Kind::Glow, w, h, BG, ACCENT);
        assert!(px(&glow, w, 24, 10).2 > px(&glow, w, w - 1, h - 1).2 + 20);
        let aurora = render(Kind::Aurora, w, h, BG, ACCENT);
        let distinct: std::collections::HashSet<u32> = aurora.iter().copied().collect();
        assert!(distinct.len() > 200, "la aurora debería tener muchos matices");
    }

    #[test]
    fn rendering_is_deterministic() {
        assert_eq!(render(Kind::Aurora, 32, 18, BG, ACCENT), render(Kind::Aurora, 32, 18, BG, ACCENT));
    }

    #[test]
    fn hue_rotation_keeps_brightness_and_greys() {
        let c = hue_rotate([1.0, 0.0, 0.0], 120.0);
        assert!(c[0] < 0.01 && c[1] > 0.99 && c[2] < 0.01, "{c:?}");
        assert_eq!(hue_rotate([0.5, 0.5, 0.5], 90.0), [0.5, 0.5, 0.5]);
        let full = hue_rotate([0.2, 0.4, 0.9], 360.0);
        assert!((full[0] - 0.2).abs() < 1e-3 && (full[2] - 0.9).abs() < 1e-3);
    }

    #[test]
    fn fit_and_kind_keys_round_trip() {
        for f in Fit::ALL {
            assert_eq!(Fit::parse(f.key()), Some(f));
            assert_eq!(Fit::from_win(f.to_win()), Some(f));
        }
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.key()), Some(k));
        }
        assert_eq!(Fit::parse("nada"), None);
    }

    #[test]
    fn file_uris_become_windows_paths() {
        assert_eq!(file_uri_to_path("file:///C:/Windows/Web/img100.jpg").as_deref(), Some(r"C:\Windows\Web\img100.jpg"));
        assert_eq!(file_uri_to_path("file:///C:/Users/x/Mi%20foto%C3%B1.jpg").as_deref(), Some("C:\\Users\\x\\Mi fotoñ.jpg"));
        assert_eq!(file_uri_to_path("https://x/y.jpg"), None);
        assert_eq!(file_uri_to_path("file:///"), None);
        assert_eq!(file_uri_to_path("file:///C:/a%2"), Some(r"C:\a%2".to_string())); // escape incompleto: se deja tal cual
        assert!(file_uri_to_path("file:///C:/%zz%ñ/x").is_some()); // basura tras un %: no debe entrar en pánico
    }

    #[test]
    fn random_index_stays_in_range() {
        assert_eq!(random_index(0), 0);
        assert!((0..200).all(|_| random_index(7) < 7));
    }

    #[test]
    fn image_extensions() {
        assert!(is_image(Path::new(r"C:\x\foto.JPG")) && is_image(Path::new("a.png")));
        assert!(!is_image(Path::new("a.txt")) && !is_image(Path::new("sinextension")));
    }
}
