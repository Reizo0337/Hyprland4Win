//! Enumeración, filtros, hooks de eventos y colocación de ventanas (todo con APIs Win32 documentadas).

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::layout::Rect;

/// Mensaje que el hook envía a la ventana principal: wparam = evento, lparam = HWND.
pub const WM_WIN_EVENT: u32 = WM_APP + 1;

pub const OFFSCREEN: i32 = -32000;

static TARGET: AtomicIsize = AtomicIsize::new(0);

pub fn to_hwnd(h: isize) -> HWND {
    HWND(h as *mut c_void)
}

pub fn id(h: HWND) -> isize {
    h.0 as isize
}

pub fn title(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Procesos elevados (o inaccesibles): UIPI impide moverlos desde un proceso normal.
fn elevated_or_inaccessible(h: HWND) -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return true;
        };
        let mut token = HANDLE::default();
        let elevated = if OpenProcessToken(proc, TOKEN_QUERY, &mut token).is_ok() {
            let mut el = TOKEN_ELEVATION::default();
            let mut ret = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut el as *mut _ as *mut c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret,
            )
            .is_ok();
            let _ = CloseHandle(token);
            !ok || el.TokenIsElevated != 0
        } else {
            true
        };
        let _ = CloseHandle(proc);
        elevated
    }
}

const IGNORED_CLASSES: [&str; 6] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Windows.UI.Core.CoreWindow",
    "XamlExplorerHostIslandWindow",
];

pub fn is_manageable(h: HWND) -> bool {
    unsafe {
        if h.0.is_null() || !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
            return false;
        }
        if GetWindow(h, GW_OWNER).is_ok_and(|o| !o.0.is_null()) {
            return false;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 {
            return false;
        }
        let style = GetWindowLongW(h, GWL_STYLE) as u32;
        if style & WS_CAPTION.0 != WS_CAPTION.0 || style & WS_THICKFRAME.0 == 0 {
            return false;
        }
        if GetWindowTextLengthW(h) == 0 {
            return false;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut c_void, 4).is_ok() && cloaked != 0 {
            return false;
        }
        let class = class_name(h);
        if IGNORED_CLASSES.contains(&class.as_str()) || class == crate::bar::CLASS_NAME {
            return false;
        }
        !elevated_or_inaccessible(h)
    }
}

pub fn enumerate() -> Vec<isize> {
    unsafe extern "system" fn cb(h: HWND, lp: LPARAM) -> BOOL {
        let v = &mut *(lp.0 as *mut Vec<isize>);
        if is_manageable(h) {
            v.push(id(h));
        }
        BOOL(1)
    }
    let mut v: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    // EnumWindows va de arriba a abajo en z-order; el orden de mosaico va de atrás a delante.
    v.reverse();
    v
}

/// Rectángulo "normal" (restaurado) de la ventana, útil para restaurarla al salir.
pub fn normal_rect(h: HWND) -> Option<Rect> {
    let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe { GetWindowPlacement(h, &mut wp).ok()? };
    let r = wp.rcNormalPosition;
    Some(Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
}

/// Diferencia entre el rectángulo de la ventana y su marco visible (bordes invisibles de Win10/11).
fn frame_insets(h: HWND) -> (i32, i32, i32, i32) {
    unsafe {
        let mut win = RECT::default();
        let mut vis = RECT::default();
        if GetWindowRect(h, &mut win).is_err()
            || DwmGetWindowAttribute(
                h,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut vis as *mut _ as *mut c_void,
                std::mem::size_of::<RECT>() as u32,
            )
            .is_err()
        {
            return (0, 0, 0, 0);
        }
        (
            (vis.left - win.left).max(0),
            (vis.top - win.top).max(0),
            (win.right - vis.right).max(0),
            (win.bottom - vis.bottom).max(0),
        )
    }
}

fn flags() -> SET_WINDOW_POS_FLAGS {
    SWP_NOZORDER | SWP_NOACTIVATE
}

/// Coloca varias ventanas de una vez con DeferWindowPos (sin parpadeo); si falla, una a una.
pub fn place(items: &[(isize, Rect)]) {
    if items.is_empty() {
        return;
    }
    let targets: Vec<(HWND, RECT)> = items
        .iter()
        .map(|&(h, r)| {
            let hwnd = to_hwnd(h);
            unsafe {
                if IsZoomed(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
            }
            let (l, t, rr, b) = frame_insets(hwnd);
            let rect = RECT { left: r.x - l, top: r.y - t, right: r.right() + rr, bottom: r.bottom() + b };
            (hwnd, rect)
        })
        .collect();
    unsafe {
        let ok = (|| -> windows::core::Result<()> {
            let mut hdwp = BeginDeferWindowPos(targets.len() as i32)?;
            for (h, r) in &targets {
                hdwp = DeferWindowPos(hdwp, *h, None, r.left, r.top, r.right - r.left, r.bottom - r.top, flags())?;
            }
            EndDeferWindowPos(hdwp)
        })();
        if ok.is_err() {
            for (h, r) in &targets {
                let _ = SetWindowPos(*h, None, r.left, r.top, r.right - r.left, r.bottom - r.top, flags());
            }
        }
    }
}

/// Mueve fuera de pantalla (sin cambiar el tamaño) las ventanas de un workspace inactivo.
pub fn hide(handles: &[isize]) {
    for &h in handles {
        unsafe {
            let _ = SetWindowPos(to_hwnd(h), None, OFFSCREEN, OFFSCREEN, 0, 0, flags() | SWP_NOSIZE);
        }
    }
}

/// Posición/tamaño exactos (sin compensar bordes), usado al restaurar.
pub fn set_exact(h: isize, r: Rect) {
    unsafe {
        let _ = SetWindowPos(to_hwnd(h), None, r.x, r.y, r.w, r.h, flags());
    }
}

pub fn raise(h: isize) {
    unsafe {
        let _ = SetWindowPos(to_hwnd(h), Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn focus(h: isize) {
    unsafe {
        let hwnd = to_hwnd(h);
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn close(h: isize) {
    unsafe {
        let _ = PostMessageW(Some(to_hwnd(h)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

pub fn is_alive(h: isize) -> bool {
    unsafe { IsWindow(Some(to_hwnd(h))).as_bool() }
}

pub fn is_visible(h: isize) -> bool {
    unsafe { IsWindowVisible(to_hwnd(h)).as_bool() }
}

pub fn foreground() -> isize {
    unsafe { id(GetForegroundWindow()) }
}

unsafe extern "system" fn hook_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if hwnd.0.is_null() || id_object != OBJID_WINDOW.0 || id_child != 0 {
        return;
    }
    // Los cambios de nombre son muy frecuentes: solo importa el de la ventana en primer plano.
    if event == EVENT_OBJECT_NAMECHANGE && hwnd != GetForegroundWindow() {
        return;
    }
    let target = TARGET.load(Ordering::Relaxed);
    if target != 0 {
        let _ = PostMessageW(Some(to_hwnd(target)), WM_WIN_EVENT, WPARAM(event as usize), LPARAM(hwnd.0 as isize));
    }
}

/// Instala los hooks fuera de contexto (WINEVENT_OUTOFCONTEXT: nada se inyecta en otros procesos).
pub fn install_hooks(target: HWND) -> Vec<HWINEVENTHOOK> {
    TARGET.store(id(target), Ordering::Relaxed);
    [
        EVENT_OBJECT_CREATE,
        EVENT_OBJECT_DESTROY,
        EVENT_OBJECT_SHOW,
        EVENT_OBJECT_HIDE,
        EVENT_OBJECT_NAMECHANGE,
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_MINIMIZESTART,
        EVENT_SYSTEM_MINIMIZEEND,
        EVENT_SYSTEM_MOVESIZEEND,
    ]
    .into_iter()
    .map(|ev| unsafe {
        SetWinEventHook(ev, ev, Some(HMODULE::default()), Some(hook_proc), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)
    })
    .collect()
}

pub fn remove_hooks(hooks: Vec<HWINEVENTHOOK>) {
    TARGET.store(0, Ordering::Relaxed);
    for h in hooks {
        unsafe {
            let _ = UnhookWinEvent(h);
        }
    }
}
