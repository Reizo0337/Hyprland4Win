//! Barra superior: ventana propia registrada como appbar y dibujada con GDI.

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::{self, Bar};

pub const CLASS_NAME: &str = "HyprWinBar";
/// Mensaje de callback del appbar (ABN_POSCHANGED, etc.).
pub const WM_APPBAR: u32 = WM_APP + 2;
const ABN_POSCHANGED: usize = 1;
const TIMER_CLOCK: usize = 1;

static BAR_HWND: AtomicIsize = AtomicIsize::new(0);

struct State {
    height: i32,
    bg: u32,
    fg: u32,
    dim: u32,
    accent: u32,
    accent_text: u32,
    font: HFONT,
    active: usize,
    occupied: Vec<bool>,
    title: String,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn dpi_scale(v: i32) -> i32 {
    (v as i64 * unsafe { GetDpiForSystem() } as i64 / 96) as i32
}

pub fn hwnd() -> HWND {
    HWND(BAR_HWND.load(Ordering::Relaxed) as *mut c_void)
}

pub fn height_px() -> i32 {
    STATE.with(|s| s.borrow().as_ref().map_or(32, |s| s.height))
}

/// Crea la ventana de la barra (aún sin registrar como appbar).
pub fn create(cfg: &Bar, workspaces: usize) -> Result<HWND, String> {
    let c = |s: &str| config::parse_color(s);
    let font_px = -(cfg.font_size * unsafe { GetDpiForSystem() } as i32 / 72);
    let font = unsafe {
        CreateFontW(
            font_px,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            &windows::core::HSTRING::from(cfg.font.as_str()),
        )
    };
    let state = State {
        height: dpi_scale(cfg.height),
        bg: c(&cfg.background)?,
        fg: c(&cfg.foreground)?,
        dim: c(&cfg.dim)?,
        accent: c(&cfg.accent)?,
        accent_text: c(&cfg.accent_text)?,
        font,
        active: 0,
        occupied: vec![false; workspaces],
        title: String::new(),
    };
    let height = state.height;
    STATE.with(|s| *s.borrow_mut() = Some(state));

    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = windows::core::HSTRING::from(CLASS_NAME);
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
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            PCWSTR(class.as_ptr()),
            w!("hypr-win bar"),
            WS_POPUP,
            0,
            0,
            GetSystemMetrics(SM_CXSCREEN),
            height,
            None,
            None,
            Some(inst.into()),
            None,
        )
        .map_err(|e| e.to_string())?;
        BAR_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        SetTimer(Some(hwnd), TIMER_CLOCK, 10_000, None);
        Ok(hwnd)
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

/// ABM_NEW + posición. Después de esto el área de trabajo del sistema excluye la barra.
pub fn register_appbar(hwnd: HWND) {
    unsafe {
        let mut abd = appbar_data(hwnd);
        SHAppBarMessage(ABM_NEW, &mut abd);
    }
    reposition(hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

/// ABM_QUERYPOS + ABM_SETPOS en el borde superior.
pub fn reposition(hwnd: HWND) {
    let height = height_px();
    unsafe {
        let mut abd = appbar_data(hwnd);
        abd.uEdge = ABE_TOP;
        abd.rc = RECT { left: 0, top: 0, right: GetSystemMetrics(SM_CXSCREEN), bottom: height };
        SHAppBarMessage(ABM_QUERYPOS, &mut abd);
        abd.rc.bottom = abd.rc.top + height;
        SHAppBarMessage(ABM_SETPOS, &mut abd);
        let r = abd.rc;
        let _ = MoveWindow(hwnd, r.left, r.top, r.right - r.left, r.bottom - r.top, true);
    }
}

pub fn unregister_appbar(hwnd: HWND) {
    unsafe {
        let mut abd = appbar_data(hwnd);
        SHAppBarMessage(ABM_REMOVE, &mut abd);
    }
}

pub fn set_state(active: usize, occupied: Vec<bool>, title: String) {
    STATE.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.active = active;
            st.occupied = occupied;
            st.title = title;
        }
    });
    unsafe {
        let _ = InvalidateRect(Some(hwnd()), None, false);
    }
}

fn draw_text(hdc: HDC, text: &str, rc: &mut RECT, color: u32, flags: DRAW_TEXT_FORMAT) {
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        SetTextColor(hdc, COLORREF(color));
        DrawTextW(hdc, &mut buf, rc, flags | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
    }
}

fn fill(hdc: HDC, rc: &RECT, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(COLORREF(color));
        FillRect(hdc, rc, brush);
        let _ = DeleteObject(brush.into());
    }
}

fn draw(hdc: HDC, width: i32, height: i32, st: &State) {
    unsafe {
        fill(hdc, &RECT { left: 0, top: 0, right: width, bottom: height }, st.bg);
        SetBkMode(hdc, TRANSPARENT);
        let old_font = SelectObject(hdc, st.font.into());

        let cell = height;
        for (i, &occ) in st.occupied.iter().enumerate() {
            let mut rc = RECT { left: i as i32 * cell, top: 0, right: (i as i32 + 1) * cell, bottom: height };
            let color = if i == st.active {
                fill(hdc, &rc, st.accent);
                st.accent_text
            } else if occ {
                st.fg
            } else {
                st.dim
            };
            draw_text(hdc, &(i + 1).to_string(), &mut rc, color, DT_CENTER);
        }

        let t = GetLocalTime();
        let clock = format!("{:02}:{:02}", t.wHour, t.wMinute);
        let clock_w = dpi_scale(64);
        let mut clock_rc = RECT { left: width - clock_w, top: 0, right: width - dpi_scale(8), bottom: height };
        draw_text(hdc, &clock, &mut clock_rc, st.fg, DT_RIGHT);

        let pad = dpi_scale(12);
        let mut title_rc = RECT {
            left: st.occupied.len() as i32 * cell + pad,
            top: 0,
            right: width - clock_w - pad,
            bottom: height,
        };
        draw_text(hdc, &st.title, &mut title_rc, st.fg, DT_LEFT | DT_END_ELLIPSIS);

        SelectObject(hdc, old_font);
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        // doble búfer para evitar parpadeo
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(mem, bmp.into());
        STATE.with(|s| {
            if let Some(st) = s.borrow().as_ref() {
                draw(mem, w, h, st);
            }
        });
        let _ = BitBlt(hdc, 0, 0, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = (lp.0 & 0xffff) as i16 as i32;
            let h = height_px().max(1);
            crate::app::on_bar_click((x / h) as usize);
            LRESULT(0)
        }
        WM_HOTKEY => {
            crate::app::on_hotkey(wp.0 as i32 - 1);
            LRESULT(0)
        }
        WM_WIN_EVENT_ALIAS => {
            crate::app::on_event(wp.0 as u32, lp.0);
            LRESULT(0)
        }
        WM_APPBAR => {
            if wp.0 == ABN_POSCHANGED {
                reposition(hwnd);
                crate::app::on_work_area_changed();
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            reposition(hwnd);
            crate::app::on_work_area_changed();
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

const WM_WIN_EVENT_ALIAS: u32 = crate::window::WM_WIN_EVENT;
