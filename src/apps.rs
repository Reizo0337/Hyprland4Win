//! Índice de aplicaciones del sistema: la carpeta `shell:AppsFolder` (la lista "Todas las aplicaciones"
//! de Inicio, con apps de escritorio y de la Tienda). Se construye en un hilo en segundo plano.

use std::sync::Mutex;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{
    IEnumShellItems, IShellItem, SHGetKnownFolderItem, ShellExecuteW, BHID_EnumItems, FOLDERID_AppsFolder,
    KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    /// Identificador dentro de `shell:AppsFolder` (AppUserModelID o ruta).
    pub id: String,
}

static INDEX: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

pub fn entries() -> Vec<Entry> {
    INDEX.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Indexa en segundo plano (no retrasa el arranque). Se puede llamar de nuevo para refrescar.
pub fn start_indexing() {
    let _ = std::thread::Builder::new().name("hypr-win-index".into()).spawn(|| {
        let mut found = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let r = enumerate();
            CoUninitialize();
            r
        };
        match found.as_mut() {
            Ok(v) => {
                v.sort_by_key(|e| e.name.to_lowercase());
                v.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
                *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = std::mem::take(v);
            }
            Err(e) => eprintln!("aviso: no se pudo indexar las aplicaciones: {e}"),
        }
    });
}

unsafe fn display_name(item: &IShellItem, kind: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    let p = item.GetDisplayName(kind).ok()?;
    let s = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const _));
    s
}

unsafe fn enumerate() -> windows::core::Result<Vec<Entry>> {
    let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
    let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
    let mut out = Vec::new();
    loop {
        let mut next = [None];
        let mut fetched = 0u32;
        items.Next(&mut next, Some(&mut fetched))?;
        if fetched == 0 {
            break;
        }
        let Some(item) = next[0].take() else { break };
        let (Some(name), Some(id)) =
            (display_name(&item, SIGDN_NORMALDISPLAY), display_name(&item, SIGDN_PARENTRELATIVEPARSING))
        else {
            continue;
        };
        if !name.trim().is_empty() && !id.is_empty() {
            out.push(Entry { name, id });
        }
    }
    Ok(out)
}

pub fn launch(e: &Entry) {
    let target = HSTRING::from(format!("shell:AppsFolder\\{}", e.id));
    unsafe {
        ShellExecuteW(None, w!("open"), &target, PCWSTR::null(), None, SW_SHOWNORMAL);
    }
}

/// Abre un URI o ruta con el programa asociado (p. ej. `ms-settings:sound`).
pub fn open_uri(uri: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(uri), PCWSTR::null(), None, SW_SHOWNORMAL);
    }
}
