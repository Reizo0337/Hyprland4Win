//! Barras superiores (una por monitor), al estilo de la barra de menús de macOS: translúcida, con los
//! workspaces como puntos (la pastilla activa se desliza, como el indicador de páginas de iOS), el título
//! de la ventana enfocada y la fecha y hora. Cada barra es una ventana registrada como appbar y
//! dibujada con GDI+ (`gfx`) con transparencia por píxel.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Globalization::{GetDateFormatEx, GetTimeFormatEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::Bar;
use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::glyphs;
use crate::status::{self, Status};
use crate::layout::Rect;
use crate::window::MonitorInfo;

pub const CLASS_NAME: &str = "HyprWinBar";
/// Mensaje de callback del appbar (ABN_POSCHANGED, etc.).
pub const WM_APPBAR: u32 = WM_APP + 2;
const ABN_POSCHANGED: usize = 1;
/// Alto fijo de la vista previa (px).
pub const BAR_PREVIEW_HEIGHT: i32 = 28;
pub const TIMER_CLOCK: usize = 1;
pub const TIMER_MOUSE: usize = 3;
const TIMER_BAR: usize = 4;
pub const TIMER_SETTLE: usize = 5;
pub const TIMER_TASKBAR: usize = 6;
const TIMER_STATUS: usize = 7;
pub const TIMER_WALLPAPER: usize = 8;
pub const TIMER_WIDGETS: usize = 9;
pub const TIMER_LIVEWALL: usize = 11;
/// Animación de la música (solo corre mientras suena algo).
const TIMER_MUSIC: usize = 10;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

/// Separación entre puntos y margen izquierdo, a 96 DPI.
const SLOT: f32 = 18.0;
const LEFT_PAD: f32 = 16.0;

/// Datos de una barra para dibujarla (sin Win32: se puede renderizar a un PNG).
pub struct BarView<'a> {
    pub occupied: &'a [bool],
    /// Posición fraccionaria (en puntos) de la pastilla activa.
    pub hl: f32,
    /// Esta barra está en el monitor enfocado.
    pub focused: bool,
    pub title: &'a str,
    pub right: &'a str,
    pub status: &'a Status,
    /// Estilo "bubbles": cada grupo en su propia burbuja y sin título de ventana.
    pub bubbles: bool,
    /// Estilo "ink": numerales kanji, sello rojo en el workspace activo y filo carmesí.
    pub ink: bool,
    /// Lo que suena ahora (si suena algo): se muestra en el centro.
    pub music: Option<MusicView<'a>>,
}

/// Lo que suena y el instante de la animación.
pub struct MusicView<'a> {
    pub title: &'a str,
    /// Segundos desde que empezó a sonar (mueve la onda).
    pub t: f32,
}

/// Barras de la onda: alto relativo (0-1) de la barra `i` en el instante `t`.
fn wave_level(i: usize, t: f32) -> f32 {
    let v = 0.5 + 0.5 * (t * (5.0 + 1.7 * i as f32) + i as f32 * 1.3).sin();
    0.28 + 0.72 * v
}

/// Pastilla de la música: onda animada + título.
fn draw_music(c: &mut Canvas, th: &Theme, font: &Font, m: &MusicView, rect: FRect, fill: Option<u32>) {
    let h = rect.h;
    if let Some(f) = fill {
        c.fill_round(rect, h / 2.0, f);
    }
    // onda
    let pad = px(12.0);
    let (bw, gap, n) = (px(2.2), px(1.6), 5usize);
    let wave_w = n as f32 * bw + (n - 1) as f32 * gap;
    let (wave_h, cy) = (h * 0.5, rect.y + h / 2.0);
    for i in 0..n {
        let bh = (wave_h * wave_level(i, m.t)).max(bw);
        let x = rect.x + pad + i as f32 * (bw + gap);
        c.fill_round(FRect::new(x, cy - bh / 2.0, bw, bh), bw / 2.0, argb(th.accent, 255));
    }
    let tx = rect.x + pad + wave_w + px(8.0);
    c.text(font, m.title, FRect::new(tx, rect.y, (rect.x + rect.w - pad - tx).max(0.0), h), argb(th.fg, 255), Align::Left);
}

/// Ancho natural de la pastilla de la música (sin recortar).
fn music_width(c: &mut Canvas, font: &Font, title: &str) -> f32 {
    let (bw, gap, n) = (px(2.2), px(1.6), 5.0);
    2.0 * px(12.0) + n * bw + (n - 1.0) * gap + px(8.0) + c.measure(font, title) + px(2.0)
}

/// Margen vertical de las burbujas, relleno interior y separación entre burbujas (px a 96 DPI).
/// (1,5 px por arriba y por abajo: la burbuja mide 3 px más que con 3.)
const BUBBLE_MARGIN: f32 = 1.5;
const BUBBLE_INSET: f32 = 10.0;
const BUBBLE_GAP: f32 = 6.0;
/// Separación de la primera y la última burbuja al borde de la pantalla.
const BUBBLE_EDGE: f32 = 6.0;
/// Separación entre los puntos de workspaces y entre los iconos de estado dentro de las burbujas.
const BUBBLE_SLOT: f32 = 18.0;
const BUBBLE_ITEM_GAP: f32 = 16.0;

/// Numeral kanji del workspace `n` (1 = 一 … 10 = 十); a partir de 11 se usan cifras.
pub fn kanji(n: usize) -> String {
    const K: [&str; 10] = ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"];
    match n {
        1..=10 => K[n - 1].to_string(),
        _ => n.to_string(),
    }
}

/// Posición inicial y separación (px) de los puntos de workspaces.
fn workspace_metrics(bubbles: bool) -> (f32, f32) {
    if bubbles {
        (px(BUBBLE_EDGE + BUBBLE_INSET), px(BUBBLE_SLOT))
    } else {
        (px(LEFT_PAD), px(SLOT))
    }
}

/// Separación entre los iconos de estado.
fn item_gap(bubbles: bool) -> f32 {
    px(if bubbles { BUBBLE_ITEM_GAP } else { 16.0 })
}

/// Ancho (px) de los iconos de estado con sus separaciones, sin el relleno de la burbuja.
fn status_width(c: &mut Canvas, font: &Font, s: &Status, icon: f32, gap: f32) -> f32 {
    let mut items = Vec::new();
    if let Some(b) = s.battery {
        items.push(c.measure(font, &format!("{}%", b.percent)) + px(5.0) + px(25.0));
    }
    if s.volume.is_some() {
        items.push(icon);
    }
    if s.wifi_lit().is_some() {
        items.push(icon);
    }
    if s.bluetooth.present {
        items.push(icon * 0.8);
    }
    items.iter().sum::<f32>() + gap * items.len().saturating_sub(1) as f32
}

/// Dibuja la barra. Devuelve el tramo horizontal (px) de los iconos de estado, que abre el panel al hacer clic.
pub fn render(c: &mut Canvas, th: &Theme, font: &Font, opacity: u8, v: &BarView) -> (f32, f32) {
    c.clear();
    let (w, h) = (c.w as f32, c.h as f32);
    let alpha = (opacity as u32 * 255 / 100) as u8;
    let fill = argb(th.bg, alpha);
    let (margin, inset) = (px(BUBBLE_MARGIN), px(BUBBLE_INSET));
    let bubble_h = h - 2.0 * margin;
    // en "bubbles" no hay franja de fondo: cada grupo es una pastilla con su propio relleno
    let bubble = |c: &mut Canvas, x: f32, width: f32| c.fill_round(FRect::new(x, margin, width, bubble_h), bubble_h / 2.0, fill);
    if !v.bubbles {
        c.fill_round(FRect::new(0.0, 0.0, w, h), 0.0, fill);
        if v.ink {
            // filo carmesí que se desvanece hacia la derecha, como una pincelada
            // tramos contiguos sin solaparse (si se solapan, el alfa se suma y quedan muescas)
            let steps = 24;
            for i in 0..steps {
                let (x0, x1) = ((w * i as f32 / steps as f32).round(), (w * (i + 1) as f32 / steps as f32).round());
                let a = (200.0 * (1.0 - i as f32 / steps as f32).powf(1.4)) as u8;
                c.fill_round(FRect::new(x0, h - 2.0, x1 - x0, 2.0), 0.0, argb(th.accent, a));
            }
        } else {
            c.fill_round(FRect::new(0.0, h - 1.0, w, 1.0), 0.0, argb(th.fg, 28));
        }
    }

    // puntos de workspaces + pastilla activa deslizante
    let ((x0, slot), cy) = (workspace_metrics(v.bubbles), h / 2.0);
    let gap = item_gap(v.bubbles);
    let dot = px(6.0);
    if v.bubbles {
        bubble(c, x0 - inset, v.occupied.len() as f32 * slot + 2.0 * inset);
    }
    if v.ink {
        // sello (hanko) que se desliza al workspace activo y numerales kanji
        let seal = px(17.0);
        let cx = x0 + v.hl * slot + slot / 2.0;
        let color = if v.focused { argb(th.accent, 255) } else { argb(th.fg, 150) };
        c.fill_round(FRect::new(cx - seal / 2.0, cy - seal / 2.0, seal, seal), px(3.0), color);
        for (i, &occ) in v.occupied.iter().enumerate() {
            let cell = FRect::new(x0 + i as f32 * slot, cy - seal / 2.0 - px(1.0), slot, seal + px(2.0));
            let under_seal = (i as f32 - v.hl).abs() < 0.5;
            let ink_col = if under_seal { argb(th.accent_text, 255) } else { argb(th.fg, if occ { 230 } else { 130 }) };
            c.text(font, &kanji(i + 1), cell, ink_col, Align::Center);
        }
    } else {
        for (i, &occ) in v.occupied.iter().enumerate() {
            let cx = x0 + i as f32 * slot + slot / 2.0;
            let a = if occ { 170 } else { 70 };
            c.fill_ellipse(FRect::new(cx - dot / 2.0, cy - dot / 2.0, dot, dot), argb(th.fg, a));
        }
        let cx = x0 + v.hl * slot + slot / 2.0;
        let pill_w = px(16.0);
        let color = if v.focused { argb(th.accent, 255) } else { argb(th.fg, 200) };
        c.fill_round(FRect::new(cx - pill_w / 2.0, cy - dot / 2.0, pill_w, dot), dot / 2.0, color);
    }

    // fecha y hora a la derecha
    let right_w = c.measure(font, v.right);
    let right_pad = if v.bubbles { px(BUBBLE_EDGE + BUBBLE_INSET) } else { px(LEFT_PAD) };
    let right = FRect::new(w - right_pad - right_w - px(4.0), 0.0, right_w + px(4.0), h);
    if v.bubbles {
        bubble(c, right.x - inset, w - px(BUBBLE_EDGE) - (right.x - inset));
    }
    c.text(font, v.right, right, argb(th.fg, 255), Align::Right);

    // iconos de estado a la izquierda del reloj: Wi-Fi, volumen, batería
    let icon = px(15.0);
    let iy = (h - icon) / 2.0;
    let status_w = status_width(c, font, v.status, icon, gap);
    let clock_left = if v.bubbles { right.x - inset } else { right.x };
    let sb_right = clock_left - px(BUBBLE_GAP);
    let mut x = if v.bubbles {
        if status_w > 0.0 {
            bubble(c, sb_right - status_w - 2.0 * inset, status_w + 2.0 * inset);
        }
        sb_right - inset
    } else {
        right.x - px(18.0)
    };
    let cluster_end = x + px(8.0);
    if let Some(b) = v.status.battery {
        let label = format!("{}%", b.percent);
        let lw = c.measure(font, &label);
        c.text(font, &label, FRect::new(x - lw, 0.0, lw + px(2.0), h), argb(th.fg, 255), Align::Right);
        let (bw, bh) = (px(25.0), px(11.5));
        x -= lw + px(5.0) + bw;
        glyphs::battery(c, &th.icons, FRect::new(x, (h - bh) / 2.0, bw, bh), b.percent, b.charging, th.fg);
        x -= gap;
    }
    if let Some(vol) = v.status.volume {
        x -= icon;
        glyphs::speaker(c, &th.icons, x, iy, icon, vol.level, vol.muted, th.fg);
        x -= gap;
    }
    if let Some(lit) = v.status.wifi_lit() {
        x -= icon;
        glyphs::wifi(c, &th.icons, x, iy, icon, lit, !v.status.wifi.enabled, th.fg);
        x -= gap;
    }
    if v.status.bluetooth.present {
        x -= icon * 0.8;
        glyphs::bluetooth(c, &th.icons, x, iy, icon, th.fg, if v.status.bluetooth.on { 255 } else { 80 });
    }
    let cluster = if v.bubbles { (sb_right - status_w - 2.0 * inset, sb_right) } else { (x - px(8.0), cluster_end) };

    // lo que suena, en el centro: entre lo que haya a la izquierda y los iconos de estado
    let tx = x0 + v.occupied.len() as f32 * slot + px(14.0);
    let mut title_limit = cluster.0 - px(20.0);
    if let Some(m) = &v.music {
        let left_end = if v.bubbles { x0 + v.occupied.len() as f32 * slot + inset + px(BUBBLE_GAP * 2.0) } else { tx + px(80.0) };
        let right_start = cluster.0 - px(if v.bubbles { BUBBLE_GAP * 2.0 } else { 12.0 });
        let width = music_width(c, font, m.title).min(w * 0.34).min(right_start - left_end);
        if width >= px(110.0) {
            let height = if v.bubbles { bubble_h } else { h - 2.0 * px(4.0) };
            let cx = (w / 2.0).clamp(left_end + width / 2.0, right_start - width / 2.0);
            let rect = FRect::new(cx - width / 2.0, (h - height) / 2.0, width, height);
            draw_music(c, th, font, m, rect, Some(if v.bubbles { fill } else { argb(th.fg, 14) }));
            title_limit = title_limit.min(rect.x - px(14.0));
        }
    }

    // título entre los puntos de workspaces y los iconos de estado (la burbuja no lo muestra)
    if !v.bubbles {
        c.text(font, v.title, FRect::new(tx, 0.0, (title_limit - tx).max(0.0), h), argb(th.fg, 255), Align::Left);
    }
    cluster
}

/// "mié 28 oct  19:45" según la configuración regional del usuario.
pub fn clock_text() -> String {
    let now = unsafe { GetLocalTime() };
    let mut date = [0u16; 64];
    let mut time = [0u16; 32];
    unsafe {
        let n = GetDateFormatEx(PCWSTR::null(), Default::default(), Some(&now), w!("ddd d MMM"), Some(&mut date), PCWSTR::null());
        let m = GetTimeFormatEx(PCWSTR::null(), Default::default(), Some(&now), w!("HH:mm"), Some(&mut time));
        let s = |buf: &[u16], len: i32| String::from_utf16_lossy(&buf[..(len - 1).max(0) as usize]);
        format!("{}  {}", s(&date, n), s(&time, m))
    }
}

struct BarWin {
    hwnd: HWND,
    /// Monitor al que pertenece (`MonitorInfo::id`).
    id: u64,
    mon: Rect,
    active: usize,
    /// Posición (en puntos, fraccionaria) de la pastilla, que se desliza hacia `active`.
    hl: f32,
    title: String,
    /// Tramo horizontal de los iconos de estado (clic = abrir el panel).
    cluster: (f32, f32),
    canvas: Option<Canvas>,
}

struct State {
    theme: Theme,
    font: Font,
    opacity: u8,
    height: i32,
    occupied: Vec<bool>,
    focused_mon: usize,
    status: Status,
    bubbles: bool,
    ink: bool,
    /// Título de lo que suena ahora (`None` si no suena nada) y desde cuándo.
    music: Option<String>,
    music_since: std::time::Instant,
    bars: Vec<BarWin>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn dpi_scale(v: i32) -> i32 {
    px(v as f32).round() as i32
}

/// Ventana que recibe los hotkeys, eventos y temporizadores (la barra del primer monitor).
pub fn hwnd() -> HWND {
    HWND(MAIN_HWND.load(Ordering::Relaxed) as *mut c_void)
}

pub fn height_px() -> i32 {
    STATE.with(|s| s.borrow().as_ref().map_or(28, |s| s.height))
}

/// Crea la ventana oculta que recibe atajos, eventos y temporizadores, y una barra por monitor (aún sin
/// registrar como appbar). Devuelve la ventana principal, que no depende de ningún monitor.
pub fn create_all(cfg: &Bar, workspaces: usize, mons: &[MonitorInfo]) -> Result<HWND, String> {
    let theme = Theme::from_cfg(cfg)?;
    let font = Font::new(&theme.font, theme.base_px(), false).ok_or("no se pudo crear la fuente de la barra")?;
    let height = dpi_scale(cfg.height);
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            theme,
            font,
            opacity: cfg.opacity,
            height,
            occupied: vec![false; workspaces],
            focused_mon: 0,
            status: status::poll(),
            bubbles: cfg.style == "bubbles",
            ink: cfg.style == "ink",
            music: None,
            music_since: std::time::Instant::now(),
            bars: Vec::new(),
        })
    });

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
            return Err("RegisterClassExW falló".into());
        }
        // Ventana principal: oculta y de nivel superior (así recibe los avisos de cambio de pantalla).
        let main = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, PCWSTR(class.as_ptr()), w!("hypr-win"), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None)
            .map_err(|e| e.to_string())?;
        MAIN_HWND.store(main.0 as isize, Ordering::Relaxed);
        SetTimer(Some(main), TIMER_CLOCK, 10_000, None);
        SetTimer(Some(main), TIMER_STATUS, 3_000, None);
        for (i, m) in mons.iter().enumerate() {
            let bar = new_bar(m, i, height)?;
            STATE.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    st.bars.push(bar);
                }
            });
        }
        Ok(main)
    }
}

/// Crea la ventana de la barra de un monitor.
fn new_bar(m: &MonitorInfo, index: usize, height: i32) -> Result<BarWin, String> {
    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = HSTRING::from(CLASS_NAME);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            PCWSTR(class.as_ptr()),
            w!("hypr-win bar"),
            WS_POPUP,
            m.rect.x,
            m.rect.y,
            m.rect.w,
            height,
            None,
            None,
            Some(inst.into()),
            None,
        )
        .map_err(|e| e.to_string())?;
        Ok(BarWin { hwnd, id: m.id, mon: m.rect, active: index, hl: index as f32, title: String::new(), cluster: (0.0, 0.0), canvas: None })
    }
}

/// Adapta las barras a los monitores actuales sin reiniciar: conserva las de los monitores que siguen,
/// crea las de los nuevos y quita las de los desconectados. Deja `bars[i]` en el monitor `mons[i]`.
pub fn sync(mons: &[MonitorInfo]) {
    let height = height_px();
    let old: Vec<BarWin> = STATE.with(|s| s.borrow_mut().as_mut().map(|st| std::mem::take(&mut st.bars)).unwrap_or_default());
    let mut old: Vec<Option<BarWin>> = old.into_iter().map(Some).collect();
    let mut fresh = Vec::new();
    let mut bars = Vec::new();
    for (i, m) in mons.iter().enumerate() {
        let kept = old.iter_mut().find(|b| b.as_ref().is_some_and(|b| b.id == m.id)).and_then(Option::take);
        match kept {
            Some(mut b) => {
                b.mon = m.rect;
                bars.push(b);
            }
            None => match new_bar(m, i, height) {
                Ok(b) => {
                    fresh.push(b.hwnd);
                    bars.push(b);
                }
                Err(e) => eprintln!("aviso: no se pudo crear la barra del monitor nuevo: {e}"),
            },
        }
    }
    for b in old.into_iter().flatten() {
        unsafe {
            let mut abd = appbar_data(b.hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut abd);
            let _ = DestroyWindow(b.hwnd);
        }
    }
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.bars = bars;
        }
    });
    for hwnd in fresh {
        unsafe {
            let mut abd = appbar_data(hwnd);
            SHAppBarMessage(ABM_NEW, &mut abd);
        }
    }
    for (hwnd, mon) in self::bars() {
        reposition(hwnd, mon);
    }
    redraw_all();
    for (hwnd, _) in self::bars() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

fn appbar_data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: std::mem::size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        uCallbackMessage: WM_APPBAR,
        ..Default::default()
    }
}

fn bars() -> Vec<(HWND, Rect)> {
    STATE.with(|s| s.borrow().as_ref().map(|s| s.bars.iter().map(|b| (b.hwnd, b.mon)).collect()).unwrap_or_default())
}

/// Cambia colores, fuente y translucidez sin reiniciar.
pub fn set_theme(cfg: &Bar) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let font = Font::new(&theme.font, theme.base_px(), false).ok_or("no se pudo crear la fuente de la barra")?;
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.theme = theme;
            st.font = font;
            st.opacity = cfg.opacity;
            st.bubbles = cfg.style == "bubbles";
            st.ink = cfg.style == "ink";
        }
    });
    redraw_all();
    Ok(())
}

fn no_change() {}

/// Mira si suena algo (cada segundo, desde el temporizador de los widgets): muestra u oculta la pastilla de la
/// música y arranca o detiene su animación.
pub fn update_music() {
    crate::media::start(no_change);
    let now = crate::media::latest().filter(|m| m.playing && !m.title.trim().is_empty()).map(|m| m.title.trim().to_string());
    let playing = STATE.with(|s| {
        let mut g = s.borrow_mut();
        let st = g.as_mut()?;
        if st.music == now {
            return None;
        }
        if st.music.is_none() {
            st.music_since = std::time::Instant::now();
        }
        st.music = now;
        Some(st.music.is_some())
    });
    let Some(playing) = playing else { return };
    unsafe {
        if playing {
            SetTimer(Some(hwnd()), TIMER_MUSIC, 33, None);
        } else {
            let _ = KillTimer(Some(hwnd()), TIMER_MUSIC);
        }
    }
    redraw_all();
}

/// Vuelve a leer Wi-Fi, volumen y batería y redibuja (lo usa también el panel tras un cambio).
pub fn refresh_status() {
    let s = status::poll();
    STATE.with(|st| {
        if let Some(st) = st.borrow_mut().as_mut() {
            st.status = s;
        }
    });
    redraw_all();
}

/// Última lectura del estado del sistema.
pub fn status() -> Status {
    STATE.with(|s| s.borrow().as_ref().map(|s| s.status.clone()).unwrap_or_default())
}

/// Redibuja y presenta todas las barras.
fn redraw_all() {
    let clock = clock_text();
    STATE.with(|s| {
        let mut g = s.borrow_mut();
        let Some(st) = g.as_mut() else { return };
        let State { theme, font, opacity, height, occupied, focused_mon, status, bubbles, ink, music, music_since, bars } = st;
        let music_t = music_since.elapsed().as_secs_f32();
        for (i, b) in bars.iter_mut().enumerate() {
            let (w, h) = (b.mon.w, *height);
            if b.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
                b.canvas = Canvas::new(w, h);
            }
            let Some(canvas) = b.canvas.as_mut() else { continue };
            let view = BarView { occupied, hl: b.hl, focused: i == *focused_mon, title: &b.title, right: &clock, status, bubbles: *bubbles, ink: *ink, music: music.as_deref().map(|title| MusicView { title, t: music_t }) };
            b.cluster = render(canvas, theme, font, *opacity, &view);
            canvas.present(b.hwnd, None);
        }
    });
}

/// ABM_NEW + posición en cada monitor. Después el área de trabajo de cada uno excluye su barra.
pub fn register_all() {
    for (hwnd, mon) in bars() {
        unsafe {
            let mut abd = appbar_data(hwnd);
            SHAppBarMessage(ABM_NEW, &mut abd);
        }
        reposition(hwnd, mon);
    }
    redraw_all();
    for (hwnd, _) in bars() {
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

/// ABM_QUERYPOS + ABM_SETPOS en el borde superior del monitor `mon`.
pub fn reposition(hwnd: HWND, mon: Rect) {
    let height = height_px();
    unsafe {
        let mut abd = appbar_data(hwnd);
        abd.uEdge = ABE_TOP;
        abd.rc = RECT { left: mon.x, top: mon.y, right: mon.right(), bottom: mon.y + height };
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        abd.rc.bottom = abd.rc.top + height;
        SHAppBarMessage(ABM_SETPOS, &mut abd);
        let r = abd.rc;
        let _ = MoveWindow(hwnd, r.left, r.top, r.right - r.left, r.bottom - r.top, true);
    }
}

/// Cambia el alto de las barras sin reiniciar (px a 96 DPI) y las vuelve a registrar con el nuevo tamaño.
pub fn set_height(cfg_height: i32, mons: &[MonitorInfo]) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.height = dpi_scale(cfg_height);
            for b in st.bars.iter_mut() {
                b.canvas = None;
            }
        }
    });
    reposition_all(mons);
}

/// Recoloca todas las barras (cambio de resolución o de monitores).
pub fn reposition_all(mons: &[MonitorInfo]) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            for (b, m) in st.bars.iter_mut().zip(mons) {
                b.mon = m.rect;
            }
        }
    });
    for (hwnd, mon) in bars() {
        reposition(hwnd, mon);
    }
    redraw_all();
}

pub fn unregister_all() {
    for (hwnd, _) in bars() {
        unsafe {
            let mut abd = appbar_data(hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut abd);
        }
    }
}

/// `per_mon[i]` = (workspace activo, título enfocado) del monitor `i`.
pub fn set_state(per_mon: Vec<(usize, String)>, occupied: Vec<bool>, focused_mon: usize) {
    STATE.with(|s| {
        let mut g = s.borrow_mut();
        let Some(st) = g.as_mut() else { return };
        for (b, (active, title)) in st.bars.iter_mut().zip(per_mon) {
            b.active = active;
            b.title = title;
        }
        st.occupied = occupied;
        st.focused_mon = focused_mon;
    });
    redraw_all();
    unsafe {
        SetTimer(Some(hwnd()), TIMER_BAR, 16, None);
    }
}

/// Desliza la pastilla de cada barra hacia su workspace activo (suavizado exponencial).
fn step_highlight() {
    let settled = STATE.with(|s| {
        let mut g = s.borrow_mut();
        let Some(st) = g.as_mut() else { return true };
        let mut settled = true;
        for b in st.bars.iter_mut() {
            let d = b.active as f32 - b.hl;
            if d.abs() < 0.01 {
                b.hl = b.active as f32;
            } else {
                b.hl += d * 0.3;
                settled = false;
            }
        }
        settled
    });
    redraw_all();
    if settled {
        unsafe {
            let _ = KillTimer(Some(hwnd()), TIMER_BAR);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            match wp.0 {
                TIMER_BAR => step_highlight(),
                TIMER_WALLPAPER => crate::app::on_wallpaper_tick(),
                TIMER_LIVEWALL => crate::livewall::tick(),
                TIMER_WIDGETS => {
                    crate::widgets::tick();
                    update_music();
                }
                TIMER_MUSIC => redraw_all(),
                TIMER_STATUS => {
                    refresh_status();
                    crate::dock::refresh();
                    // cada ~30 s devuelve a Windows la memoria que no se usa
                    thread_local!(static N: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
                    if N.with(|n| { n.set(n.get() + 1); n.get() % 10 == 0 }) {
                        crate::mem::trim();
                    }
                }
                TIMER_TASKBAR => crate::window::hide_taskbars_if_visible(),
                TIMER_MOUSE => crate::app::on_mouse_tick(),
                TIMER_SETTLE => {
                    let _ = KillTimer(Some(hwnd), TIMER_SETTLE);
                    crate::app::on_settle();
                }
                _ => redraw_all(), // reloj
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = (lp.0 & 0xffff) as i16 as f32;
            let hit = STATE.with(|s| {
                s.borrow().as_ref().and_then(|s| {
                    let i = s.bars.iter().position(|b| b.hwnd == hwnd)?;
                    Some((i, s.bars[i].cluster, s.bars[i].mon))
                })
            });
            if let Some((bar, (c0, c1), mon)) = hit {
                if x >= c0 && x <= c1 {
                    crate::panel::toggle(mon, height_px());
                } else {
                    let (x0, slot) = workspace_metrics(STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.bubbles)));
                    let idx = ((x - x0) / slot).floor();
                    if idx >= 0.0 {
                        crate::app::on_bar_click(bar, idx as usize);
                    }
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            crate::app::on_hotkey(wp.0 as i32 - 1);
            LRESULT(0)
        }
        crate::keybinds::WM_CAPTURE_KEY => {
            crate::palette::on_capture(wp.0 as u32, lp.0 as u32);
            LRESULT(0)
        }
        crate::keybinds::WM_SWITCH_KEY => {
            crate::app::on_switch(wp.0);
            LRESULT(0)
        }
        crate::keybinds::WM_HOOK_KEY => {
            crate::app::on_hotkey(wp.0 as i32);
            LRESULT(0)
        }
        crate::window::WM_WIN_EVENT => {
            crate::app::on_event(wp.0 as u32, lp.0);
            LRESULT(0)
        }
        WM_APPBAR => {
            if wp.0 == ABN_POSCHANGED {
                if let Some(mon) = STATE.with(|s| {
                    s.borrow().as_ref().and_then(|s| s.bars.iter().find(|b| b.hwnd == hwnd).map(|b| b.mon))
                }) {
                    reposition(hwnd, mon);
                }
                crate::app::on_work_area_changed(false);
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            crate::app::on_work_area_changed(true);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            crate::app::on_work_area_changed(false);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            if hwnd == self::hwnd() {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_stays_in_range_and_moves() {
        let mut changed = false;
        for i in 0..5 {
            for k in 0..200 {
                let t = k as f32 * 0.05;
                let v = wave_level(i, t);
                assert!((0.27..=1.01).contains(&v));
                changed |= (v - wave_level(i, t + 0.2)).abs() > 0.05;
            }
        }
        assert!(changed);
    }
}
