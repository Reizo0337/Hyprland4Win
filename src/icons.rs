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
use windows::Win32::Graphics::GdiPlus::{GdipCreateBitmapFromScan0, GdipDisposeImage, GpBitmap, GpImage};
use windows::Win32::UI::Shell::{IShellItem, IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_ICONONLY};

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
    /// Píxeles ARGB premultiplicados (fila a fila).
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }
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
    use super::to_premultiplied;

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
}
