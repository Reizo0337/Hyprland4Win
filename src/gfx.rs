//! Dibujo con GDI+ sobre un búfer ARGB premultiplicado: formas suavizadas, texto con alfa correcto y
//! presentación con `UpdateLayeredWindow` (transparencia por píxel). Todo ocurre en nuestras ventanas.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Graphics::GdiPlus::{
    FillModeAlternate, FontStyleBold, FontStyleRegular, GdipAddPathArc, GdipAddPathRectangle, GdipClosePathFigure,
    GdipCreateBitmapFromScan0, GdipCreateFont, GdipCreateFontFamilyFromName, GdipCreatePath, GdipCreatePen1,
    GdipCreateSolidFill, GdipCreateStringFormat, GdipDeleteBrush, GdipDeleteFont, GdipDeleteFontFamily,
    GdipDeleteGraphics, GdipDeletePath, GdipDeletePen, GdipDeleteStringFormat, GdipDisposeImage, GdipDrawEllipse,
    GdipDrawLine, GdipDrawPath, GdipDrawString, GdipFillEllipse, GdipFillPath, GdipGetImageGraphicsContext,
    GdipMeasureString, GdipSetPenEndCap, GdipSetPenStartCap, GdipSetInterpolationMode, GdipSetPixelOffsetMode, GdipSetSmoothingMode, GdipDrawArc, GdipDrawImageRect, GdipFillPolygon, InterpolationModeHighQualityBicubic, PointF,
    GdipSetStringFormatAlign, GdipSetStringFormatFlags, GdipSetStringFormatLineAlign,
    GdipSetStringFormatTrimming, GdipSetTextRenderingHint, GdiplusStartup, GdiplusStartupInput, GpBitmap, GpBrush,
    GpFont, GpFontFamily, GpGraphics, GpImage, GpPath, GpStringFormat, LineCapRound, PixelOffsetModeHalf, RectF,
    SmoothingModeAntiAlias, Status, StringAlignmentCenter, StringAlignmentFar, StringAlignmentNear,
    StringFormatFlagsNoWrap, StringTrimmingEllipsisCharacter, TextRenderingHintAntiAlias, UnitPixel,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};

use crate::config::{self, Bar};

/// Factor de escala del sistema (1.0 = 96 DPI).
pub fn dpi() -> f32 {
    unsafe { GetDpiForSystem() as f32 / 96.0 }
}

/// `v` px a 96 DPI -> px reales.
pub fn px(v: f32) -> f32 {
    v * dpi()
}

/// COLORREF (0x00BBGGRR) + alfa -> ARGB de GDI+.
pub fn argb(c: u32, alpha: u8) -> u32 {
    let (r, g, b) = (c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff);
    (alpha as u32) << 24 | r << 16 | g << 8 | b
}

/// Colores y fuente compartidos por la barra y el lanzador.
#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: u32,
    pub fg: u32,
    pub dim: u32,
    pub accent: u32,
    pub accent_text: u32,
    pub font: String,
    pub font_pt: f32,
}

impl Theme {
    pub fn from_cfg(cfg: &Bar) -> Result<Theme, String> {
        let c = |s: &str| config::parse_color(s);
        Ok(Theme {
            bg: c(&cfg.background)?,
            fg: c(&cfg.foreground)?,
            dim: c(&cfg.dim)?,
            accent: c(&cfg.accent)?,
            accent_text: c(&cfg.accent_text)?,
            font: cfg.font.clone(),
            font_pt: cfg.font_size as f32,
        })
    }

    /// Tamaño base de la fuente en px reales.
    pub fn base_px(&self) -> f32 {
        self.font_pt * 96.0 / 72.0 * dpi()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl FRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn grow(&self, d: f32) -> Self {
        Self::new(self.x - d, self.y - d, self.w + 2.0 * d, self.h + 2.0 * d)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// PixelFormat32bppPARGB de GDI+.
const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;

static TOKEN: OnceLock<usize> = OnceLock::new();

pub(crate) fn start() {
    TOKEN.get_or_init(|| unsafe {
        let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
        let mut token = 0usize;
        GdiplusStartup(&mut token, &input, null_mut());
        token
    });
}

pub struct Font {
    family: *mut GpFontFamily,
    font: *mut GpFont,
}

impl Font {
    /// Crea la fuente `name` (con respaldo a Segoe UI) de `size` px.
    pub fn new(name: &str, size: f32, bold: bool) -> Option<Font> {
        start();
        unsafe {
            for candidate in [name, "Segoe UI"] {
                let mut family = null_mut();
                let n = HSTRING::from(candidate);
                if GdipCreateFontFamilyFromName(PCWSTR(n.as_ptr()), null_mut(), &mut family) != Status(0) {
                    continue;
                }
                let mut font = null_mut();
                let style = if bold { FontStyleBold.0 } else { FontStyleRegular.0 };
                if GdipCreateFont(family, size, style, UnitPixel, &mut font) == Status(0) {
                    return Some(Font { family, font });
                }
                GdipDeleteFontFamily(family);
            }
        }
        None
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteFont(self.font);
            GdipDeleteFontFamily(self.family);
        }
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    hdc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    bitmap: *mut GpBitmap,
    g: *mut GpGraphics,
    fmt: [*mut GpStringFormat; 3],
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Option<Canvas> {
        if w <= 0 || h <= 0 {
            return None;
        }
        start();
        unsafe {
            let screen = GetDC(None);
            let hdc = CreateCompatibleDC(Some(screen));
            ReleaseDC(None, screen);
            let bi = BITMAPINFO {
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
            let mut bits: *mut c_void = null_mut();
            let bmp = CreateDIBSection(Some(hdc), &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
            let old = SelectObject(hdc, bmp.into());

            // GDI+ dibuja directamente en la memoria del DIB, en formato premultiplicado
            let mut bitmap = null_mut();
            GdipCreateBitmapFromScan0(w, h, w * 4, PIXEL_FORMAT_32BPP_PARGB, Some(bits as *const u8), &mut bitmap);
            let mut g = null_mut();
            GdipGetImageGraphicsContext(bitmap as *mut GpImage, &mut g);
            GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            GdipSetTextRenderingHint(g, TextRenderingHintAntiAlias);
            GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
            GdipSetInterpolationMode(g, InterpolationModeHighQualityBicubic);

            let mut fmt = [null_mut(); 3];
            for (f, align) in fmt.iter_mut().zip([StringAlignmentNear, StringAlignmentCenter, StringAlignmentFar]) {
                GdipCreateStringFormat(0, 0, f);
                GdipSetStringFormatAlign(*f, align);
                GdipSetStringFormatLineAlign(*f, StringAlignmentCenter);
                GdipSetStringFormatTrimming(*f, StringTrimmingEllipsisCharacter);
                GdipSetStringFormatFlags(*f, StringFormatFlagsNoWrap.0);
            }
            Some(Canvas { w, h, hdc, bmp, old, bits: bits as *mut u32, bitmap, g, fmt })
        }
    }

    pub fn clear(&mut self) {
        unsafe { std::ptr::write_bytes(self.bits, 0, (self.w * self.h) as usize) };
    }

    /// Píxeles premultiplicados ARGB (fila a fila, de arriba abajo).
    pub fn pixels(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.bits, (self.w * self.h) as usize) }
    }

    unsafe fn rounded_path(r: FRect, radius: f32) -> *mut GpPath {
        let mut path = null_mut();
        GdipCreatePath(FillModeAlternate, &mut path);
        let rad = radius.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
        if rad < 0.5 {
            GdipAddPathRectangle(path, r.x, r.y, r.w, r.h);
        } else {
            let d = rad * 2.0;
            GdipAddPathArc(path, r.x, r.y, d, d, 180.0, 90.0);
            GdipAddPathArc(path, r.x + r.w - d, r.y, d, d, 270.0, 90.0);
            GdipAddPathArc(path, r.x + r.w - d, r.y + r.h - d, d, d, 0.0, 90.0);
            GdipAddPathArc(path, r.x, r.y + r.h - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(path);
        }
        path
    }

    pub fn fill_round(&mut self, r: FRect, radius: f32, color: u32) {
        unsafe {
            let path = Self::rounded_path(r, radius);
            let mut brush = null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipFillPath(self.g, brush as *mut GpBrush, path);
            GdipDeleteBrush(brush as *mut GpBrush);
            GdipDeletePath(path);
        }
    }

    pub fn stroke_round(&mut self, r: FRect, radius: f32, width: f32, color: u32) {
        unsafe {
            let path = Self::rounded_path(r, radius);
            let mut pen = null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipDrawPath(self.g, pen, path);
            GdipDeletePen(pen);
            GdipDeletePath(path);
        }
    }

    pub fn fill_ellipse(&mut self, r: FRect, color: u32) {
        unsafe {
            let mut brush = null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipFillEllipse(self.g, brush as *mut GpBrush, r.x, r.y, r.w, r.h);
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    pub fn stroke_ellipse(&mut self, r: FRect, width: f32, color: u32) {
        unsafe {
            let mut pen = null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipDrawEllipse(self.g, pen, r.x, r.y, r.w, r.h);
            GdipDeletePen(pen);
        }
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: u32) {
        unsafe {
            let mut pen = null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipSetPenStartCap(pen, LineCapRound);
            GdipSetPenEndCap(pen, LineCapRound);
            GdipDrawLine(self.g, pen, x1, y1, x2, y2);
            GdipDeletePen(pen);
        }
    }

    /// Texto en una línea, centrado en vertical dentro de `r`, con puntos suspensivos si no cabe.
    pub fn text(&mut self, font: &Font, s: &str, r: FRect, color: u32, align: Align) {
        if s.is_empty() {
            return;
        }
        let wide: Vec<u16> = s.encode_utf16().collect();
        let rect = RectF { X: r.x, Y: r.y, Width: r.w, Height: r.h };
        let idx = match align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        unsafe {
            let mut brush = null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipDrawString(
                self.g,
                PCWSTR(wide.as_ptr()),
                wide.len() as i32,
                font.font,
                &rect,
                self.fmt[idx],
                brush as *mut GpBrush,
            );
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    /// Tarjeta flotante: sombra suave, fondo translúcido y borde fino (colores como COLORREF).
    pub fn card(&mut self, card: FRect, radius: f32, bg: u32, bg_alpha: u8, border: u32) {
        let u = px(1.0);
        for i in 0..16 {
            let grow = i as f32 * u;
            let r = FRect::new(card.x - grow, card.y - grow + 8.0 * u, card.w + 2.0 * grow, card.h + 2.0 * grow);
            self.fill_round(r, radius + grow, argb(0x000000, 3));
        }
        self.fill_round(card, radius, argb(bg, bg_alpha));
        self.stroke_round(card.grow(-0.5), radius, 1.0, argb(border, 30));
    }

    /// Arco del elipse inscrito en `bbox` (ángulos en grados, 0 = derecha, positivo = sentido horario).
    pub fn arc(&mut self, bbox: FRect, start: f32, sweep: f32, width: f32, color: u32) {
        unsafe {
            let mut pen = null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipSetPenStartCap(pen, LineCapRound);
            GdipSetPenEndCap(pen, LineCapRound);
            GdipDrawArc(self.g, pen, bbox.x, bbox.y, bbox.w, bbox.h, start, sweep);
            GdipDeletePen(pen);
        }
    }

    pub fn fill_polygon(&mut self, pts: &[(f32, f32)], color: u32) {
        let v: Vec<PointF> = pts.iter().map(|&(x, y)| PointF { X: x, Y: y }).collect();
        unsafe {
            let mut brush = null_mut();
            GdipCreateSolidFill(color, &mut brush);
            GdipFillPolygon(self.g, brush as *mut GpBrush, v.as_ptr(), v.len() as i32, FillModeAlternate);
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    /// Dibuja un icono escalado a `r` (interpolación bicúbica de alta calidad).
    pub fn draw_icon(&mut self, icon: &crate::icons::Icon, r: FRect) {
        unsafe {
            GdipDrawImageRect(self.g, icon.bitmap as *mut windows::Win32::Graphics::GdiPlus::GpImage, r.x, r.y, r.w, r.h);
        }
    }

    /// Ancho en px del texto.
    pub fn measure(&mut self, font: &Font, s: &str) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        let wide: Vec<u16> = s.encode_utf16().collect();
        let layout = RectF { X: 0.0, Y: 0.0, Width: 10_000.0, Height: 200.0 };
        let mut bounds = RectF::default();
        unsafe {
            GdipMeasureString(
                self.g,
                PCWSTR(wide.as_ptr()),
                wide.len() as i32,
                font.font,
                &layout,
                self.fmt[0],
                &mut bounds,
                null_mut(),
                null_mut(),
            );
        }
        bounds.Width
    }

    /// Muestra el búfer en una ventana `WS_EX_LAYERED` (con su tamaño; en `pos` si se indica).
    pub fn present(&self, hwnd: HWND, pos: Option<(i32, i32)>) {
        let size = SIZE { cx: self.w, cy: self.h };
        let dst = pos.map(|(x, y)| POINT { x, y });
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                dst.as_ref().map(|p| p as *const POINT),
                Some(&size),
                Some(self.hdc),
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            for f in self.fmt {
                GdipDeleteStringFormat(f);
            }
            GdipDeleteGraphics(self.g);
            GdipDisposeImage(self.bitmap as *mut GpImage);
            SelectObject(self.hdc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.hdc);
        }
    }
}
