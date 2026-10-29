//! Parseo de atajos ("Super+Shift+Left, move, left") y registro con `RegisterHotKey`.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, SendInput, UnregisterHotKey, HOT_KEY_MODIFIERS, INPUT, INPUT_0, INPUT_KEYBOARD,
    KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT,
    LLKHF_INJECTED, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Mensaje que el hook de teclado envía a la ventana principal: wparam = índice del atajo.
pub const WM_HOOK_KEY: u32 = WM_APP + 3;
/// Combinación capturada (modo captura de atajos): wparam = tecla, lparam = modificadores.
pub const WM_CAPTURE_KEY: u32 = WM_APP + 4;

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;
pub const MOD_NOREPEAT: u32 = 0x4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Exec(String),
    Close,
    Focus(Dir),
    Move(Dir),
    /// Índice base 0.
    Workspace(usize),
    MoveToWorkspace(usize),
    /// Workspace anterior (-1) o siguiente (+1), con vuelta al final.
    WorkspaceRel(i32),
    MoveToWorkspaceRel(i32),
    /// Cicla el foco entre las ventanas del workspace: +1 siguiente, -1 anterior.
    Cycle(i32),
    Fullscreen,
    ToggleLayout,
    Quit,
    /// Abre o cierra el lanzador de aplicaciones.
    Launcher,
    /// Abre o cierra el panel de control (Wi-Fi, sonido, batería).
    Panel,
    /// Abre o cierra el menú del sistema (atajos, instalar, apps web...).
    Menu,
    /// No hace nada: solo captura la combinación para que Windows no la reciba.
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub mods: u32,
    pub vk: u32,
    pub action: Action,
}

fn parse_dir(s: &str) -> Result<Dir, String> {
    match s.to_ascii_lowercase().as_str() {
        "left" | "l" => Ok(Dir::Left),
        "right" | "r" => Ok(Dir::Right),
        "up" | "u" => Ok(Dir::Up),
        "down" | "d" => Ok(Dir::Down),
        _ => Err(format!("dirección inválida: '{s}'")),
    }
}

fn parse_rel(s: &str) -> Option<i32> {
    match s.to_ascii_lowercase().as_str() {
        "next" | "+1" => Some(1),
        "prev" | "previous" | "-1" => Some(-1),
        _ => None,
    }
}

fn parse_ws(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(n @ 1..=9) => Ok(n - 1),
        _ => Err(format!("workspace inválido: '{s}' (1-9)")),
    }
}

pub fn parse_keys(s: &str) -> Result<(u32, u32), String> {
    let mut mods = MOD_NOREPEAT;
    let mut vk = None;
    for part in s.split('+').map(str::trim) {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "super" | "win" | "mod" => mods |= MOD_WIN,
            "shift" => mods |= MOD_SHIFT,
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            _ => {
                if vk.replace(parse_vk(&lower)?).is_some() {
                    return Err(format!("más de una tecla en '{s}'"));
                }
            }
        }
    }
    vk.map(|k| (mods, k)).ok_or_else(|| format!("falta la tecla en '{s}'"))
}

fn parse_vk(k: &str) -> Result<u32, String> {
    let named = match k {
        "return" | "enter" => Some(0x0D),
        "space" => Some(0x20),
        "tab" => Some(0x09),
        "escape" | "esc" => Some(0x1B),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        _ => None,
    };
    if let Some(v) = named {
        return Ok(v);
    }
    let bytes = k.as_bytes();
    if bytes.len() == 1 && bytes[0].is_ascii_alphanumeric() {
        return Ok(bytes[0].to_ascii_uppercase() as u32);
    }
    if let Some(n) = k.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=12).contains(&n) {
            return Ok(0x70 + n - 1);
        }
    }
    Err(format!("tecla desconocida: '{k}'"))
}

pub fn parse_binding(s: &str) -> Result<Binding, String> {
    let parts: Vec<&str> = s.splitn(3, ',').map(str::trim).collect();
    if parts.len() < 2 {
        return Err(format!("atajo mal formado: '{s}'"));
    }
    let (mods, vk) = parse_keys(parts[0])?;
    let arg = parts.get(2).copied().unwrap_or("");
    let need_arg = |name: &str| {
        if arg.is_empty() {
            Err(format!("'{name}' necesita un argumento en '{s}'"))
        } else {
            Ok(())
        }
    };
    let action = match parts[1].to_ascii_lowercase().as_str() {
        "exec" => {
            need_arg("exec")?;
            Action::Exec(arg.to_string())
        }
        "close" => Action::Close,
        "focus" => Action::Focus(parse_dir(arg)?),
        "move" => Action::Move(parse_dir(arg)?),
        "workspace" => match parse_rel(arg) {
            Some(d) => Action::WorkspaceRel(d),
            None => Action::Workspace(parse_ws(arg)?),
        },
        "movetoworkspace" => match parse_rel(arg) {
            Some(d) => Action::MoveToWorkspaceRel(d),
            None => Action::MoveToWorkspace(parse_ws(arg)?),
        },
        "cycle" => Action::Cycle(parse_rel(arg).ok_or_else(|| format!("'cycle' necesita next o prev en '{s}'"))?),
        "fullscreen" => Action::Fullscreen,
        "togglelayout" => Action::ToggleLayout,
        "quit" => Action::Quit,
        "launcher" => Action::Launcher,
        "panel" => Action::Panel,
        "menu" => Action::Menu,
        "none" => Action::None,
        other => return Err(format!("acción desconocida: '{other}'")),
    };
    Ok(Binding { mods, vk, action })
}

/// Registra con `RegisterHotKey` los atajos sin Super (id = índice + 1). Los atajos con Super
/// los atiende el hook de teclado, porque el shell se queda antes con Win+Número, Win+Q, etc.
/// Devuelve los que fallaron.
pub fn register_all(hwnd: HWND, binds: &[Binding]) -> Vec<String> {
    let mut failed = Vec::new();
    for (i, b) in binds.iter().enumerate().filter(|(_, b)| b.mods & MOD_WIN == 0) {
        let r = unsafe { RegisterHotKey(Some(hwnd), i as i32 + 1, HOT_KEY_MODIFIERS(b.mods), b.vk) };
        if let Err(e) = r {
            failed.push(format!("{:?}: {e}", b.action));
        }
    }
    failed
}

pub fn unregister_all(hwnd: HWND, count: usize) {
    for i in 0..count {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), i as i32 + 1);
        }
    }
}

// --- Hook de teclado de bajo nivel (WH_KEYBOARD_LL) para los atajos con Super ---
// Se ejecuta en nuestro propio hilo (no se inyecta nada en otros procesos).

const MOD_VKS: [u32; 8] = [0x5B, 0x5C, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];

#[derive(Default)]
struct HookState {
    target: isize,
    /// (mods sin NOREPEAT, vk, índice del atajo)
    binds: Vec<(u32, u32, usize)>,
    /// Bitset de modificadores pulsados, según MOD_VKS.
    down: u8,
    swallowed: Vec<u32>,
    win_used: bool,
    /// Tragar la tecla Win sola (no abrir el menú Inicio).
    block_start: bool,
    /// Modo captura: cada tecla (con sus modificadores) se entrega a la app en vez de ejecutarse.
    capture: bool,
}

impl HookState {
    fn mods(&self) -> u32 {
        let bit = |a: u8, b: u8, m: u32| if self.down & (a | b) != 0 { m } else { 0 };
        bit(1, 2, MOD_WIN) | bit(4, 8, MOD_SHIFT) | bit(16, 32, MOD_CONTROL) | bit(64, 128, MOD_ALT)
    }
}

thread_local! {
    static HOOK: RefCell<HookState> = RefCell::new(HookState::default());
}

/// Devuelve (tragar la tecla, enviar tecla ficticia para evitar el menú Inicio).
fn handle(msg: u32, vk: u32) -> (bool, bool) {
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if !down && !up {
        return (false, false);
    }
    HOOK.with(|h| {
        let mut s = h.borrow_mut();
        if let Some(i) = MOD_VKS.iter().position(|&m| m == vk) {
            let bit = 1u8 << i;
            if down && i < 2 && s.block_start {
                s.win_used = true; // al soltar Win se enviará la tecla ficticia que cancela el menú
            }
            let win_before = s.down & 3 != 0;
            if down {
                s.down |= bit;
            } else {
                s.down &= !bit;
            }
            let released_win = win_before && s.down & 3 == 0;
            let dummy = released_win && s.win_used;
            if released_win {
                s.win_used = false;
            }
            return (false, dummy);
        }
        if s.capture {
            if down {
                let mods = s.mods();
                if mods & MOD_WIN != 0 {
                    s.win_used = true;
                }
                unsafe {
                    let _ = PostMessageW(Some(HWND(s.target as *mut _)), WM_CAPTURE_KEY, WPARAM(vk as usize), LPARAM(mods as isize));
                }
            }
            return (true, false);
        }
        if up {
            return match s.swallowed.iter().position(|&k| k == vk) {
                Some(p) => {
                    s.swallowed.remove(p);
                    (true, false)
                }
                None => (false, false),
            };
        }
        if s.swallowed.contains(&vk) {
            return (true, false); // autorepetición de un atajo ya atendido
        }
        let mods = s.mods();
        let Some(&(_, _, idx)) = s.binds.iter().find(|&&(m, k, _)| m == mods && k == vk) else {
            return (false, false);
        };
        s.swallowed.push(vk);
        if mods & MOD_WIN != 0 {
            s.win_used = true;
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(s.target as *mut _)), WM_HOOK_KEY, WPARAM(idx), LPARAM(0));
        }
        (true, false)
    })
}

/// Enviar una tecla sin significado (0xFF) entre Win-down y Win-up evita que se abra el menú Inicio.
fn cancel_start_menu() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0xFF), dwFlags: flags, ..Default::default() },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

unsafe extern "system" fn ll_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        if kb.flags.0 & LLKHF_INJECTED.0 == 0 {
            let (swallow, dummy) = handle(wp.0 as u32, kb.vkCode);
            if dummy {
                cancel_start_menu();
            }
            if swallow {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Instala el hook para los atajos con Super. Debe llamarse desde el hilo del bucle de mensajes.
pub fn install_hook(target: HWND, binds: &[Binding], block_start: bool) -> Result<HHOOK, String> {
    HOOK.with(|h| {
        let mut s = h.borrow_mut();
        s.target = target.0 as isize;
        s.block_start = block_start;
        s.binds = binds
            .iter()
            .enumerate()
            .filter(|(_, b)| b.mods & MOD_WIN != 0)
            .map(|(i, b)| (b.mods & !MOD_NOREPEAT, b.vk, i))
            .collect();
    });
    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), Some(inst.into()), 0).map_err(|e| e.to_string())
    }
}

/// Activa o desactiva el modo captura de atajos del hook.
pub fn set_capture(on: bool) {
    HOOK.with(|h| h.borrow_mut().capture = on);
}

/// Sustituye los atajos con Super atendidos por el hook (tras editar los atajos en vivo).
pub fn update_hook(binds: &[Binding]) {
    HOOK.with(|h| {
        h.borrow_mut().binds = binds
            .iter()
            .enumerate()
            .filter(|(_, b)| b.mods & MOD_WIN != 0)
            .map(|(i, b)| (b.mods & !MOD_NOREPEAT, b.vk, i))
            .collect();
    });
}

pub fn remove_hook(hook: HHOOK) {
    unsafe {
        let _ = UnhookWindowsHookEx(hook);
    }
}

/// Texto de una combinación ("Super+Shift+Left"); `None` si la tecla no se puede usar en un atajo.
pub fn format_keys(mods: u32, vk: u32) -> Option<String> {
    let key = match vk {
        0x0D => "Return".to_string(),
        0x20 => "Space".to_string(),
        0x09 => "Tab".to_string(),
        0x1B => "Escape".to_string(),
        0x25 => "Left".to_string(),
        0x26 => "Up".to_string(),
        0x27 => "Right".to_string(),
        0x28 => "Down".to_string(),
        0x70..=0x7B => format!("F{}", vk - 0x70 + 1),
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        _ => return None,
    };
    let mut parts = Vec::new();
    for (bit, name) in [(MOD_WIN, "Super"), (MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift")] {
        if mods & bit != 0 {
            parts.push(name);
        }
    }
    parts.push(&key);
    Some(parts.join("+"))
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::Left => "a la izquierda",
        Dir::Right => "a la derecha",
        Dir::Up => "hacia arriba",
        Dir::Down => "hacia abajo",
    }
}

/// Descripción en español de lo que hace un atajo.
pub fn describe(a: &Action) -> String {
    match a {
        Action::Exec(c) => format!("Abrir {c}"),
        Action::Close => "Cerrar la ventana".into(),
        Action::Focus(d) => format!("Enfocar {}", dir_name(*d)),
        Action::Move(d) => format!("Mover la ventana {}", dir_name(*d)),
        Action::Workspace(n) => format!("Ir al escritorio {}", n + 1),
        Action::MoveToWorkspace(n) => format!("Llevar la ventana al escritorio {}", n + 1),
        Action::WorkspaceRel(d) => if *d > 0 { "Escritorio siguiente" } else { "Escritorio anterior" }.into(),
        Action::MoveToWorkspaceRel(d) => {
            if *d > 0 { "Llevar la ventana al siguiente escritorio" } else { "Llevar la ventana al escritorio anterior" }.into()
        }
        Action::Cycle(d) => if *d > 0 { "Ventana siguiente" } else { "Ventana anterior" }.into(),
        Action::Fullscreen => "Pantalla completa".into(),
        Action::ToggleLayout => "Cambiar de layout".into(),
        Action::Launcher => "Buscar aplicaciones".into(),
        Action::Panel => "Panel de control".into(),
        Action::Menu => "Menú".into(),
        Action::Quit => "Salir de hypr-win".into(),
        Action::None => "Bloquear un atajo de Windows".into(),
    }
}
