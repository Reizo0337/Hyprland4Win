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
}

/// Dibuja la barra. Devuelve el tramo horizontal (px) de los iconos de estado, que abre el panel al hacer clic.
pub fn render(c: &mut Canvas, th: &Theme, font: &Font, opacity: u8, v: &BarView) -> (f32, f32) {
    c.clear();
    let (w, h) = (c.w as f32, c.h as f32);
    let alpha = (opacity as u32 * 255 / 100) as u8;
    c.fill_round(FRect::new(0.0, 0.0, w, h), 0.0, argb(th.bg, alpha));
    c.fill_round(FRect::new(0.0, h - 1.0, w, 1.0), 0.0, argb(th.fg, 28));

    // puntos de workspaces + pastilla activa deslizante
    let (slot, x0, cy) = (px(SLOT), px(LEFT_PAD), h / 2.0);
    let dot = px(6.0);
    for (i, &occ) in v.occupied.iter().enumerate() {
        let cx = x0 + i as f32 * slot + slot / 2.0;
        let a = if occ { 170 } else { 70 };
        c.fill_ellipse(FRect::new(cx - dot / 2.0, cy - dot / 2.0, dot, dot), argb(th.fg, a));
    }
    let cx = x0 + v.hl * slot + slot / 2.0;
    let pill_w = px(16.0);
    let color = if v.focused { argb(th.accent, 255) } else { argb(th.fg, 200) };
    c.fill_round(FRect::new(cx - pill_w / 2.0, cy - dot / 2.0, pill_w, dot), dot / 2.0, color);

    // fecha y hora a la derecha
    let right_w = c.measure(font, v.right);
    let right = FRect::new(w - px(LEFT_PAD) - right_w - px(4.0), 0.0, right_w + px(4.0), h);
    c.text(font, v.right, right, argb(th.fg, 255), Align::Right);

    // iconos de estado a la izquierda del reloj: Wi-Fi, volumen, batería
    let icon = px(15.0);
    let iy = (h - icon) / 2.0;
    let mut x = right.x - px(18.0);
    let cluster_end = x + px(8.0);
    if let Some(b) = v.status.battery {
        let label = format!("{}%", b.percent);
        let lw = c.measure(font, &label);
        c.text(font, &label, FRect::new(x - lw, 0.0, lw + px(2.0), h), argb(th.fg, 255), Align::Right);
        let (bw, bh) = (px(25.0), px(11.5));
        x -= lw + px(5.0) + bw;
        glyphs::battery(c, FRect::new(x, (h - bh) / 2.0, bw, bh), b.percent, b.charging, th.fg);
        x -= px(16.0);
    }
    if let Some(vol) = v.status.volume {
        x -= icon;
        glyphs::speaker(c, x, iy, icon, vol.level, vol.muted, th.fg);
        x -= px(16.0);
    }
    if let Some(lit) = v.status.wifi_lit() {
        x -= icon;
        glyphs::wifi(c, x, iy, icon, lit, !v.status.wifi.enabled, th.fg);
    }
    let cluster = (x - px(8.0), cluster_end);

    // título entre los puntos de workspaces y los iconos de estado
    let tx = x0 + v.occupied.len() as f32 * slot + px(14.0);
    c.text(font, v.title, FRect::new(tx, 0.0, (cluster.0 - tx - px(20.0)).max(0.0), h), argb(th.fg, 255), Align::Left);
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

/// Crea una barra por monitor (aún sin registrar como appbar).
pub fn create_all(cfg: &Bar, workspaces: usize, mons: &[MonitorInfo]) -> Result<Vec<HWND>, String> {
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
        let mut out = Vec::new();
        for (i, m) in mons.iter().enumerate() {
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
            if i == 0 {
                MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
                SetTimer(Some(hwnd), TIMER_CLOCK, 10_000, None);
                SetTimer(Some(hwnd), TIMER_STATUS, 3_000, None);
            }
            STATE.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    st.bars.push(BarWin { hwnd, mon: m.rect, active: i, hl: i as f32, title: String::new(), cluster: (0.0, 0.0), canvas: None });
                }
            });
            out.push(hwnd);
        }
        Ok(out)
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
        let State { theme, font, opacity, height, occupied, focused_mon, status, bars } = st;
        for (i, b) in bars.iter_mut().enumerate() {
            let (w, h) = (b.mon.w, *height);
            if b.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
                b.canvas = Canvas::new(w, h);
            }
            let Some(canvas) = b.canvas.as_mut() else { continue };
            let view = BarView { occupied, hl: b.hl, focused: i == *focused_mon, title: &b.title, right: &clock, status };
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
                TIMER_STATUS => {
                    refresh_status();
                    crate::dock::refresh();
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
                    let idx = ((x - px(LEFT_PAD)) / px(SLOT)).floor();
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
