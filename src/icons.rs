//! Iconos reales de aplicaciones vía el shell (`IShellItemImageFactory`), listos para dibujar con GDI+.
//! Requiere COM inicializado en el hilo que llama.

use std::ffi::c_void;
use std::ptr::null_mut;

use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Graphics::GdiPlus::{
    CombineModeReplace, GdipCloneBitmapAreaI, GdipCreateBitmapFromFile, GdipCreateBitmapFromScan0, GdipCreatePath,
    GdipAddPathArc, GdipClosePathFigure, GdipDeleteGraphics, GdipDeletePath, GdipDisposeImage, GdipDrawImageRectRectI,
    GdipGetImageGraphicsContext, GdipGetImageHeight, GdipGetImageWidth, GdipSetClipPath, GdipSetInterpolationMode,
    GdipSetSmoothingMode, FillModeAlternate, GpBitmap, GpImage, InterpolationModeHighQualityBicubic, SmoothingModeAntiAlias, Status,
    UnitPixel,
};
use windows::Win32::UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};

/// Estilos de los iconos de las aplicaciones en el dock: (clave, nombre, descripción).
pub const APP_STYLES: &[(&str, &str, &str)] = &[
    ("original", "Originales", "Los iconos de cada aplicación, sin cambios"),
    ("tile", "En ficha", "Cada icono sobre una ficha redondeada del color del tema"),
    ("mono", "Monocromo", "Iconos en tonos del color del texto del tema"),
    ("accent", "Acento", "Iconos en tonos del color de acento del tema"),
];

/// Resolución con la que se piden los iconos: da margen para la magnificación del dock.
const REQUEST_SIZE: i32 = 128;

/// PixelFormat32bppPARGB de GDI+.
const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;

pub struct Icon {
    pub w: i32,
    pub h: i32,
    /// ARGB premultiplicado; el bitmap de GDI+ apunta a este búfer.
    pixels: Vec<u32>,
    pub(crate) bitmap: *mut GpBitmap,
}

impl Icon {
    /// Guarda el icono como PNG (para la caché de miniaturas). `true` si se guardó.
    pub fn save_png(&self, path: &str) -> bool {
        use windows::Win32::Graphics::GdiPlus::GdipSaveImageToFile;
        let png = windows::core::GUID::from_u128(0x557cf406_1a04_11d3_9a73_0000f81ef32e);
        let wide = HSTRING::from(path);
        unsafe {
            GdipSaveImageToFile(self.bitmap as *mut GpImage, windows::core::PCWSTR(wide.as_ptr()), &png, std::ptr::null()) == Status(0)
        }
    }

    /// Píxeles ARGB premultiplicados (fila a fila).
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// Icono a partir de píxeles ARGB premultiplicados (`w * h` elementos).
    pub fn from_pixels(w: i32, h: i32, pixels: Vec<u32>) -> Option<Icon> {
        if w <= 0 || h <= 0 || pixels.len() != (w * h) as usize {
            return None;
        }
        crate::gfx::start();
        let mut bitmap = null_mut();
        unsafe {
            GdipCreateBitmapFromScan0(w, h, w * 4, PIXEL_FORMAT_32BPP_PARGB, Some(pixels.as_ptr() as *const u8), &mut bitmap);
        }
        (!bitmap.is_null()).then_some(Icon { w, h, pixels, bitmap })
    }

    /// Copia del icono en tonos del color `tint` (COLORREF): se conserva el relieve (la luminosidad) y la forma.
    pub fn tinted(&self, tint: u32) -> Option<Icon> {
        if self.pixels.is_empty() {
            return None;
        }
        Icon::from_pixels(self.w, self.h, tint_pixels(&self.pixels, tint))
    }
}

/// Recolorea píxeles ARGB premultiplicados: el alfa se mantiene y el color pasa a ser `tint` modulado por la
/// luminosidad original.
pub fn tint_pixels(pixels: &[u32], tint: u32) -> Vec<u32> {
    let (tr, tg, tb) = ((tint & 0xff) as f32, ((tint >> 8) & 0xff) as f32, ((tint >> 16) & 0xff) as f32);
    pixels
        .iter()
        .map(|&p| {
            let a = p >> 24;
            if a == 0 {
                return 0;
            }
            let un = |v: u32| (v as f32 * 255.0 / a as f32).min(255.0);
            let (r, g, b) = (un((p >> 16) & 0xff), un((p >> 8) & 0xff), un(p & 0xff));
            let lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
            let k = (0.35 + 0.65 * lum) * a as f32 / 255.0;
            let ch = |t: f32| (t * k).round().min(255.0) as u32;
            a << 24 | ch(tr) << 16 | ch(tg) << 8 | ch(tb)
        })
        .collect()
}

/// Carpeta donde el usuario deja iconos propios (`<nombre de la app>.png` o `.ico`).
pub fn custom_dir() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join(r"hypr-win\icons"))
}

/// Archivo `.png`/`.ico`/`.jpg` de `dir` cuyo nombre (sin extensión, sin distinguir mayúsculas) es uno de `names`.
pub fn find_custom(dir: &std::path::Path, names: &[&str]) -> Option<std::path::PathBuf> {
    let wanted: Vec<String> = names.iter().map(|n| n.trim().to_lowercase()).filter(|n| !n.is_empty()).collect();
    for entry in std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()) {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "ico" | "jpg" | "jpeg") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).map(str::to_lowercase).unwrap_or_default();
        if wanted.contains(&stem) {
            return Some(path);
        }
    }
    None
}

/// Icono propio del usuario para una app, por su nombre o por el de su ejecutable.
pub fn custom_for(name: &str, exe: Option<&str>) -> Option<Icon> {
    let dir = custom_dir()?;
    let stem = exe.and_then(|e| std::path::Path::new(e).file_stem()).and_then(|s| s.to_str()).unwrap_or("");
    let path = find_custom(&dir, &[name, stem])?;
    from_image_file(&path.to_string_lossy())
}

impl Drop for Icon {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.bitmap as *mut GpImage);
        }
    }
}

/// Icono de una entrada de `shell:AppsFolder` (apps de escritorio y de la Tienda).
pub fn from_apps_folder(id: &str) -> Option<Icon> {
    from_parsing_name(&format!("shell:AppsFolder\\{id}"))
}

/// Icono desde un archivo de imagen (`.png`, `.ico`...). Se copia a memoria para no dejar el archivo bloqueado.
pub fn from_image_file(path: &str) -> Option<Icon> {
    crate::gfx::start();
    let wide = HSTRING::from(path);
    unsafe {
        let mut src = null_mut();
        if GdipCreateBitmapFromFile(windows::core::PCWSTR(wide.as_ptr()), &mut src) != Status(0) || src.is_null() {
            return None;
        }
        let (mut w, mut h) = (0u32, 0u32);
        GdipGetImageWidth(src as *mut GpImage, &mut w);
        GdipGetImageHeight(src as *mut GpImage, &mut h);
        let mut dst = null_mut();
        let st = GdipCloneBitmapAreaI(0, 0, w as i32, h as i32, PIXEL_FORMAT_32BPP_PARGB, src, &mut dst);
        GdipDisposeImage(src as *mut GpImage);
        if st != Status(0) || dst.is_null() || w == 0 || h == 0 {
            return None;
        }
        Some(Icon { w: w as i32, h: h as i32, pixels: Vec::new(), bitmap: dst })
    }
}

/// Miniatura cuadrada de `size` px (recorte central, esquinas redondeadas) de una imagen de cualquier tamaño.
/// Se escala al leerla: no se guarda la imagen completa en memoria.
pub fn thumbnail_from_file(path: &str, size: i32) -> Option<Icon> {
    crate::gfx::start();
    let wide = HSTRING::from(path);
    unsafe {
        let mut src = null_mut();
        if GdipCreateBitmapFromFile(windows::core::PCWSTR(wide.as_ptr()), &mut src) != Status(0) || src.is_null() {
            return None;
        }
        let (mut w, mut h) = (0u32, 0u32);
        GdipGetImageWidth(src as *mut GpImage, &mut w);
        GdipGetImageHeight(src as *mut GpImage, &mut h);
        let side = w.min(h) as i32;
        let (sx, sy) = ((w as i32 - side) / 2, (h as i32 - side) / 2);
        let mut dst = null_mut();
        GdipCreateBitmapFromScan0(size, size, 0, PIXEL_FORMAT_32BPP_PARGB, None, &mut dst);
        if dst.is_null() || side <= 0 {
            GdipDisposeImage(src as *mut GpImage);
            return None;
        }
        let mut g = null_mut();
        GdipGetImageGraphicsContext(dst as *mut GpImage, &mut g);
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
        GdipSetInterpolationMode(g, InterpolationModeHighQualityBicubic);
        // esquinas redondeadas
        let (r, d) = (size as f32 * 0.22, size as f32 * 0.44);
        let mut path = null_mut();
        GdipCreatePath(FillModeAlternate, &mut path);
        let s = size as f32;
        GdipAddPathArc(path, 0.0, 0.0, d, d, 180.0, 90.0);
        GdipAddPathArc(path, s - d, 0.0, d, d, 270.0, 90.0);
        GdipAddPathArc(path, s - d, s - d, d, d, 0.0, 90.0);
        GdipAddPathArc(path, 0.0, s - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(path);
        let _ = r;
        GdipSetClipPath(g, path, CombineModeReplace);
        GdipDeletePath(path);
        GdipDrawImageRectRectI(g, src as *mut GpImage, 0, 0, size, size, sx, sy, side, side, UnitPixel, std::ptr::null(), 0, std::ptr::null_mut());
        GdipDeleteGraphics(g);
        GdipDisposeImage(src as *mut GpImage);
        Some(Icon { w: size, h: size, pixels: Vec::new(), bitmap: dst })
    }
}

/// Icono de un archivo (por ejemplo, un `.exe`).
pub fn from_path(path: &str) -> Option<Icon> {
    from_parsing_name(path)
}

fn from_parsing_name(name: &str) -> Option<Icon> {
    crate::gfx::start();
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(name), None).ok()?;
        let factory: IShellItemImageFactory = windows::core::Interface::cast(&item).ok()?;
        let hbm = factory.GetImage(SIZE { cx: REQUEST_SIZE, cy: REQUEST_SIZE }, SIIGBF_ICONONLY).ok()?;
        let icon = bitmap_to_icon(hbm);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        icon
    }
}

unsafe fn bitmap_to_icon(hbm: windows::Win32::Graphics::Gdi::HBITMAP) -> Option<Icon> {
    let mut bm = BITMAP::default();
    if GetObjectW(HGDIOBJ(hbm.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut pixels = vec![0u32; (w * h) as usize];
    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // de arriba abajo
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let dc = GetDC(None);
    let lines = GetDIBits(dc, hbm, 0, h as u32, Some(pixels.as_mut_ptr() as *mut c_void), &mut bmi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    if lines == 0 {
        return None;
    }
    to_premultiplied(&mut pixels);

    let mut bitmap = null_mut();
    GdipCreateBitmapFromScan0(w, h, w * 4, PIXEL_FORMAT_32BPP_PARGB, Some(pixels.as_ptr() as *const u8), &mut bitmap);
    if bitmap.is_null() {
        return None;
    }
    Some(Icon { w, h, pixels, bitmap })
}

/// Normaliza el búfer a ARGB premultiplicado: sin alfa -> opaco; alfa recto -> se premultiplica.
pub fn to_premultiplied(pixels: &mut [u32]) {
    if pixels.iter().all(|p| p >> 24 == 0) {
        for p in pixels.iter_mut() {
            *p |= 0xFF00_0000; // el icono no trae alfa
        }
        return;
    }
    let straight = pixels.iter().any(|&p| {
        let a = p >> 24;
        (p >> 16 & 0xff) > a || (p >> 8 & 0xff) > a || (p & 0xff) > a
    });
    if straight {
        for p in pixels.iter_mut() {
            let a = *p >> 24;
            let m = |c: u32| (c * a + 127) / 255;
            *p = a << 24 | m(*p >> 16 & 0xff) << 16 | m(*p >> 8 & 0xff) << 8 | m(*p & 0xff);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_when_no_alpha() {
        let mut p = [0x0000_00ffu32, 0x00ff_0000];
        to_premultiplied(&mut p);
        assert_eq!(p, [0xff00_00ff, 0xffff_0000]);
    }

    #[test]
    fn premultiplies_straight_alpha() {
        let mut p = [0x80ff_ffffu32, 0xff10_2030];
        to_premultiplied(&mut p);
        assert_eq!(p[0], 0x8080_8080);
        assert_eq!(p[1], 0xff10_2030);
    }

    #[test]
    fn keeps_premultiplied() {
        let mut p = [0x8080_8080u32, 0xff10_2030];
        to_premultiplied(&mut p);
        assert_eq!(p, [0x8080_8080, 0xff10_2030]);
    }

    #[test]
    fn tinting_keeps_alpha_and_shape_and_recolors() {
        // rojo opaco, blanco opaco, mitad transparente, vacío (premultiplicado)
        let px = [0xFFFF0000, 0xFFFFFFFF, 0x80808080, 0];
        let out = tint_pixels(&px, 0x00FF8000); // COLORREF: R=0x00, G=0x80, B=0xFF
        assert_eq!(out[3], 0);
        assert_eq!(out[1] >> 24, 0xFF);
        assert_eq!(out[2] >> 24, 0x80);
        // el blanco toma el color completo del tinte y el rojo (menos luminoso) uno más apagado
        let blue = |p: u32| p & 0xff;
        assert_eq!(blue(out[1]), 0xFF);
        assert!(blue(out[0]) < blue(out[1]));
        // premultiplicado: ningún canal supera al alfa
        assert!(out.iter().all(|&p| (p >> 16 & 0xff) <= p >> 24 && (p >> 8 & 0xff) <= p >> 24 && (p & 0xff) <= p >> 24));
    }

    #[test]
    fn custom_icons_are_found_by_app_or_executable_name() {
        let dir = std::env::temp_dir().join(format!("hypr-win-icons-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Visual Studio Code.PNG"), b"x").unwrap();
        std::fs::write(dir.join("chrome.ico"), b"x").unwrap();
        std::fs::write(dir.join("notas.txt"), b"x").unwrap();
        assert!(find_custom(&dir, &["visual studio code", "code"]).is_some());
        assert!(find_custom(&dir, &["Google Chrome", "CHROME"]).is_some());
        assert!(find_custom(&dir, &["notas"]).is_none(), "solo imágenes");
        assert!(find_custom(&dir, &["firefox", ""]).is_none());
        assert!(find_custom(&dir.join("no-existe"), &["chrome"]).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn icons_can_be_built_from_pixels() {
        assert!(Icon::from_pixels(2, 2, vec![0xFF00FF00; 4]).is_some());
        assert!(Icon::from_pixels(2, 2, vec![0; 3]).is_none());
        let i = Icon::from_pixels(2, 2, vec![0xFF0000FF; 4]).unwrap();
        assert!(i.tinted(0x00FFFFFF).is_some());
    }

    #[test]
    fn every_style_key_is_unique() {
        for (i, (k, _, _)) in APP_STYLES.iter().enumerate() {
            assert!(APP_STYLES.iter().skip(i + 1).all(|(o, _, _)| o != k));
        }
    }
}
