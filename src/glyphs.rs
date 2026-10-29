//! Glifos vectoriales de estado (Wi-Fi, volumen, batería), dibujados con GDI+ en un cuadrado de lado `s`.
//! `color` es un COLORREF; el alfa se ajusta aquí para atenuar las partes apagadas.

use crate::gfx::{argb, Canvas, FRect};

/// Wi-Fi: punto + 3 arcos. `lit` = arcos encendidos (0-3); `off` = radio apagada (se tacha).
pub fn wifi(c: &mut Canvas, x: f32, y: f32, s: f32, lit: u8, off: bool, color: u32) {
    let (ox, oy) = (x + s / 2.0, y + s * 0.88);
    let w = (s * 0.12).max(1.2);
    for k in 0..3u8 {
        let r = s * (0.32 + 0.26 * k as f32);
        let a = if !off && k < lit { 255 } else { 70 };
        c.arc(FRect::new(ox - r, oy - r, 2.0 * r, 2.0 * r), 227.0, 86.0, w, argb(color, a));
    }
    let d = s * 0.17;
    c.fill_ellipse(FRect::new(ox - d / 2.0, oy - d / 2.0, d, d), argb(color, if off { 70 } else { 255 }));
    if off {
        c.line(x + s * 0.1, y + s * 0.1, x + s * 0.9, y + s * 0.9, w, argb(color, 255));
    }
}

/// Altavoz con 0-2 ondas según el nivel; tachado si está en silencio.
pub fn speaker(c: &mut Canvas, x: f32, y: f32, s: f32, level: f32, muted: bool, color: u32) {
    let p = |u: f32, v: f32| (x + u * s, y + v * s);
    c.fill_polygon(&[p(0.04, 0.38), p(0.26, 0.38), p(0.5, 0.16), p(0.5, 0.84), p(0.26, 0.62), p(0.04, 0.62)], argb(color, 255));
    let w = (s * 0.11).max(1.2);
    if muted || level <= 0.0 {
        let (x1, y1) = p(0.64, 0.36);
        let (x2, y2) = p(0.92, 0.64);
        c.line(x1, y1, x2, y2, w, argb(color, 255));
        c.line(x1, y2, x2, y1, w, argb(color, 255));
        return;
    }
    let waves = if level > 0.5 { 2 } else { 1 };
    let (cx, cy) = p(0.5, 0.5);
    for k in 0..waves {
        let r = s * (0.24 + 0.2 * k as f32);
        c.arc(FRect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), -48.0, 96.0, w, argb(color, 255));
    }
}

/// Batería horizontal de `w` x `h`. Verde cargando, roja con poca carga.
pub fn battery(c: &mut Canvas, r: FRect, percent: u8, charging: bool, color: u32) {
    let FRect { x, y, w, h } = r;
    let nub = (w * 0.1).max(2.0);
    let body = FRect::new(x, y, w - nub - 1.0, h);
    c.stroke_round(body.grow(-0.5), h * 0.28, 1.2, argb(color, 150));
    c.fill_round(FRect::new(x + w - nub, y + h * 0.3, nub, h * 0.4), nub / 2.0, argb(color, 150));
    let fill = if charging {
        0x58D130 // verde (COLORREF de #30d158)
    } else if percent <= 15 {
        0x3A45FF // rojo (#ff453a)
    } else {
        color
    };
    let inner = body.grow(-2.2);
    let fw = (inner.w * percent.min(100) as f32 / 100.0).max(if percent > 0 { 1.5 } else { 0.0 });
    if fw > 0.0 {
        c.fill_round(FRect::new(inner.x, inner.y, fw, inner.h), (inner.h * 0.22).max(1.0), argb(fill, 255));
    }
}

/// Candado pequeño (red protegida).
pub fn lock(c: &mut Canvas, x: f32, y: f32, s: f32, color: u32) {
    let w = (s * 0.12).max(1.0);
    c.arc(FRect::new(x + s * 0.22, y + s * 0.02, s * 0.56, s * 0.64), 180.0, 180.0, w, argb(color, 255));
    c.fill_round(FRect::new(x + s * 0.12, y + s * 0.42, s * 0.76, s * 0.52), s * 0.12, argb(color, 255));
}

/// Marca de verificación.
pub fn check(c: &mut Canvas, x: f32, y: f32, s: f32, color: u32) {
    let w = (s * 0.16).max(1.4);
    c.line(x + s * 0.1, y + s * 0.55, x + s * 0.38, y + s * 0.82, w, argb(color, 255));
    c.line(x + s * 0.38, y + s * 0.82, x + s * 0.92, y + s * 0.2, w, argb(color, 255));
}

/// Símbolo de Bluetooth (la "B" rúnica); `alpha` atenúa cuando está apagado.
pub fn bluetooth(c: &mut Canvas, x: f32, y: f32, s: f32, color: u32, alpha: u8) {
    let w = (s * 0.11).max(1.2);
    let p = |u: f32, v: f32| (x + u * s, y + v * s);
    let col = argb(color, alpha);
    let (a, b) = (p(0.5, 0.04), p(0.5, 0.96));
    c.line(a.0, a.1, b.0, b.1, w, col);
    let pts = [p(0.5, 0.04), p(0.8, 0.31), p(0.2, 0.69), p(0.5, 0.96), p(0.8, 0.69), p(0.2, 0.31), p(0.5, 0.04)];
    for pair in pts.windows(2) {
        c.line(pair[0].0, pair[0].1, pair[1].0, pair[1].1, w, col);
    }
}
