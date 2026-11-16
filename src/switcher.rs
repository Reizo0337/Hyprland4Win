//! Alt+Tab propio: una tira de tarjetas (icono y título de cada ventana, por uso reciente) en el centro del
//! monitor. El hook de teclado (`keybinds`) traga el Alt+Tab de Windows y manda aquí las teclas; esta ventana
//! nunca coge el foco, así que la app activa no se entera. Si se suelta Alt enseguida (un toque rápido) se
//! cambia de ventana sin llegar a mostrar nada.

use std::cell::RefCell;
use std::collections::HashMap;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::Bar;
use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::icons::{self, Icon};
use crate::layout::Rect;

pub const CLASS_NAME: &str = "HyprWinSwitcher";
/// Tiempo que hay que mantener Alt antes de que aparezca la lista.
const SHOW_DELAY_MS: u32 = 110;
const TIMER_SHOW: usize = 1;

/// Una ventana de la lista (lo que el programa sabe de ella).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemSpec {
    pub title: String,
    /// Nombre de la app (para buscar un icono propio del usuario).
    pub name: String,
    pub exe: Option<String>,
    pub aumid: Option<String>,
    /// Está minimizada: se restaurará al elegirla.
    pub minimized: bool,
}

impl ItemSpec {
    fn icon_key(&self) -> String {
        self.aumid.clone().or_else(|| self.exe.clone()).unwrap_or_else(|| self.name.clone())
    }
}

pub struct ItemView<'a> {
    pub title: &'a str,
    pub icon: Option<&'a Icon>,
    pub minimized: bool,
}

pub struct Fonts {
    label: Font,
    letter: Font,
}

impl Fonts {
    pub fn new(th: &Theme) -> Option<Fonts> {
        let b = th.base_px();
        Some(Fonts { label: Font::new(&th.font, b * 0.95, false)?, letter: Font::new(&th.font, b * 2.2, true)? })
    }
}

/// Medidas en px reales.
pub struct Metrics {
    card_w: f32,
    card_h: f32,
    icon: f32,
    gap: f32,
    pad: f32,
    footer: f32,
}

impl Metrics {
    pub fn new() -> Metrics {
        Metrics { card_w: px(128.0), card_h: px(116.0), icon: px(56.0), gap: px(8.0), pad: px(16.0), footer: px(36.0) }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Columnas de la rejilla: caben en el 86 % del ancho del monitor (como mucho 10).
pub fn columns(n: usize, mon_w: i32, m: &Metrics) -> usize {
    let usable = mon_w as f32 * 0.86 - 2.0 * m.pad + m.gap;
    let fit = (usable / (m.card_w + m.gap)).floor().max(1.0) as usize;
    n.clamp(1, fit.min(10))
}

/// Tamaño (px) del lienzo para `n` ventanas en `cols` columnas.
pub fn canvas_size(n: usize, cols: usize, m: &Metrics) -> (i32, i32) {
    let rows = n.div_ceil(cols.max(1)).max(1);
    let w = cols as f32 * m.card_w + (cols.saturating_sub(1)) as f32 * m.gap + 2.0 * m.pad;
    let h = rows as f32 * m.card_h + (rows - 1) as f32 * m.gap + 2.0 * m.pad + m.footer;
    (w.ceil() as i32, h.ceil() as i32)
}

/// Índice tras moverse `delta` posiciones, dando la vuelta por los dos extremos.
pub fn step_index(len: usize, sel: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (sel as i64 + delta as i64).rem_euclid(len as i64) as usize
}

/// Dibuja la lista (sin Win32: se puede renderizar a un PNG).
pub fn render(c: &mut Canvas, th: &Theme, f: &Fonts, m: &Metrics, items: &[ItemView], sel: usize, cols: usize) {
    c.clear();
    let (w, h) = (c.w as f32, c.h as f32);
    c.card(FRect::new(0.0, 0.0, w, h), px(20.0), th.bg, 0xE6, th.fg);
    let radius = px(14.0);
    for (i, it) in items.iter().enumerate() {
        let (col, row) = (i % cols, i / cols);
        let card = FRect::new(m.pad + col as f32 * (m.card_w + m.gap), m.pad + row as f32 * (m.card_h + m.gap), m.card_w, m.card_h);
        if i == sel {
            c.fill_round(card, radius, argb(th.accent, 70));
            c.stroke_round(card.grow(-1.0), radius, 2.0, argb(th.accent, 255));
        }
        let icon = FRect::new(card.x + (card.w - m.icon) / 2.0, card.y + px(14.0), m.icon, m.icon);
        match it.icon {
            Some(ic) => c.draw_icon(ic, icon),
            None => {
                c.fill_round(icon, m.icon * 0.22, argb(crate::palette::tile_color(it.title), 255));
                let letter: String = it.title.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
                c.text(&f.letter, &letter, icon, argb(0xFFFFFF, 255), Align::Center);
            }
        }
        let label = FRect::new(card.x + px(8.0), icon.y + m.icon + px(6.0), card.w - px(16.0), px(30.0));
        // las minimizadas, atenuadas
        let alpha = match (i == sel, it.minimized) {
            (true, false) => 255,
            (true, true) => 200,
            (false, false) => 200,
            (false, true) => 120,
        };
        c.text(&f.label, it.title, label, argb(th.fg, alpha), Align::Center);
    }
    // título completo de la ventana elegida
    if let Some(it) = items.get(sel) {
        let footer = FRect::new(m.pad, h - m.footer - px(2.0), w - 2.0 * m.pad, m.footer);
        c.fill_round(FRect::new(m.pad, h - m.footer - px(4.0), w - 2.0 * m.pad, 1.0), 0.0, argb(th.fg, 28));
        let text = if it.minimized { format!("{} · minimizada", it.title) } else { it.title.to_string() };
        c.text(&f.label, &text, footer, argb(th.dim, 255), Align::Center);
    }
}

// ---------- ventana ----------

struct State {
    hwnd: HWND,
    theme: Theme,
    fonts: Fonts,
    items: Vec<ItemSpec>,
    sel: usize,
    mon: Rect,
    /// Hay un cambio de ventana en curso.
    active: bool,
    /// La lista ya está en pantalla.
    shown: bool,
    icons: HashMap<String, Option<Icon>>,
    canvas: Option<Canvas>,
}

thread_local! {
    static S: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    S.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn create(cfg: &Bar) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = Fonts::new(&theme).ok_or("no se pudo crear la fuente del cambio de ventana")?;
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
            return Err("RegisterClassExW (cambio de ventana) falló".into());
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
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
        S.with(|s| {
            *s.borrow_mut() = Some(State {
                hwnd,
                theme,
                fonts,
                items: Vec::new(),
                sel: 0,
                mon: Rect::new(0, 0, 1920, 1080),
                active: false,
                shown: false,
                icons: HashMap::new(),
                canvas: None,
            })
        });
    }
    Ok(())
}

/// Cambia colores y fuente sin reiniciar.
pub fn set_theme(cfg: &Bar) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = Fonts::new(&theme).ok_or("no se pudo crear la fuente del cambio de ventana")?;
    let _ = with(|s| {
        s.theme = theme;
        s.fonts = fonts;
        if s.shown {
            s.draw();
        }
    });
    Ok(())
}

/// Empieza un cambio de ventana: la lista aparece si se sigue en ello pasado un instante.
pub fn begin(items: Vec<ItemSpec>, sel: usize, mon: Rect) {
    let _ = with(|s| {
        s.items = items;
        s.sel = sel.min(s.items.len().saturating_sub(1));
        s.mon = mon;
        s.active = true;
        s.shown = false;
        unsafe {
            SetTimer(Some(s.hwnd), TIMER_SHOW, SHOW_DELAY_MS, None);
        }
    });
}

pub fn select(sel: usize) {
    let _ = with(|s| {
        s.sel = sel.min(s.items.len().saturating_sub(1));
        if s.shown {
            s.draw();
        }
    });
}

/// Cierra la lista (y deja de esperar para mostrarla).
pub fn end() {
    let _ = with(|s| {
        s.active = false;
        s.shown = false;
        unsafe {
            let _ = KillTimer(Some(s.hwnd), TIMER_SHOW);
            let _ = ShowWindow(s.hwnd, SW_HIDE);
        }
        s.canvas = None;
    });
    crate::mem::trim();
}

impl State {
    fn load_icons(&mut self) {
        for spec in &self.items {
            self.icons.entry(spec.icon_key()).or_insert_with(|| {
                icons::custom_for(&spec.name, spec.exe.as_deref())
                    .or_else(|| spec.aumid.as_deref().and_then(icons::from_apps_folder))
                    .or_else(|| spec.exe.as_deref().and_then(icons::from_path))
            });
        }
    }

    fn draw(&mut self) {
        let m = Metrics::new();
        let cols = columns(self.items.len(), self.mon.w, &m);
        let (w, h) = canvas_size(self.items.len(), cols, &m);
        if self.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.canvas = Canvas::new(w, h);
        }
        let State { items, icons, theme, fonts, canvas, sel, mon, hwnd, .. } = self;
        let Some(canvas) = canvas.as_mut() else { return };
        let views: Vec<ItemView> =
            items.iter().map(|it| ItemView { title: &it.title, icon: icons.get(&it.icon_key()).and_then(|i| i.as_ref()), minimized: it.minimized }).collect();
        render(canvas, theme, fonts, &m, &views, *sel, cols);
        let (x, y) = (mon.x + (mon.w - w) / 2, mon.y + (mon.h - h) / 2);
        canvas.present(*hwnd, Some((x, y)));
        unsafe {
            let _ = SetWindowPos(*hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    fn show(&mut self) {
        if !self.active || self.items.is_empty() {
            return;
        }
        self.load_icons();
        self.shown = true;
        self.draw();
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wp.0 == TIMER_SHOW => {
            let _ = KillTimer(Some(hwnd), TIMER_SHOW);
            let _ = with(|s| s.show());
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_wraps_in_both_directions() {
        assert_eq!(step_index(4, 3, 1), 0);
        assert_eq!(step_index(4, 0, -1), 3);
        assert_eq!(step_index(4, 1, 1), 2);
        assert_eq!(step_index(1, 0, 1), 0);
        assert_eq!(step_index(0, 0, 1), 0);
        assert_eq!(step_index(5, 2, -7), 0);
    }

    #[test]
    fn the_grid_fits_the_monitor() {
        let m = Metrics::new();
        // pocas ventanas: una columna por ventana
        assert_eq!(columns(3, 1920, &m), 3);
        assert_eq!(columns(0, 1920, &m), 1);
        // muchas: se parte en filas y nunca pasa del ancho útil
        let cols = columns(40, 1920, &m);
        assert!((2..=10).contains(&cols));
        let (w, h) = canvas_size(40, cols, &m);
        assert!(w as f32 <= 1920.0 * 0.86 + 1.0, "{w}");
        let (_, h_one) = canvas_size(cols, cols, &m);
        assert!(h > h_one, "más filas, más alto");
        // una pantalla estrecha sigue dando al menos una columna
        assert_eq!(columns(5, 300, &m), 1);
    }

    #[test]
    fn the_list_draws_selection_and_titles() {
        let th = crate::gfx::Theme::from_cfg(&crate::config::Config::default_config().bar).unwrap();
        let f = Fonts::new(&th).unwrap();
        let m = Metrics::new();
        let titles = ["Uno", "Dos", "Tres"];
        let views: Vec<ItemView> = titles.iter().map(|t| ItemView { title: t, icon: None, minimized: false }).collect();
        let (w, h) = canvas_size(3, 3, &m);
        let mut c = Canvas::new(w, h).unwrap();
        render(&mut c, &th, &f, &m, &views, 1, 3);
        assert!(c.pixels().iter().any(|&p| p >> 24 != 0));
        // la tarjeta elegida (la del medio) se pinta con el acento; la primera no
        let accent = (th.accent & 0xff) << 16 | (th.accent >> 8 & 0xff) << 8 | th.accent >> 16 & 0xff;
        let near = |px: u32| ((px >> 16 & 0xff) as i32 - (accent >> 16 & 0xff) as i32).abs() < 40 && ((px & 0xff) as i32 - (accent & 0xff) as i32).abs() < 40;
        let count = |x0: i32, x1: i32| {
            let mut n = 0;
            for y in 0..h {
                for x in x0..x1 {
                    let p = c.pixels()[(y * w + x) as usize];
                    if p >> 24 > 200 && near(p) {
                        n += 1;
                    }
                }
            }
            n
        };
        let third = w / 3;
        assert!(count(third, 2 * third) > 50, "la tarjeta elegida debería llevar el acento");
        assert!(count(0, third / 2) < 5, "las demás no");
    }
}
