//! Dibujo de los widgets de escritorio (estilo macOS): tarjetas translúcidas con el tema activo.
//! Todo es dibujo puro sobre un `Canvas` a partir de datos ya leídos, así que se puede renderizar a un PNG.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::icons::Icon;
use crate::palette::rgb_to_colorref;

// ---------- tipos ----------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Clock,
    Calendar,
    System,
    Weather,
    Media,
    Notes,
    Shortcuts,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    /// Alto y ancho de la tarjeta a 96 DPI.
    pub fn dims(self) -> (f32, f32) {
        match self {
            Size::Small => (160.0, 160.0),
            Size::Medium => (340.0, 160.0),
            Size::Large => (340.0, 340.0),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Size::Small => "pequeño",
            Size::Medium => "mediano",
            Size::Large => "grande",
        }
    }
}

impl Kind {
    pub const ALL: [Kind; 7] = [Kind::Clock, Kind::Calendar, Kind::System, Kind::Weather, Kind::Media, Kind::Notes, Kind::Shortcuts];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Clock => "Reloj",
            Kind::Calendar => "Calendario",
            Kind::System => "Sistema",
            Kind::Weather => "Tiempo",
            Kind::Media => "Música",
            Kind::Notes => "Notas",
            Kind::Shortcuts => "Atajos",
        }
    }

    /// Glifo de *Segoe Fluent Icons* y color de la ficha en el menú.
    pub fn glyph(self) -> (char, u32) {
        match self {
            Kind::Clock => ('\u{E121}', 0xFF9F0A),
            Kind::Calendar => ('\u{E787}', 0xFF453A),
            Kind::System => ('\u{E9D9}', 0x30D158),
            Kind::Weather => ('\u{E753}', 0x0A84FF),
            Kind::Media => ('\u{E8D6}', 0xFF375F),
            Kind::Notes => ('\u{E70B}', 0xFFD60A),
            Kind::Shortcuts => ('\u{E765}', 0xBF5AF2),
        }
    }

    /// Tamaños que admite cada widget.
    pub fn sizes(self) -> &'static [Size] {
        match self {
            Kind::Clock => &[Size::Small, Size::Medium],
            Kind::Calendar => &[Size::Small, Size::Large],
            Kind::System => &[Size::Medium],
            Kind::Weather => &[Size::Small, Size::Medium],
            Kind::Media => &[Size::Medium],
            Kind::Notes => &[Size::Medium, Size::Large],
            Kind::Shortcuts => &[Size::Large],
        }
    }
}

/// Instante local ya descompuesto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Now {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// 0 = lunes ... 6 = domingo
    pub weekday: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub cpu: f32,
    pub mem_pct: f32,
    pub mem_used_gb: f32,
    pub mem_total_gb: f32,
    pub disk_pct: f32,
    pub disk_free_gb: f32,
    /// Bytes por segundo.
    pub net_down: f64,
    pub net_up: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub label: String,
    pub hi: i32,
    pub lo: i32,
    pub code: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub city: String,
    pub temp: i32,
    /// Código WMO del tiempo actual.
    pub code: u32,
    pub days: Vec<Day>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub thumb: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Data {
    pub stats: Stats,
    pub weather: Option<Weather>,
    pub media: Option<Media>,
    pub note: String,
    /// (teclas, descripción) de los atajos.
    pub shortcuts: Vec<(String, String)>,
    /// Ciudad configurada para el tiempo.
    pub city: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    MediaPrev,
    MediaPlay,
    MediaNext,
}

pub struct WFonts {
    huge: Font,
    big: Font,
    med: Font,
    small: Font,
    tiny: Font,
    glyph: crate::iconset::IconFont,
}

impl WFonts {
    pub fn new(th: &Theme) -> Option<WFonts> {
        let b = th.base_px();
        Some(WFonts {
            huge: Font::new(&th.font, b * 4.4, false)?,
            big: Font::new(&th.font, b * 2.3, false)?,
            med: Font::new(&th.font, b * 1.2, false)?,
            small: Font::new(&th.font, b * 0.92, false)?,
            tiny: Font::new(&th.font, b * 0.78, true)?,
            glyph: crate::iconset::IconFont::new(&th.icons, b * 1.1)?,
        })
    }
}

// ---------- calendario (puro) ----------

pub const MONTHS: [&str; 12] =
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
pub const WEEKDAYS: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
pub const WEEKDAY_INITIALS: [&str; 7] = ["L", "M", "X", "J", "V", "S", "D"];

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
    }
}

/// Día de la semana (0 = lunes ... 6 = domingo), algoritmo de Sakamoto.
pub fn weekday_of(year: i32, month: u32, day: u32) -> u32 {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    let sunday_based = (y + y / 4 - y / 100 + y / 400 + T[(month - 1) as usize] + day as i32).rem_euclid(7);
    ((sunday_based + 6) % 7) as u32
}

/// Celdas del mes en filas de 7 (lunes primero); `None` = hueco.
pub fn month_grid(year: i32, month: u32) -> Vec<Option<u32>> {
    let first = weekday_of(year, month, 1) as usize;
    let mut cells: Vec<Option<u32>> = vec![None; first];
    cells.extend((1..=days_in_month(year, month)).map(Some));
    while !cells.len().is_multiple_of(7) {
        cells.push(None);
    }
    cells
}

// ---------- utilidades ----------

/// "1,2 MB/s", "840 KB/s", "0 B/s".
pub fn format_rate(bytes_per_s: f64) -> String {
    const K: f64 = 1024.0;
    if bytes_per_s >= K * K * K {
        format!("{:.1} GB/s", bytes_per_s / (K * K * K))
    } else if bytes_per_s >= K * K {
        format!("{:.1} MB/s", bytes_per_s / (K * K))
    } else if bytes_per_s >= K {
        format!("{:.0} KB/s", bytes_per_s / K)
    } else {
        format!("{:.0} B/s", bytes_per_s)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeatherIcon {
    Sun,
    PartlyCloudy,
    Cloud,
    Fog,
    Rain,
    Snow,
    Storm,
}

/// Código WMO (Open-Meteo) -> icono.
pub fn weather_icon(code: u32) -> WeatherIcon {
    match code {
        0 => WeatherIcon::Sun,
        1 | 2 => WeatherIcon::PartlyCloudy,
        3 => WeatherIcon::Cloud,
        45 | 48 => WeatherIcon::Fog,
        51..=67 | 80..=82 => WeatherIcon::Rain,
        71..=77 | 85 | 86 => WeatherIcon::Snow,
        95..=99 => WeatherIcon::Storm,
        _ => WeatherIcon::Cloud,
    }
}

pub fn weather_text(code: u32) -> &'static str {
    match weather_icon(code) {
        WeatherIcon::Sun => "Despejado",
        WeatherIcon::PartlyCloudy => "Poco nuboso",
        WeatherIcon::Cloud => "Nublado",
        WeatherIcon::Fog => "Niebla",
        WeatherIcon::Rain => "Lluvia",
        WeatherIcon::Snow => "Nieve",
        WeatherIcon::Storm => "Tormenta",
    }
}

fn wrap_lines(c: &mut Canvas, f: &Font, text: &str, max_w: f32, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    'outer: for para in text.lines() {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if cur.is_empty() || c.measure(f, &candidate) <= max_w {
                cur = candidate;
            } else {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
                if lines.len() >= max_lines {
                    break 'outer;
                }
            }
        }
        lines.push(cur);
        if lines.len() >= max_lines {
            break;
        }
    }
    lines.truncate(max_lines);
    lines
}

fn col(rgb: u32) -> u32 {
    rgb_to_colorref(rgb)
}

// ---------- dibujo ----------

/// Margen para la sombra alrededor de la tarjeta.
pub fn margin() -> f32 {
    px(26.0)
}

/// Tamaño del lienzo (tarjeta + margen) en px reales.
pub fn canvas_size(size: Size) -> (i32, i32) {
    let (w, h) = size.dims();
    ((px(w) + 2.0 * margin()).ceil() as i32, (px(h) + 2.0 * margin()).ceil() as i32)
}

/// Dibuja el widget. Devuelve las zonas interactivas (controles de música).
#[allow(clippy::too_many_arguments)]
pub fn render(c: &mut Canvas, th: &Theme, f: &WFonts, kind: Kind, size: Size, data: &Data, now: &Now, thumb: Option<&Icon>) -> Vec<(FRect, Hit)> {
    c.clear();
    let (w, h) = size.dims();
    let card = FRect::new(margin(), margin(), px(w), px(h));
    c.card(card, px(24.0), th.bg, 0xD8, th.fg);
    let inner = card.grow(-px(16.0));
    let mut hits = Vec::new();
    match kind {
        Kind::Clock => clock(c, th, f, size, now, card),
        Kind::Calendar => calendar(c, th, f, size, now, inner),
        Kind::System => system(c, th, f, &data.stats, inner),
        Kind::Weather => weather(c, th, f, size, data, inner),
        Kind::Media => hits = media(c, th, f, data.media.as_ref(), thumb, inner),
        Kind::Notes => notes(c, th, f, size, &data.note, inner),
        Kind::Shortcuts => shortcuts(c, th, f, &data.shortcuts, inner),
    }
    hits
}

fn label(c: &mut Canvas, th: &Theme, f: &WFonts, text: &str, r: FRect, color: u32) {
    c.text(&f.tiny, &text.to_uppercase(), r, argb(color, 255), Align::Left);
    let _ = th;
}

fn clock(c: &mut Canvas, th: &Theme, f: &WFonts, size: Size, now: &Now, card: FRect) {
    if size == Size::Small {
        let (cx, cy) = (card.x + card.w / 2.0, card.y + card.h / 2.0);
        let r = card.w.min(card.h) / 2.0 - px(14.0);
        c.stroke_ellipse(FRect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), px(1.5), argb(th.fg, 50));
        for i in 0..12 {
            let a = i as f32 * 30.0_f32.to_radians();
            let (len, wd) = if i % 3 == 0 { (px(9.0), px(2.2)) } else { (px(5.0), px(1.4)) };
            c.line(cx + (r - len) * a.sin(), cy - (r - len) * a.cos(), cx + (r - px(2.0)) * a.sin(), cy - (r - px(2.0)) * a.cos(), wd, argb(th.fg, if i % 3 == 0 { 230 } else { 120 }));
        }
        let hand = |c: &mut Canvas, deg: f32, len: f32, width: f32, color: u32| {
            let a = deg.to_radians();
            c.line(cx - len * 0.12 * a.sin(), cy + len * 0.12 * a.cos(), cx + len * a.sin(), cy - len * a.cos(), width, color);
        };
        hand(c, ((now.hour % 12) as f32 + now.min as f32 / 60.0) * 30.0, r * 0.5, px(4.0), argb(th.fg, 255));
        hand(c, (now.min as f32 + now.sec as f32 / 60.0) * 6.0, r * 0.78, px(2.8), argb(th.fg, 255));
        hand(c, now.sec as f32 * 6.0, r * 0.86, px(1.4), argb(th.accent, 255));
        c.fill_ellipse(FRect::new(cx - px(3.5), cy - px(3.5), px(7.0), px(7.0)), argb(th.accent, 255));
    } else {
        let inner = card.grow(-px(18.0));
        label(c, th, f, WEEKDAYS[now.weekday as usize], FRect::new(inner.x, inner.y, inner.w, px(16.0)), th.accent);
        let time = format!("{:02}:{:02}", now.hour, now.min);
        c.text(&f.huge, &time, FRect::new(inner.x, inner.y + px(16.0), inner.w, px(76.0)), argb(th.fg, 255), Align::Left);
        let date = format!("{} de {} de {}", now.day, MONTHS[(now.month - 1) as usize], now.year);
        c.text(&f.med, &date, FRect::new(inner.x, inner.y + inner.h - px(26.0), inner.w, px(24.0)), argb(th.dim, 255), Align::Left);
        c.text(&f.big, &format!(":{:02}", now.sec), FRect::new(inner.x, inner.y + px(34.0), inner.w, px(36.0)), argb(th.dim, 200), Align::Right);
    }
}

fn calendar(c: &mut Canvas, th: &Theme, f: &WFonts, size: Size, now: &Now, inner: FRect) {
    if size == Size::Small {
        label(c, th, f, WEEKDAYS[now.weekday as usize], FRect::new(inner.x, inner.y, inner.w, px(16.0)), 0x3A45FF);
        c.text(&f.huge, &now.day.to_string(), FRect::new(inner.x, inner.y + px(14.0), inner.w, px(76.0)), argb(th.fg, 255), Align::Left);
        c.text(&f.med, &format!("{} {}", MONTHS[(now.month - 1) as usize], now.year), FRect::new(inner.x, inner.y + inner.h - px(24.0), inner.w, px(22.0)), argb(th.dim, 255), Align::Left);
        return;
    }
    let title = format!("{} {}", MONTHS[(now.month - 1) as usize], now.year);
    c.text(&f.big, &title, FRect::new(inner.x, inner.y, inner.w, px(36.0)), argb(th.fg, 255), Align::Left);
    let cell = inner.w / 7.0;
    let top = inner.y + px(46.0);
    for (i, w) in WEEKDAY_INITIALS.iter().enumerate() {
        let weekend = i >= 5;
        c.text(&f.tiny, w, FRect::new(inner.x + i as f32 * cell, top, cell, px(18.0)), argb(if weekend { th.dim } else { th.fg }, if weekend { 200 } else { 230 }), Align::Center);
    }
    let rows_top = top + px(22.0);
    let row_h = (inner.y + inner.h - rows_top) / 6.0;
    for (i, d) in month_grid(now.year, now.month).iter().enumerate() {
        let Some(day) = d else { continue };
        let r = FRect::new(inner.x + (i % 7) as f32 * cell, rows_top + (i / 7) as f32 * row_h, cell, row_h);
        let today = *day == now.day;
        if today {
            let d = r.w.min(r.h) - px(4.0);
            c.fill_ellipse(FRect::new(r.x + (r.w - d) / 2.0, r.y + (r.h - d) / 2.0, d, d), argb(0x3A45FF, 255));
        }
        let color = if today { 0xFFFFFF } else if i % 7 >= 5 { th.dim } else { th.fg };
        c.text(&f.small, &day.to_string(), r, argb(color, 255), Align::Center);
    }
}

fn system(c: &mut Canvas, th: &Theme, f: &WFonts, s: &Stats, inner: FRect) {
    let rows: [(&str, f32, String); 4] = [
        ("CPU", s.cpu, format!("{:.0} %", s.cpu)),
        ("Memoria", s.mem_pct, format!("{:.1} / {:.0} GB", s.mem_used_gb, s.mem_total_gb)),
        ("Disco C:", s.disk_pct, format!("{:.0} GB libres", s.disk_free_gb)),
        ("Red", 0.0, format!("↓ {}  ↑ {}", format_rate(s.net_down), format_rate(s.net_up))),
    ];
    let row_h = inner.h / 4.0;
    for (i, (name, pct, text)) in rows.iter().enumerate() {
        let y = inner.y + i as f32 * row_h;
        c.text(&f.small, name, FRect::new(inner.x, y, px(70.0), row_h), argb(th.dim, 255), Align::Left);
        if i < 3 {
            let track = FRect::new(inner.x + px(72.0), y + row_h / 2.0 - px(4.0), inner.w - px(72.0) - px(112.0), px(8.0));
            c.fill_round(track, px(4.0), argb(th.fg, 38));
            let fill = (track.w * (pct / 100.0).clamp(0.0, 1.0)).max(if *pct > 0.0 { px(8.0) } else { 0.0 });
            let color = if *pct >= 85.0 { col(0xFF453A) } else if *pct >= 65.0 { col(0xFF9F0A) } else { th.accent };
            if fill > 0.0 {
                c.fill_round(FRect::new(track.x, track.y, fill, track.h), px(4.0), argb(color, 255));
            }
            c.text(&f.small, text, FRect::new(track.x + track.w + px(8.0), y, px(104.0), row_h), argb(th.fg, 255), Align::Right);
        } else {
            c.text(&f.small, text, FRect::new(inner.x + px(72.0), y, inner.w - px(72.0), row_h), argb(th.fg, 255), Align::Right);
        }
    }
}

fn weather_glyph(c: &mut Canvas, th: &Theme, icon: WeatherIcon, r: FRect) {
    let sun = col(0xFFD60A);
    let cloud = argb(th.fg, 235);
    let blue = col(0x64D2FF);
    let (x, y, w, h) = (r.x, r.y, r.w, r.h);
    let draw_sun = |c: &mut Canvas, cx: f32, cy: f32, rad: f32| {
        c.fill_ellipse(FRect::new(cx - rad, cy - rad, 2.0 * rad, 2.0 * rad), argb(sun, 255));
        for i in 0..8 {
            let a = (i as f32 * 45.0).to_radians();
            c.line(cx + rad * 1.35 * a.sin(), cy - rad * 1.35 * a.cos(), cx + rad * 1.75 * a.sin(), cy - rad * 1.75 * a.cos(), (rad * 0.22).max(1.2), argb(sun, 255));
        }
    };
    let draw_cloud = |c: &mut Canvas, ox: f32, oy: f32, s: f32| {
        c.fill_ellipse(FRect::new(ox + s * 0.10, oy + s * 0.38, s * 0.42, s * 0.42), cloud);
        c.fill_ellipse(FRect::new(ox + s * 0.30, oy + s * 0.18, s * 0.46, s * 0.46), cloud);
        c.fill_ellipse(FRect::new(ox + s * 0.54, oy + s * 0.34, s * 0.38, s * 0.38), cloud);
        c.fill_round(FRect::new(ox + s * 0.22, oy + s * 0.46, s * 0.58, s * 0.32), s * 0.16, cloud);
    };
    match icon {
        WeatherIcon::Sun => draw_sun(c, x + w / 2.0, y + h / 2.0, w.min(h) * 0.22),
        WeatherIcon::PartlyCloudy => {
            draw_sun(c, x + w * 0.36, y + h * 0.34, w.min(h) * 0.15);
            draw_cloud(c, x + w * 0.12, y + h * 0.18, w.min(h) * 0.9);
        }
        WeatherIcon::Cloud => draw_cloud(c, x, y, w.min(h)),
        WeatherIcon::Fog => {
            draw_cloud(c, x, y - h * 0.1, w.min(h));
            for i in 0..3 {
                c.line(x + w * 0.18, y + h * (0.72 + i as f32 * 0.1), x + w * 0.82, y + h * (0.72 + i as f32 * 0.1), px(2.0), cloud);
            }
        }
        WeatherIcon::Rain => {
            draw_cloud(c, x, y - h * 0.08, w.min(h));
            for i in 0..3 {
                let dx = x + w * (0.3 + i as f32 * 0.2);
                c.line(dx, y + h * 0.78, dx - w * 0.06, y + h * 0.94, px(2.2), argb(blue, 255));
            }
        }
        WeatherIcon::Snow => {
            draw_cloud(c, x, y - h * 0.08, w.min(h));
            for i in 0..3 {
                c.fill_ellipse(FRect::new(x + w * (0.28 + i as f32 * 0.2), y + h * 0.82, px(4.0), px(4.0)), argb(0xFFFFFF, 255));
            }
        }
        WeatherIcon::Storm => {
            draw_cloud(c, x, y - h * 0.08, w.min(h));
            c.fill_polygon(
                &[(x + w * 0.52, y + h * 0.62), (x + w * 0.38, y + h * 0.86), (x + w * 0.5, y + h * 0.86), (x + w * 0.44, y + h * 1.04), (x + w * 0.66, y + h * 0.78), (x + w * 0.54, y + h * 0.78)],
                argb(sun, 255),
            );
        }
    }
}

fn weather(c: &mut Canvas, th: &Theme, f: &WFonts, size: Size, data: &Data, inner: FRect) {
    let Some(wx) = &data.weather else {
        let msg = if data.city.is_empty() { "Elige tu ciudad en Menú > Widgets" } else { "Cargando el tiempo…" };
        c.text(&f.small, msg, inner, argb(th.dim, 255), Align::Center);
        return;
    };
    if size == Size::Small {
        c.text(&f.small, &wx.city, FRect::new(inner.x, inner.y, inner.w, px(20.0)), argb(th.fg, 255), Align::Left);
        c.text(&f.huge, &format!("{}°", wx.temp), FRect::new(inner.x, inner.y + px(18.0), inner.w, px(76.0)), argb(th.fg, 255), Align::Left);
        weather_glyph(c, th, weather_icon(wx.code), FRect::new(inner.x + inner.w - px(40.0), inner.y + px(2.0), px(40.0), px(40.0)));
        c.text(&f.small, weather_text(wx.code), FRect::new(inner.x, inner.y + inner.h - px(40.0), inner.w, px(18.0)), argb(th.fg, 255), Align::Left);
        if let Some(d) = wx.days.first() {
            c.text(&f.small, &format!("Máx. {}°  Mín. {}°", d.hi, d.lo), FRect::new(inner.x, inner.y + inner.h - px(20.0), inner.w, px(18.0)), argb(th.dim, 255), Align::Left);
        }
        return;
    }
    let left_w = inner.w * 0.42;
    c.text(&f.small, &wx.city, FRect::new(inner.x, inner.y, left_w, px(20.0)), argb(th.fg, 255), Align::Left);
    c.text(&f.huge, &format!("{}°", wx.temp), FRect::new(inner.x, inner.y + px(18.0), left_w, px(76.0)), argb(th.fg, 255), Align::Left);
    c.text(&f.small, weather_text(wx.code), FRect::new(inner.x, inner.y + inner.h - px(22.0), left_w, px(20.0)), argb(th.dim, 255), Align::Left);
    let col_w = (inner.w - left_w) / 3.0;
    for (i, d) in wx.days.iter().take(3).enumerate() {
        let x = inner.x + left_w + i as f32 * col_w;
        c.text(&f.tiny, &d.label.to_uppercase(), FRect::new(x, inner.y + px(4.0), col_w, px(16.0)), argb(th.dim, 255), Align::Center);
        weather_glyph(c, th, weather_icon(d.code), FRect::new(x + col_w / 2.0 - px(17.0), inner.y + px(30.0), px(34.0), px(34.0)));
        c.text(&f.small, &format!("{}°", d.hi), FRect::new(x, inner.y + px(80.0), col_w, px(18.0)), argb(th.fg, 255), Align::Center);
        c.text(&f.small, &format!("{}°", d.lo), FRect::new(x, inner.y + px(100.0), col_w, px(18.0)), argb(th.dim, 255), Align::Center);
    }
}

fn media(c: &mut Canvas, th: &Theme, f: &WFonts, media: Option<&Media>, thumb: Option<&Icon>, inner: FRect) -> Vec<(FRect, Hit)> {
    let side = inner.h - px(4.0);
    let art = FRect::new(inner.x, inner.y + px(2.0), side, side);
    match thumb {
        Some(icon) => c.draw_icon(icon, art),
        None => {
            c.fill_round(art, px(12.0), argb(th.accent, 255));
            f.glyph.draw(c, '\u{E8D6}', art, argb(th.accent_text, 255));
        }
    }
    let x = art.x + art.w + px(14.0);
    let w = inner.x + inner.w - x;
    let Some(m) = media.filter(|m| !m.title.is_empty()) else {
        c.text(&f.med, "Nada en reproducción", FRect::new(x, inner.y, w, inner.h), argb(th.dim, 255), Align::Left);
        return Vec::new();
    };
    c.text(&f.med, &m.title, FRect::new(x, inner.y + px(6.0), w, px(24.0)), argb(th.fg, 255), Align::Left);
    c.text(&f.small, &m.artist, FRect::new(x, inner.y + px(30.0), w, px(20.0)), argb(th.dim, 255), Align::Left);
    // controles
    let cy = inner.y + inner.h - px(34.0);
    let btn = px(34.0);
    let cx = x + w / 2.0;
    let mut hits = Vec::new();
    let prev = FRect::new(cx - btn * 1.9, cy, btn, btn);
    let play = FRect::new(cx - btn / 2.0, cy - px(2.0), btn + px(4.0), btn + px(4.0));
    let next = FRect::new(cx + btn * 0.9, cy, btn, btn);
    let color = argb(th.fg, 255);
    for (r, flip) in [(prev, true), (next, false)] {
        let (cx2, cy2) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let s = px(7.0);
        let dir = if flip { -1.0 } else { 1.0 };
        c.fill_polygon(&[(cx2 - dir * s, cy2 - s), (cx2 - dir * s, cy2 + s), (cx2 + dir * s * 0.2, cy2)], color);
        c.fill_polygon(&[(cx2 + dir * s * 0.1, cy2 - s), (cx2 + dir * s * 0.1, cy2 + s), (cx2 + dir * s * 1.3, cy2)], color);
    }
    c.fill_ellipse(play, argb(th.accent, 255));
    let (pcx, pcy) = (play.x + play.w / 2.0, play.y + play.h / 2.0);
    if m.playing {
        for dx in [-px(4.0), px(1.5)] {
            c.fill_round(FRect::new(pcx + dx, pcy - px(7.0), px(3.5), px(14.0)), px(1.5), argb(th.accent_text, 255));
        }
    } else {
        c.fill_polygon(&[(pcx - px(4.0), pcy - px(8.0)), (pcx - px(4.0), pcy + px(8.0)), (pcx + px(8.0), pcy)], argb(th.accent_text, 255));
    }
    hits.push((prev.grow(px(4.0)), Hit::MediaPrev));
    hits.push((play, Hit::MediaPlay));
    hits.push((next.grow(px(4.0)), Hit::MediaNext));
    hits
}

fn notes(c: &mut Canvas, th: &Theme, f: &WFonts, size: Size, note: &str, inner: FRect) {
    label(c, th, f, "Nota", FRect::new(inner.x, inner.y, inner.w, px(16.0)), 0x0AD6FF);
    if note.trim().is_empty() {
        c.text(&f.small, "Escríbela en Menú > Widgets > Editar la nota", FRect::new(inner.x, inner.y + px(26.0), inner.w, px(40.0)), argb(th.dim, 255), Align::Left);
        return;
    }
    let line_h = px(21.0);
    let max_lines = (((inner.h - px(26.0)) / line_h).floor() as usize).max(1).min(if size == Size::Large { 12 } else { 4 });
    for (i, line) in wrap_lines(c, &f.med, note, inner.w, max_lines).iter().enumerate() {
        c.text(&f.med, line, FRect::new(inner.x, inner.y + px(24.0) + i as f32 * line_h, inner.w, line_h), argb(th.fg, 255), Align::Left);
    }
}

fn shortcuts(c: &mut Canvas, th: &Theme, f: &WFonts, list: &[(String, String)], inner: FRect) {
    label(c, th, f, "Atajos", FRect::new(inner.x, inner.y, inner.w, px(16.0)), th.accent);
    let row_h = px(27.0);
    let rows = (((inner.h - px(26.0)) / row_h).floor() as usize).max(1);
    for (i, (keys, desc)) in list.iter().take(rows).enumerate() {
        let y = inner.y + px(24.0) + i as f32 * row_h;
        let kw = c.measure(&f.small, keys) + px(14.0);
        let pill = FRect::new(inner.x, y + px(3.0), kw, row_h - px(6.0));
        c.fill_round(pill, px(7.0), argb(th.fg, 32));
        c.text(&f.small, keys, pill, argb(th.fg, 255), Align::Center);
        c.text(&f.small, desc, FRect::new(inner.x + kw + px(10.0), y, inner.w - kw - px(10.0), row_h), argb(th.dim, 255), Align::Left);
    }
    if list.is_empty() {
        c.text(&f.small, "Sin atajos", FRect::new(inner.x, inner.y + px(26.0), inner.w, px(20.0)), argb(th.dim, 255), Align::Left);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_algorithm_matches_known_dates() {
        assert_eq!(weekday_of(2026, 10, 29), 3); // jueves
        assert_eq!(weekday_of(2000, 1, 1), 5); // sábado
        assert_eq!(weekday_of(2024, 2, 29), 3); // jueves (bisiesto)
        assert_eq!(weekday_of(2023, 12, 31), 6); // domingo
        assert_eq!(weekday_of(2026, 3, 2), 0); // lunes
    }

    #[test]
    fn month_lengths_and_leap_years() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 10), 31);
        assert_eq!(days_in_month(2026, 9), 30);
    }

    #[test]
    fn month_grid_is_aligned_to_monday() {
        let g = month_grid(2026, 10); // el 1 de octubre de 2026 es jueves
        assert_eq!(g.len() % 7, 0);
        assert_eq!(&g[..4], &[None, None, None, Some(1)]);
        assert_eq!(g.iter().flatten().count(), 31);
        assert_eq!(g.iter().flatten().last(), Some(&31));
        // febrero de 2027 empieza en lunes y tiene 28 días: 4 filas exactas
        assert_eq!(month_grid(2027, 2).len(), 28);
    }

    #[test]
    fn rates_are_formatted_with_units() {
        assert_eq!(format_rate(0.0), "0 B/s");
        assert_eq!(format_rate(900.0), "900 B/s");
        assert_eq!(format_rate(2048.0), "2 KB/s");
        assert_eq!(format_rate(1.5 * 1024.0 * 1024.0), "1.5 MB/s");
        assert_eq!(format_rate(3.0 * 1024.0 * 1024.0 * 1024.0), "3.0 GB/s");
    }

    #[test]
    fn wmo_codes_map_to_icons() {
        assert_eq!(weather_icon(0), WeatherIcon::Sun);
        assert_eq!(weather_icon(2), WeatherIcon::PartlyCloudy);
        assert_eq!(weather_icon(45), WeatherIcon::Fog);
        assert_eq!(weather_icon(63), WeatherIcon::Rain);
        assert_eq!(weather_icon(73), WeatherIcon::Snow);
        assert_eq!(weather_icon(95), WeatherIcon::Storm);
        assert_eq!(weather_icon(999), WeatherIcon::Cloud);
    }

    #[test]
    fn every_kind_has_sizes_and_unique_glyphs() {
        for k in Kind::ALL {
            assert!(!k.sizes().is_empty() && !k.label().is_empty());
        }
        let mut g: Vec<char> = Kind::ALL.iter().map(|k| k.glyph().0).collect();
        g.sort_unstable();
        g.dedup();
        assert_eq!(g.len(), Kind::ALL.len());
    }

    #[test]
    fn text_wrapping_respects_width_and_line_limit() {
        let theme = Theme::from_cfg(&crate::config::Bar::default()).unwrap();
        let font = Font::new(&theme.font, 16.0, false).unwrap();
        let mut c = Canvas::new(300, 100).unwrap();
        let text = "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince";
        let lines = wrap_lines(&mut c, &font, text, 120.0, 10);
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| c.measure(&font, l) <= 120.0 || !l.contains(' ')));
        assert_eq!(lines.join(" "), text);
        assert_eq!(wrap_lines(&mut c, &font, text, 120.0, 2).len(), 2);
        assert_eq!(wrap_lines(&mut c, &font, "a\nb\nc", 200.0, 10), vec!["a", "b", "c"]);
    }
}
