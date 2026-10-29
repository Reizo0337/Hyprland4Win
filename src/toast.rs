//! Notificaciones flotantes al estilo de macOS (arriba a la derecha, bajo la barra): tarjeta con icono,
//! título y subtítulo. Se pueden publicar desde cualquier hilo (`upsert`/`dismiss`); la ventana las dibuja
//! en el hilo de la interfaz con GDI+ y alfa por píxel, sin robar el foco.

use std::cell::RefCell;
use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::Bar;
use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::layout::Rect;
use crate::palette::rgb_to_colorref;

pub const CLASS_NAME: &str = "HyprWinToast";
const WM_TOAST: u32 = WM_APP + 7;
const TIMER_EXPIRE: usize = 1;

/// Colores del sistema de Apple para cada tipo de aviso (0xRRGGBB).
pub const BLUE: u32 = 0x0A84FF;
pub const GREEN: u32 = 0x30D158;
pub const RED: u32 = 0xFF453A;

pub struct ToastView<'a> {
    pub glyph: char,
    pub rgb: u32,
    pub title: &'a str,
    pub subtitle: &'a str,
}

pub struct Fonts {
    title: Font,
    sub: Font,
    glyph: Font,
}

impl Fonts {
    pub fn new(th: &Theme) -> Option<Fonts> {
        let b = th.base_px();
        Some(Fonts {
            title: Font::new(&th.font, b * 1.05, true)?,
            sub: Font::new(&th.font, b * 0.9, false)?,
            glyph: Font::with_fallbacks(&["Segoe Fluent Icons", "Segoe MDL2 Assets"], b * 1.25, false)?,
        })
    }
}

struct Geo {
    margin: f32,
    card_w: f32,
    card_h: f32,
    gap: f32,
}

impl Geo {
    fn new() -> Geo {
        Geo { margin: px(24.0), card_w: px(360.0), card_h: px(68.0), gap: px(8.0) }
    }
}

/// Tamaño del lienzo para `n` tarjetas apiladas.
pub fn canvas_size(n: usize) -> (i32, i32) {
    let g = Geo::new();
    let h = n as f32 * g.card_h + n.saturating_sub(1) as f32 * g.gap;
    ((g.card_w + 2.0 * g.margin).ceil() as i32, (h + 2.0 * g.margin).ceil() as i32)
}

pub fn render(c: &mut Canvas, th: &Theme, f: &Fonts, toasts: &[ToastView]) {
    c.clear();
    let g = Geo::new();
    for (i, t) in toasts.iter().enumerate() {
        let card = FRect::new(g.margin, g.margin + i as f32 * (g.card_h + g.gap), g.card_w, g.card_h);
        c.card(card, px(16.0), th.bg, 0xF0, th.fg);
        let tile = FRect::new(card.x + px(14.0), card.y + (g.card_h - px(40.0)) / 2.0, px(40.0), px(40.0));
        c.fill_round(tile, px(10.0), argb(rgb_to_colorref(t.rgb), 255));
        c.text(&f.glyph, &t.glyph.to_string(), tile, argb(0xFFFFFF, 255), Align::Center);
        let tx = tile.x + tile.w + px(14.0);
        let w = card.x + card.w - tx - px(14.0);
        c.text(&f.title, t.title, FRect::new(tx, card.y + px(10.0), w, px(24.0)), argb(th.fg, 255), Align::Left);
        c.text(&f.sub, t.subtitle, FRect::new(tx, card.y + px(34.0), w, px(22.0)), argb(th.dim, 255), Align::Left);
    }
}

// ---------- API desde cualquier hilo ----------

enum Cmd {
    Upsert { id: u64, glyph: char, rgb: u32, title: String, sub: String, ttl: Option<Duration> },
    Dismiss(u64),
}

static QUEUE: Mutex<Vec<Cmd>> = Mutex::new(Vec::new());
static HWND_SLOT: AtomicIsize = AtomicIsize::new(0);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn post(cmd: Cmd) {
    QUEUE.lock().unwrap_or_else(|e| e.into_inner()).push(cmd);
    let h = HWND_SLOT.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut std::ffi::c_void)), WM_TOAST, WPARAM(0), LPARAM(0));
        }
    }
}

/// Muestra o actualiza la notificación `id`. `ttl` = cuánto dura (`None`: se queda hasta `dismiss`).
pub fn upsert(id: u64, glyph: char, rgb: u32, title: &str, sub: &str, ttl: Option<Duration>) {
    post(Cmd::Upsert { id, glyph, rgb, title: title.into(), sub: sub.into(), ttl });
}

pub fn dismiss(id: u64) {
    post(Cmd::Dismiss(id));
}

// ---------- ventana ----------

struct Toast {
    id: u64,
    glyph: char,
    rgb: u32,
    title: String,
    sub: String,
    expires: Option<Instant>,
}

struct State {
    hwnd: HWND,
    theme: Theme,
    fonts: Fonts,
    toasts: Vec<Toast>,
    mon: Rect,
    bar_h: i32,
    canvas: Option<Canvas>,
}

thread_local! {
    static S: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    S.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn create(cfg: &Bar, mon: Rect, bar_h: i32) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = Fonts::new(&theme).ok_or("no se pudo crear la fuente de las notificaciones")?;
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
            return Err("RegisterClassExW (notificaciones) falló".into());
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
        HWND_SLOT.store(hwnd.0 as isize, Ordering::Relaxed);
        S.with(|s| *s.borrow_mut() = Some(State { hwnd, theme, fonts, toasts: Vec::new(), mon, bar_h, canvas: None }));
    }
    Ok(())
}

impl State {
    fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Upsert { id, glyph, rgb, title, sub, ttl } => {
                let t = Toast { id, glyph, rgb, title, sub, expires: ttl.map(|d| Instant::now() + d) };
                match self.toasts.iter_mut().find(|x| x.id == id) {
                    Some(x) => *x = t,
                    None => self.toasts.push(t),
                }
            }
            Cmd::Dismiss(id) => self.toasts.retain(|t| t.id != id),
        }
    }

    fn redraw(&mut self) {
        if self.toasts.is_empty() {
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_EXPIRE);
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            return;
        }
        let (w, h) = canvas_size(self.toasts.len());
        if self.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.canvas = Canvas::new(w, h);
        }
        let Some(canvas) = self.canvas.as_mut() else { return };
        let views: Vec<ToastView> =
            self.toasts.iter().map(|t| ToastView { glyph: t.glyph, rgb: t.rgb, title: &t.title, subtitle: &t.sub }).collect();
        render(canvas, &self.theme, &self.fonts, &views);
        let g = Geo::new();
        let x = self.mon.right() - w + g.margin as i32 - px(16.0) as i32;
        let y = self.mon.y + self.bar_h + px(8.0) as i32 - g.margin as i32;
        canvas.present(self.hwnd, Some((x, y)));
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(self.hwnd), TIMER_EXPIRE, 400, None);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TOAST => {
            let cmds: Vec<Cmd> = std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()));
            let _ = with(|s| {
                for c in cmds {
                    s.apply(c);
                }
                s.redraw();
            });
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = with(|s| {
                let now = Instant::now();
                let before = s.toasts.len();
                s.toasts.retain(|t| t.expires.is_none_or(|e| e > now));
                if s.toasts.len() != before {
                    s.redraw();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // un clic descarta la tarjeta pulsada
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32;
            let _ = with(|s| {
                let g = Geo::new();
                let i = ((y - g.margin) / (g.card_h + g.gap)).floor();
                if i >= 0.0 && (i as usize) < s.toasts.len() {
                    s.toasts.remove(i as usize);
                    s.redraw();
                }
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
