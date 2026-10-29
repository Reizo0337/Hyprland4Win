//! Lanzador de aplicaciones (Super+K), al estilo de Spotlight de macOS: tarjeta translúcida con
//! esquinas redondeadas, campo de búsqueda con lupa, fichas de color y selección en pastilla.
//! Se dibuja con GDI+ (`gfx`) en una ventana con transparencia por píxel.

use std::cell::RefCell;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::apps::{self, Entry};
use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::config::Bar;
use crate::layout::Rect;
use crate::search;
use crate::window;

pub const CLASS_NAME: &str = "HyprWinLauncher";

const MAX_ROWS: usize = 7;

/// Colores del sistema de Apple (modo oscuro) para las fichas de las apps.
const TILE_COLORS: [u32; 9] = [
    0x0A84FF, 0x5E5CE6, 0xBF5AF2, 0xFF375F, 0xFF453A, 0xFF9F0A, 0x30D158, 0x40C8E0, 0xAC8E68,
];

pub struct Fonts {
    query: Font,
    row: Font,
    header: Font,
    tile: Font,
}

impl Fonts {
    pub fn new(th: &Theme) -> Option<Fonts> {
        let b = th.base_px();
        Some(Fonts {
            query: Font::new(&th.font, b * 1.65, false)?,
            row: Font::new(&th.font, b * 1.15, false)?,
            header: Font::new(&th.font, b * 0.85, true)?,
            tile: Font::new(&th.font, b * 1.05, true)?,
        })
    }
}

/// Medidas (px reales) de la tarjeta según el número de filas.
struct Geo {
    margin: f32,
    card_w: f32,
    input_h: f32,
    header_h: f32,
    row_h: f32,
    pad: f32,
    rows: usize,
}

impl Geo {
    fn new(results: usize) -> Geo {
        Geo {
            margin: px(30.0),
            card_w: px(680.0),
            input_h: px(62.0),
            header_h: px(32.0),
            row_h: px(46.0),
            pad: px(24.0),
            rows: results.clamp(1, MAX_ROWS),
        }
    }

    fn card_h(&self) -> f32 {
        self.input_h + self.header_h + self.rows as f32 * self.row_h + px(12.0)
    }

    fn size(&self) -> (i32, i32) {
        ((self.card_w + 2.0 * self.margin).ceil() as i32, (self.card_h() + 2.0 * self.margin).ceil() as i32)
    }
}

pub struct View<'a> {
    pub query: &'a str,
    pub names: &'a [&'a str],
    pub sel: usize,
    pub top: usize,
    pub indexing: bool,
}

/// Tamaño del lienzo para `results` resultados.
pub fn canvas_size(results: usize) -> (i32, i32) {
    Geo::new(results).size()
}

pub(crate) fn tile_color(name: &str) -> u32 {
    let h = name.bytes().fold(7usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    // la tabla está en RGB; `argb` espera COLORREF (BGR)
    let c = TILE_COLORS[h % TILE_COLORS.len()];
    (c & 0xff) << 16 | (c & 0xff00) | c >> 16
}

pub fn render(c: &mut Canvas, th: &Theme, f: &Fonts, v: &View) {
    c.clear();
    let g = Geo::new(v.names.len());
    let card = FRect::new(g.margin, g.margin, g.card_w, g.card_h());
    let radius = px(18.0);

    // sombra suave: capas concéntricas con poco alfa, algo desplazadas hacia abajo
    for i in 0..16 {
        let grow = i as f32 * px(1.0);
        let r = FRect::new(card.x - grow, card.y - grow + px(8.0), card.w + 2.0 * grow, card.h + 2.0 * grow);
        c.fill_round(r, radius + grow, argb(0x000000, 3));
    }
    c.fill_round(card, radius, argb(th.bg, 0xF0));
    c.stroke_round(card.grow(-0.5), radius, 1.0, argb(th.fg, 30));

    // lupa
    let mid = card.y + g.input_h / 2.0;
    let lx = card.x + g.pad;
    c.stroke_ellipse(FRect::new(lx, mid - px(9.0), px(15.0), px(15.0)), px(2.0), argb(th.dim, 255));
    c.line(lx + px(13.0), mid + px(4.0), lx + px(19.0), mid + px(10.0), px(2.0), argb(th.dim, 255));

    // campo de búsqueda
    let tx = lx + px(36.0);
    let field = FRect::new(tx, card.y, card.w - (tx - card.x) - g.pad, g.input_h);
    if v.query.is_empty() {
        c.text(&f.query, "Buscar aplicaciones", field, argb(th.dim, 255), Align::Left);
        c.fill_round(FRect::new(tx, mid - px(12.0), px(2.0), px(24.0)), px(1.0), argb(th.accent, 255));
    } else {
        c.text(&f.query, v.query, field, argb(th.fg, 255), Align::Left);
        let cx = tx + c.measure(&f.query, v.query).min(field.w) + px(2.0);
        c.fill_round(FRect::new(cx, mid - px(12.0), px(2.0), px(24.0)), px(1.0), argb(th.accent, 255));
    }

    // separador y cabecera de sección
    let sep_y = card.y + g.input_h;
    c.fill_round(FRect::new(card.x, sep_y, card.w, 1.0), 0.0, argb(th.fg, 28));
    c.text(
        &f.header,
        "Aplicaciones",
        FRect::new(card.x + g.pad, sep_y, card.w - 2.0 * g.pad, g.header_h),
        argb(th.dim, 255),
        Align::Left,
    );

    // filas
    let list_y = sep_y + g.header_h;
    if v.names.is_empty() {
        let msg = if v.indexing { "Indexando aplicaciones…" } else { "Sin resultados" };
        c.text(&f.row, msg, FRect::new(card.x + g.pad, list_y, card.w - 2.0 * g.pad, g.row_h), argb(th.dim, 255), Align::Left);
        return;
    }
    for (row, name) in v.names.iter().enumerate().skip(v.top).take(MAX_ROWS) {
        let y = list_y + (row - v.top) as f32 * g.row_h;
        let selected = row == v.sel;
        if selected {
            c.fill_round(FRect::new(card.x + px(10.0), y + px(2.0), card.w - px(20.0), g.row_h - px(4.0)), px(10.0), argb(th.accent, 255));
        }
        let tile = px(30.0);
        let tile_r = FRect::new(card.x + g.pad - px(2.0), y + (g.row_h - tile) / 2.0, tile, tile);
        c.fill_round(tile_r, px(7.5), argb(tile_color(name), 255));
        if selected {
            c.stroke_round(tile_r.grow(-0.75), px(7.5), 1.5, argb(0xFFFFFF, 170));
        }
        let letter: String = name.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
        c.text(&f.tile, &letter, tile_r, argb(0xFFFFFF, 255), Align::Center);

        let text_x = tile_r.x + tile + px(14.0);
        let color = if selected { argb(th.accent_text, 255) } else { argb(th.fg, 255) };
        let name_w = card.x + card.w - px(64.0) - text_x;
        c.text(&f.row, name, FRect::new(text_x, y, name_w, g.row_h), color, Align::Left);
        if selected {
            c.text(
                &f.row,
                "↵",
                FRect::new(card.x + card.w - px(56.0), y, px(36.0), g.row_h),
                argb(th.accent_text, 200),
                Align::Right,
            );
        }
    }
}

struct Launcher {
    hwnd: HWND,
    theme: Theme,
    fonts: Fonts,
    query: String,
    results: Vec<Entry>,
    sel: usize,
    top: usize,
    prev_fg: isize,
    mon: Rect,
    canvas: Option<Canvas>,
}

thread_local! {
    static L: RefCell<Option<Launcher>> = const { RefCell::new(None) };
}

/// Ejecuta `f` si el estado no está prestado (los mensajes pueden ser reentrantes).
fn with<R>(f: impl FnOnce(&mut Launcher) -> R) -> Option<R> {
    L.with(|l| l.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn create(cfg: &Bar) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = Fonts::new(&theme).ok_or("no se pudo crear la fuente del lanzador")?;
    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = HSTRING::from(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("RegisterClassExW (lanzador) falló".into());
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(inst.into()),
            None,
        )
        .map_err(|e| e.to_string())?;
        L.with(|s| {
            *s.borrow_mut() = Some(Launcher {
                hwnd,
                theme,
                fonts,
                query: String::new(),
                results: Vec::new(),
                sel: 0,
                top: 0,
                prev_fg: 0,
                mon: Rect::new(0, 0, 1920, 1080),
                canvas: None,
            })
        });
    }
    Ok(())
}

pub fn is_open() -> bool {
    with(|l| unsafe { IsWindowVisible(l.hwnd).as_bool() }).unwrap_or(false)
}

/// Abre el lanzador centrado en el monitor `mon`, o lo cierra si ya estaba abierto.
pub fn toggle(mon: Rect) {
    if is_open() {
        close(true);
        return;
    }
    let Some(hwnd) = with(|l| {
        l.query.clear();
        l.sel = 0;
        l.top = 0;
        l.mon = mon;
        l.prev_fg = window::foreground();
        l.refresh();
        l.redraw();
        l.hwnd
    }) else {
        return;
    };
    apps::start_indexing(); // refresca el índice por si se instaló algo; el actual sigue valiendo
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    window::focus(window::id(hwnd));
}

fn close(refocus: bool) {
    let Some((hwnd, prev)) = with(|l| (l.hwnd, l.prev_fg)) else { return };
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    if refocus && prev != 0 && window::is_alive(prev) {
        window::focus(prev);
    }
}

fn launch_selected() {
    let Some(entry) = with(|l| l.results.get(l.sel).cloned()).flatten() else { return };
    close(false);
    apps::launch(&entry);
}

impl Launcher {
    fn refresh(&mut self) {
        let all = apps::entries();
        let names: Vec<String> = all.iter().map(|e| e.name.clone()).collect();
        self.results = search::rank(&self.query, &names, 200).into_iter().map(|i| all[i].clone()).collect();
        self.sel = 0;
        self.top = 0;
    }

    fn move_sel(&mut self, delta: i32) {
        if self.results.is_empty() {
            return;
        }
        let last = self.results.len() as i32 - 1;
        self.sel = (self.sel as i32 + delta).clamp(0, last) as usize;
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + MAX_ROWS {
            self.top = self.sel + 1 - MAX_ROWS;
        }
    }

    /// Redibuja y presenta la ventana (la coloca en el monitor y se adapta al número de resultados).
    fn redraw(&mut self) {
        let (w, h) = canvas_size(self.results.len());
        if self.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.canvas = Canvas::new(w, h);
        }
        let Some(canvas) = self.canvas.as_mut() else { return };
        let names: Vec<&str> = self.results.iter().map(|e| e.name.as_str()).collect();
        let indexing = names.is_empty() && apps::entries().is_empty();
        let view = View { query: &self.query, names: &names, sel: self.sel, top: self.top, indexing };
        render(canvas, &self.theme, &self.fonts, &view);
        let g = Geo::new(self.results.len());
        let x = self.mon.x + (self.mon.w - w) / 2;
        let y = self.mon.y + (self.mon.h as f32 * 0.18) as i32 - g.margin as i32;
        canvas.present(self.hwnd, Some((x, y)));
    }
}

fn edit(f: impl FnOnce(&mut Launcher)) {
    let _ = with(|l| {
        f(l);
        l.redraw();
    });
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_ACTIVATE => {
            if wp.0 & 0xffff == 0 {
                close(false); // perdió el foco (clic fuera, Alt+Tab...)
            }
            LRESULT(0)
        }
        WM_CHAR => {
            match wp.0 as u32 {
                0x08 => edit(|l| {
                    l.query.pop();
                    l.refresh();
                }),
                0x7f | 0x15 => edit(|l| {
                    l.query.clear(); // Ctrl+Backspace / Ctrl+U
                    l.refresh();
                }),
                c if c >= 0x20 => {
                    if let Some(ch) = char::from_u32(c) {
                        edit(|l| {
                            l.query.push(ch);
                            l.refresh();
                        });
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            match wp.0 as u32 {
                0x1B => close(true),
                0x0D => launch_selected(),
                0x26 => edit(|l| l.move_sel(-1)),
                0x28 | 0x09 => edit(|l| l.move_sel(1)),
                0x21 => edit(|l| l.move_sel(-(MAX_ROWS as i32))),
                0x22 => edit(|l| l.move_sel(MAX_ROWS as i32)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wp.0 >> 16) as i16;
            edit(|l| l.move_sel(if delta > 0 { -1 } else { 1 }));
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
            let hit = with(|l| {
                let g = Geo::new(l.results.len());
                let rel = y - g.margin - g.input_h - g.header_h;
                if rel < 0.0 {
                    return false;
                }
                let idx = l.top + (rel / g.row_h) as usize;
                (idx < l.results.len()).then(|| l.sel = idx).is_some()
            });
            if hit == Some(true) {
                launch_selected();
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
