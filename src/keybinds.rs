//! Parseo de atajos ("Super+Shift+Left, move, left") y registro con `RegisterHotKey`.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS,
};

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
    Fullscreen,
    ToggleLayout,
    Quit,
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
        "workspace" => Action::Workspace(parse_ws(arg)?),
        "movetoworkspace" => Action::MoveToWorkspace(parse_ws(arg)?),
        "fullscreen" => Action::Fullscreen,
        "togglelayout" => Action::ToggleLayout,
        "quit" => Action::Quit,
        other => return Err(format!("acción desconocida: '{other}'")),
    };
    Ok(Binding { mods, vk, action })
}

/// Registra todos los atajos; el id es `índice + 1`. Devuelve los que fallaron.
pub fn register_all(hwnd: HWND, binds: &[Binding]) -> Vec<String> {
    let mut failed = Vec::new();
    for (i, b) in binds.iter().enumerate() {
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
