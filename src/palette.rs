//! Paleta de comandos al estilo de Spotlight de macOS: tarjeta translúcida con esquinas redondeadas,
//! búsqueda grande con lupa, secciones, filas con icono y selección en pastilla. Es genérica: el
//! contenido y las acciones los pone un `Handler` (lanzador de apps, menú del sistema...).
//! Se dibuja con GDI+ (`gfx`) en una ventana con transparencia por píxel.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::icons::{self, Icon};
use crate::keybinds;
use crate::layout::Rect;
use crate::window;

pub const CLASS_NAME: &str = "HyprWinPalette";

const MAX_ROWS: usize = 7;

/// Mensaje que avisa a la paleta de que un favicon terminó de descargarse.
pub const WM_ICON_READY: u32 = WM_APP + 5;
/// Mensaje que pide volver a calcular las filas (llegó un resultado de una búsqueda en segundo plano).
pub const WM_ROWS_READY: u32 = WM_APP + 6;

static PALETTE_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Colores del sistema de Apple para las fichas.
const TILE_COLORS: [u32; 9] = [0x0A84FF, 0x5E5CE6, 0xBF5AF2, 0xFF375F, 0xFF453A, 0xFF9F0A, 0x30D158, 0x40C8E0, 0xAC8E68];

/// Ficha de color estable para un nombre (COLORREF).
pub fn tile_color(name: &str) -> u32 {
    let h = name.bytes().fold(7usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    rgb_to_colorref(TILE_COLORS[h % TILE_COLORS.len()])
}

/// 0xRRGGBB -> COLORREF (0x00BBGGRR).
pub fn rgb_to_colorref(c: u32) -> u32 {
    (c & 0xff) << 16 | (c & 0xff00) | c >> 16
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowIcon {
    /// Ficha de color con la inicial del título.
    Letter,
    /// Icono real de una entrada de `shell:AppsFolder`.
    App(String),
    /// Favicon de un dominio (se descarga en segundo plano; mientras tanto, la ficha de color).
    Favicon(String),
    /// Glifo de *Segoe Fluent Icons* sobre una ficha de color (0xRRGGBB).
    Glyph(char, u32),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub title: String,
    pub subtitle: String,
    /// Etiqueta a la derecha ("Aplicación", "Cambiar"...).
    pub hint: String,
    pub icon: RowIcon,
    /// Cabecera de sección; vacía = sin cabecera.
    pub section: &'static str,
}

impl Row {
    pub fn new(title: impl Into<String>) -> Row {
        Row { title: title.into(), subtitle: String::new(), hint: String::new(), icon: RowIcon::Letter, section: "" }
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Row {
        self.subtitle = s.into();
        self
    }

    pub fn hint(mut self, s: impl Into<String>) -> Row {
        self.hint = s.into();
        self
    }

    pub fn icon(mut self, i: RowIcon) -> Row {
        self.icon = i;
        self
    }

    pub fn glyph(self, c: char, rgb: u32) -> Row {
        self.icon(RowIcon::Glyph(c, rgb))
    }

    pub fn section(mut self, s: &'static str) -> Row {
        self.section = s;
        self
    }
}

/// Qué hacer tras activar una fila.
pub enum Outcome {
    Close,
    /// Vacía la consulta y vuelve a pedir las filas (cambio de página).
    Refresh,
    /// Solo redibujar.
    Stay,
    /// Vuelve a pedir las filas conservando la consulta (p. ej. para mostrar un aviso).
    Rerun,
    /// Sustituye el contenido de la paleta por otro manejador.
    Switch(Box<dyn Handler>),
}

/// Contenido y comportamiento de la paleta.
pub trait Handler {
    fn placeholder(&self) -> String;
    /// Ruta ("Instalar") que se muestra como etiqueta a la izquierda del campo.
    fn crumb(&self) -> String {
        String::new()
    }
    /// ¿Mostrar la lista con la consulta vacía? (Spotlight solo muestra la barra hasta escribir.)
    fn show_on_empty(&self) -> bool {
        false
    }
    fn empty_message(&self) -> String {
        "Sin resultados".into()
    }
    fn rows(&mut self, query: &str) -> Vec<Row>;
    fn activate(&mut self, index: usize, query: &str) -> Outcome;
    /// Esc o retroceso con el campo vacío: sube un nivel. `false` = no hay nivel superior (cerrar).
    fn back(&mut self) -> bool {
        false
    }
    /// ↓ o Tab con el campo vacío y la lista oculta: pedir que se muestre.
    fn expand(&mut self) -> bool {
        false
    }
    /// Se está capturando una combinación de teclas (el hook las entrega a `on_capture`).
    fn capturing(&self) -> bool {
        false
    }
    fn capture(&mut self, _vk: u32, _mods: u32) -> Outcome {
        Outcome::Stay
    }
}

// ---------- dibujo ----------

pub struct Fonts {
    query: Font,
    row: Font,
    sub: Font,
    header: Font,
    hint: Font,
    glyph: Font,
    letter: Font,
}

impl Fonts {
    pub fn new(th: &Theme) -> Option<Fonts> {
        let b = th.base_px();
        Some(Fonts {
            query: Font::new(&th.font, b * 1.9, false)?,
            row: Font::new(&th.font, b * 1.2, false)?,
            sub: Font::new(&th.font, b * 0.88, false)?,
            header: Font::new(&th.font, b * 0.82, true)?,
            hint: Font::new(&th.font, b * 0.9, false)?,
            glyph: Font::with_fallbacks(&["Segoe Fluent Icons", "Segoe MDL2 Assets"], b * 1.2, false)?,
            letter: Font::new(&th.font, b * 1.15, true)?,
        })
    }
}

struct Geo {
    margin: f32,
    card_w: f32,
    input_h: f32,
    header_h: f32,
    row_h: f32,
    pad: f32,
    radius: f32,
}

impl Geo {
    fn new() -> Geo {
        Geo { margin: px(30.0), card_w: px(700.0), input_h: px(68.0), header_h: px(30.0), row_h: px(54.0), pad: px(24.0), radius: px(20.0) }
    }
}

enum Line<'a> {
    Header(&'a str),
    Row(usize),
}

/// Cabeceras y filas visibles desde `top`.
fn lines(rows: &[Row], top: usize) -> Vec<Line<'_>> {
    let mut v = Vec::new();
    let mut last = "\0";
    for (i, r) in rows.iter().enumerate().skip(top).take(MAX_ROWS) {
        if !r.section.is_empty() && r.section != last {
            v.push(Line::Header(r.section));
        }
        last = r.section;
        v.push(Line::Row(i));
    }
    v
}

pub struct View<'a> {
    pub crumb: &'a str,
    pub placeholder: &'a str,
    pub query: &'a str,
    pub rows: &'a [Row],
    /// Icono real de cada fila (mismo orden que `rows`), si lo hay.
    pub icons: &'a [Option<&'a Icon>],
    pub sel: usize,
    pub top: usize,
    pub show_list: bool,
    pub empty_msg: &'a str,
}

fn list_height(v: &View, g: &Geo) -> f32 {
    if !v.show_list {
        return 0.0;
    }
    let body: f32 = if v.rows.is_empty() {
        g.row_h
    } else {
        lines(v.rows, v.top).iter().map(|l| if matches!(l, Line::Header(_)) { g.header_h } else { g.row_h }).sum()
    };
    body + px(12.0)
}

/// Tamaño del lienzo (con margen para la sombra).
pub fn canvas_size(v: &View) -> (i32, i32) {
    let g = Geo::new();
    let h = g.input_h + list_height(v, &g);
    ((g.card_w + 2.0 * g.margin).ceil() as i32, (h + 2.0 * g.margin).ceil() as i32)
}

/// Fila bajo la coordenada vertical `rel` (px desde el inicio de la lista), para el clic.
pub fn row_at(rows: &[Row], top: usize, rel: f32) -> Option<usize> {
    let g = Geo::new();
    let mut y = 0.0;
    for l in lines(rows, top) {
        let h = if matches!(l, Line::Header(_)) { g.header_h } else { g.row_h };
        if let Line::Row(i) = l {
            if rel >= y && rel < y + h {
                return Some(i);
            }
        }
        y += h;
    }
    None
}

/// Y de inicio de la lista respecto al borde superior del lienzo.
fn list_top() -> f32 {
    let g = Geo::new();
    g.margin + g.input_h
}

pub fn render(c: &mut Canvas, th: &Theme, f: &Fonts, v: &View) {
    c.clear();
    let g = Geo::new();
    let card = FRect::new(g.margin, g.margin, g.card_w, g.input_h + list_height(v, &g));
    c.card(card, g.radius, th.bg, 0xF0, th.fg);

    // lupa
    let mid = card.y + g.input_h / 2.0;
    let lx = card.x + g.pad;
    c.stroke_ellipse(FRect::new(lx, mid - px(11.0), px(18.0), px(18.0)), px(2.2), argb(th.dim, 255));
    c.line(lx + px(15.5), mid + px(5.5), lx + px(22.0), mid + px(12.0), px(2.2), argb(th.dim, 255));

    // etiqueta de ruta + campo de búsqueda
    let mut tx = lx + px(42.0);
    if !v.crumb.is_empty() {
        let w = c.measure(&f.hint, v.crumb) + px(20.0);
        let r = FRect::new(tx, mid - px(14.0), w, px(28.0));
        c.fill_round(r, px(8.0), argb(th.fg, 30));
        c.text(&f.hint, v.crumb, r, argb(th.fg, 230), Align::Center);
        tx += w + px(10.0);
    }
    let field = FRect::new(tx, card.y, card.x + card.w - tx - g.pad, g.input_h);
    let caret_x = if v.query.is_empty() {
        c.text(&f.query, v.placeholder, field, argb(th.dim, 255), Align::Left);
        tx
    } else {
        c.text(&f.query, v.query, field, argb(th.fg, 255), Align::Left);
        tx + c.measure(&f.query, v.query).min(field.w) + px(2.0)
    };
    c.fill_round(FRect::new(caret_x, mid - px(14.0), px(2.0), px(28.0)), px(1.0), argb(th.accent, 255));

    if !v.show_list {
        return;
    }
    let sep_y = card.y + g.input_h;
    c.fill_round(FRect::new(card.x, sep_y, card.w, 1.0), 0.0, argb(th.fg, 28));

    let mut y = sep_y;
    if v.rows.is_empty() {
        c.text(&f.row, v.empty_msg, FRect::new(card.x + g.pad, y, card.w - 2.0 * g.pad, g.row_h), argb(th.dim, 255), Align::Left);
        return;
    }
    for line in lines(v.rows, v.top) {
        match line {
            Line::Header(h) => {
                c.text(&f.header, h, FRect::new(card.x + g.pad, y, card.w - 2.0 * g.pad, g.header_h), argb(th.dim, 255), Align::Left);
                y += g.header_h;
            }
            Line::Row(i) => {
                let row = &v.rows[i];
                let selected = i == v.sel;
                if selected {
                    c.fill_round(FRect::new(card.x + px(10.0), y + px(2.0), card.w - px(20.0), g.row_h - px(4.0)), px(11.0), argb(th.accent, 255));
                }
                // icono
                let side = px(36.0);
                let ir = FRect::new(card.x + g.pad - px(2.0), y + (g.row_h - side) / 2.0, side, side);
                draw_icon(c, f, row, v.icons.get(i).copied().flatten(), ir);
                if selected {
                    c.stroke_round(ir.grow(-0.75), px(8.0), 1.5, argb(0xFFFFFF, 150));
                }
                // título y subtítulo
                let text_x = ir.x + side + px(14.0);
                let hint_w = px(110.0);
                let name_w = card.x + card.w - hint_w - px(40.0) - text_x;
                let (c1, c2) = if selected { (argb(th.accent_text, 255), argb(th.accent_text, 190)) } else { (argb(th.fg, 255), argb(th.dim, 255)) };
                if row.subtitle.is_empty() {
                    c.text(&f.row, &row.title, FRect::new(text_x, y, name_w, g.row_h), c1, Align::Left);
                } else {
                    c.text(&f.row, &row.title, FRect::new(text_x, y + px(5.0), name_w, g.row_h / 2.0), c1, Align::Left);
                    c.text(&f.sub, &row.subtitle, FRect::new(text_x, y + g.row_h / 2.0 - px(1.0), name_w, g.row_h / 2.0 - px(4.0)), c2, Align::Left);
                }
                // etiqueta a la derecha (+ ↵ en la seleccionada)
                let right = card.x + card.w - px(26.0);
                let mut hx = right;
                if selected {
                    c.text(&f.hint, "↵", FRect::new(right - px(18.0), y, px(18.0), g.row_h), argb(th.accent_text, 210), Align::Right);
                    hx -= px(24.0);
                }
                if !row.hint.is_empty() {
                    let col = if selected { argb(th.accent_text, 200) } else { argb(th.dim, 255) };
                    c.text(&f.hint, &row.hint, FRect::new(hx - hint_w, y, hint_w, g.row_h), col, Align::Right);
                }
                y += g.row_h;
            }
        }
    }
}

fn draw_icon(c: &mut Canvas, f: &Fonts, row: &Row, icon: Option<&Icon>, r: FRect) {
    if let Some(icon) = icon {
        c.draw_icon(icon, r);
        return;
    }
    match &row.icon {
        RowIcon::Glyph(ch, rgb) => {
            c.fill_round(r, px(8.5), argb(rgb_to_colorref(*rgb), 255));
            c.text(&f.glyph, &ch.to_string(), r, argb(0xFFFFFF, 255), Align::Center);
        }
        _ => {
            c.fill_round(r, px(8.5), argb(tile_color(&row.title), 255));
            let letter: String = row.title.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
            c.text(&f.letter, &letter, r, argb(0xFFFFFF, 255), Align::Center);
        }
    }
}

// ---------- ventana y estado ----------

/// Dominios que se están descargando o que ya fallaron (no se reintentan).
static FETCHING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Descarga el favicon de `host` en otro hilo y avisa a la paleta cuando llega.
fn fetch_favicon_in_background(host: String, hwnd: isize) {
    {
        let mut f = FETCHING.lock().unwrap_or_else(|e| e.into_inner());
        if f.contains(&host) {
            return;
        }
        f.push(host.clone());
    }
    let _ = std::thread::Builder::new().name("hypr-win-favicon".into()).spawn(move || {
        if crate::webapp::cached_favicon(&host).is_some() {
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut std::ffi::c_void)), WM_ICON_READY, WPARAM(0), LPARAM(0));
            }
        }
    });
}

struct Palette {
    hwnd: HWND,
    theme: Theme,
    fonts: Fonts,
    handler: Option<Box<dyn Handler>>,
    query: String,
    rows: Vec<Row>,
    sel: usize,
    top: usize,
    prev_fg: isize,
    mon: Rect,
    canvas: Option<Canvas>,
    icons: HashMap<String, Option<Icon>>,
}

thread_local! {
    static P: RefCell<Option<Palette>> = const { RefCell::new(None) };
}

/// Ejecuta `f` si el estado no está prestado (los mensajes pueden ser reentrantes).
fn with<R>(f: impl FnOnce(&mut Palette) -> R) -> Option<R> {
    P.with(|p| p.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn create(cfg: &crate::config::Bar) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = Fonts::new(&theme).ok_or("no se pudo crear la fuente de la paleta")?;
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
            return Err("RegisterClassExW (paleta) falló".into());
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
        PALETTE_HWND.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
        P.with(|s| {
            *s.borrow_mut() = Some(Palette {
                hwnd,
                theme,
                fonts,
                handler: None,
                query: String::new(),
                rows: Vec::new(),
                sel: 0,
                top: 0,
                prev_fg: 0,
                mon: Rect::new(0, 0, 1920, 1080),
                canvas: None,
                icons: HashMap::new(),
            })
        });
    }
    Ok(())
}

pub fn is_open() -> bool {
    with(|p| unsafe { IsWindowVisible(p.hwnd).as_bool() }).unwrap_or(false)
}

/// Abre la paleta con `handler` en el monitor `mon`.
pub fn open(handler: Box<dyn Handler>, mon: Rect) {
    let Some(hwnd) = with(|p| {
        p.handler = Some(handler);
        p.query.clear();
        p.mon = mon;
        p.prev_fg = window::foreground();
        p.reload_rows();
        p.sync_capture();
        p.redraw();
        p.hwnd
    }) else {
        return;
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    window::focus(window::id(hwnd));
}

pub fn close(refocus: bool) {
    let Some((hwnd, prev)) = with(|p| (p.hwnd, p.prev_fg)) else { return };
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    keybinds::set_capture(false);
    let _ = with(|p| p.handler = None);
    if refocus && prev != 0 && window::is_alive(prev) {
        window::focus(prev);
    }
}

/// Pide (desde cualquier hilo) que la paleta vuelva a calcular sus filas.
pub fn refresh_rows_async() {
    let h = PALETTE_HWND.load(std::sync::atomic::Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut std::ffi::c_void)), WM_ROWS_READY, WPARAM(0), LPARAM(0));
        }
    }
}

/// Combinación capturada por el hook de teclado (modo captura de atajos).
pub fn on_capture(vk: u32, mods: u32) {
    let _ = with(|p| {
        let out = p.handler.as_mut().map(|h| h.capture(vk, mods));
        if let Some(o) = out {
            p.apply(o);
        }
    });
    finish_apply();
}

/// Tras un `apply` que pidió cerrar, cierra fuera del préstamo del estado.
fn finish_apply() {
    if WANT_CLOSE.with(|c| c.replace(false)) {
        close(false);
    }
}

thread_local! {
    static WANT_CLOSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Palette {
    fn handler(&mut self) -> &mut dyn Handler {
        self.handler.as_deref_mut().expect("paleta sin manejador")
    }

    fn show_list(&self) -> bool {
        !self.query.is_empty() || self.handler.as_ref().is_some_and(|h| h.show_on_empty())
    }

    fn reload_rows(&mut self) {
        let q = self.query.clone();
        let rows = match self.handler.as_mut() {
            Some(h) => h.rows(&q),
            None => Vec::new(),
        };
        self.rows = rows;
        self.sel = 0;
        self.top = 0;
        self.ensure_icons();
    }

    /// Pide los iconos reales de las filas visibles que aún no están en la caché.
    fn ensure_icons(&mut self) {
        let wanted: Vec<RowIcon> = self.rows.iter().skip(self.top).take(MAX_ROWS).map(|r| r.icon.clone()).collect();
        let hwnd = self.hwnd.0 as isize;
        for icon in wanted {
            match icon {
                RowIcon::App(id) => {
                    self.icons.entry(id.clone()).or_insert_with(|| icons::from_apps_folder(&id));
                }
                RowIcon::Favicon(host) => {
                    let key = format!("fav:{host}");
                    if self.icons.contains_key(&key) {
                        continue;
                    }
                    match crate::webapp::favicon_if_cached(&host) {
                        Some(path) => {
                            self.icons.insert(key, icons::from_image_file(&path.to_string_lossy()));
                        }
                        None => fetch_favicon_in_background(host, hwnd),
                    }
                }
                _ => {}
            }
        }
    }

    fn sync_capture(&mut self) {
        let capturing = self.handler.as_ref().is_some_and(|h| h.capturing());
        keybinds::set_capture(capturing);
    }

    fn apply(&mut self, out: Outcome) {
        match out {
            Outcome::Close => WANT_CLOSE.with(|c| c.set(true)),
            Outcome::Refresh => {
                self.query.clear();
                self.reload_rows();
            }
            Outcome::Stay => {}
            Outcome::Rerun => {
                let sel = self.sel;
                self.reload_rows();
                self.sel = sel.min(self.rows.len().saturating_sub(1));
            }
            Outcome::Switch(h) => {
                self.handler = Some(h);
                self.query.clear();
                self.reload_rows();
            }
        }
        self.sync_capture();
        self.redraw();
    }

    fn move_sel(&mut self, delta: i32) {
        if self.rows.is_empty() {
            return;
        }
        let last = self.rows.len() as i32 - 1;
        self.sel = (self.sel as i32 + delta).clamp(0, last) as usize;
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + MAX_ROWS {
            self.top = self.sel + 1 - MAX_ROWS;
        }
        self.ensure_icons();
    }

    fn redraw(&mut self) {
        let Some(handler) = self.handler.as_ref() else { return };
        let (placeholder, crumb, empty_msg) = (handler.placeholder(), handler.crumb(), handler.empty_message());
        let show_list = self.show_list();
        let icons: Vec<Option<&Icon>> = self
            .rows
            .iter()
            .map(|r| match &r.icon {
                RowIcon::App(id) => self.icons.get(id).and_then(|i| i.as_ref()),
                RowIcon::Favicon(h) => self.icons.get(&format!("fav:{h}")).and_then(|i| i.as_ref()),
                _ => None,
            })
            .collect();
        let view = View {
            crumb: &crumb,
            placeholder: &placeholder,
            query: &self.query,
            rows: &self.rows,
            icons: &icons,
            sel: self.sel,
            top: self.top,
            show_list,
            empty_msg: &empty_msg,
        };
        let (w, h) = canvas_size(&view);
        if self.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.canvas = Canvas::new(w, h);
        }
        let Some(canvas) = self.canvas.as_mut() else { return };
        render(canvas, &self.theme, &self.fonts, &view);
        let g = Geo::new();
        let x = self.mon.x + (self.mon.w - w) / 2;
        let y = self.mon.y + (self.mon.h as f32 * 0.18) as i32 - g.margin as i32;
        canvas.present(self.hwnd, Some((x, y)));
    }

    fn type_char(&mut self, ch: char) {
        self.query.push(ch);
        self.reload_rows();
        self.redraw();
    }

    fn backspace(&mut self) {
        // con texto: borrar una letra; con el campo vacío: subir un nivel (si lo hay)
        if self.query.pop().is_none() && !self.handler().back() {
            return;
        }
        self.reload_rows();
        self.sync_capture();
        self.redraw();
    }

    fn escape(&mut self) -> bool {
        if !self.query.is_empty() {
            self.query.clear();
            self.reload_rows();
            self.redraw();
            true
        } else if self.handler().back() {
            self.reload_rows();
            self.sync_capture();
            self.redraw();
            true
        } else {
            false
        }
    }

    fn activate(&mut self) {
        if self.rows.is_empty() {
            return;
        }
        let q = self.query.clone();
        let idx = self.sel;
        let out = self.handler().activate(idx, &q);
        self.apply(out);
    }

    fn expand_or_move(&mut self, delta: i32) {
        if self.query.is_empty() && !self.show_list() && self.handler().expand() {
            self.reload_rows();
            self.redraw();
        } else {
            self.move_sel(delta);
            self.redraw();
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_ROWS_READY => {
            let _ = with(|p| {
                if p.handler.is_none() {
                    return;
                }
                // conserva la selección y el desplazamiento aunque cambie el número de filas
                let (sel, top) = (p.sel, p.top);
                p.reload_rows();
                let last = p.rows.len().saturating_sub(1);
                p.sel = sel.min(last);
                p.top = top.min(last);
                p.ensure_icons();
                p.redraw();
            });
            LRESULT(0)
        }
        WM_ICON_READY => {
            let _ = with(|p| {
                // los iconos que fallaron se pidieron como `None`: se vuelven a mirar ahora que hay archivo
                p.icons.retain(|k, v| !(k.starts_with("fav:") && v.is_none()));
                p.ensure_icons();
                p.redraw();
            });
            LRESULT(0)
        }
        WM_ACTIVATE => {
            if wp.0 & 0xffff == 0 {
                close(false); // perdió el foco (clic fuera, Alt+Tab...)
            }
            LRESULT(0)
        }
        WM_CHAR => {
            let c = wp.0 as u32;
            let _ = with(|p| {
                if p.handler.as_ref().is_some_and(|h| h.capturing()) {
                    return;
                }
                match c {
                    0x08 => p.backspace(),
                    0x7f | 0x15 => {
                        p.query.clear();
                        p.reload_rows();
                        p.redraw();
                    }
                    c if c >= 0x20 => {
                        if let Some(ch) = char::from_u32(c) {
                            p.type_char(ch);
                        }
                    }
                    _ => {}
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u32;
            let mut do_close = false;
            let _ = with(|p| {
                if p.handler.as_ref().is_some_and(|h| h.capturing()) {
                    return; // las teclas llegan por el hook
                }
                match vk {
                    0x1B => do_close = !p.escape(),
                    0x0D => p.activate(),
                    0x26 => p.expand_or_move(-1),
                    0x28 | 0x09 => p.expand_or_move(1),
                    0x21 => {
                        p.move_sel(-(MAX_ROWS as i32));
                        p.redraw();
                    }
                    0x22 => {
                        p.move_sel(MAX_ROWS as i32);
                        p.redraw();
                    }
                    _ => {}
                }
            });
            if do_close {
                close(true);
            }
            finish_apply();
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wp.0 >> 16) as i16;
            let _ = with(|p| {
                p.move_sel(if delta > 0 { -1 } else { 1 });
                p.redraw();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
            let _ = with(|p| {
                if let Some(i) = row_at(&p.rows, p.top, y - list_top()) {
                    p.sel = i;
                    p.activate();
                }
            });
            finish_apply();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
