//! Aplicaciones web: un acceso directo del menú Inicio que abre una página en una ventana propia
//! (`msedge --app=URL`, o Chrome/Brave si no hay Edge), con el favicon como icono. Así aparecen en el
//! lanzador, el dock y el mosaico como cualquier otra aplicación.
//! Requiere COM inicializado en el hilo que llama.

use std::path::{Path, PathBuf};

use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::Urlmon::URLDownloadToFileW;
use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

pub struct WebApp {
    pub name: String,
    pub path: PathBuf,
}

fn env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// Carpeta del menú Inicio donde se guardan los accesos directos.
pub fn dir() -> Option<PathBuf> {
    Some(env("APPDATA")?.join(r"Microsoft\Windows\Start Menu\Programs\Hypr Web Apps"))
}

fn icon_dir() -> Option<PathBuf> {
    Some(env("APPDATA")?.join(r"hypr-win\webapps"))
}

pub fn list() -> Vec<WebApp> {
    let Some(dir) = dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut v: Vec<WebApp> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
        .filter_map(|p| Some(WebApp { name: p.file_stem()?.to_string_lossy().into_owned(), path: p }))
        .collect();
    v.sort_by_key(|a| a.name.to_lowercase());
    v
}

/// "youtube.com" -> "https://youtube.com"; `None` si no parece una dirección.
pub fn normalize_url(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() || s.contains(char::is_whitespace) {
        return None;
    }
    let url = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    let host = host_of(&url);
    (host.contains('.') && !host.starts_with('.') && !host.ends_with('.')).then_some(url)
}

/// Host de una URL ("https://www.youtube.com/watch" -> "www.youtube.com").
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or("").split('@').next_back().unwrap_or("").split(':').next().unwrap_or("").to_string()
}

/// Nombre apto para un archivo de Windows.
pub fn sanitize_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    s.trim().trim_end_matches('.').to_string()
}

/// Envuelve un PNG en un contenedor ICO (Windows admite PNG dentro de un icono desde Vista).
pub fn png_to_ico(png: &[u8]) -> Option<Vec<u8>> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if png.len() < 24 || png[..8] != SIG {
        return None;
    }
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    let dim = |v: u32| if v >= 256 { 0u8 } else { v as u8 };
    let mut ico = Vec::with_capacity(22 + png.len());
    ico.extend([0, 0, 1, 0, 1, 0]); // ICONDIR: 1 imagen
    ico.extend([dim(w), dim(h), 0, 0]);
    ico.extend(1u16.to_le_bytes()); // planos
    ico.extend(32u16.to_le_bytes()); // bits por píxel
    ico.extend((png.len() as u32).to_le_bytes());
    ico.extend(22u32.to_le_bytes()); // desplazamiento de los datos
    ico.extend(png);
    Some(ico)
}

/// Navegador con soporte de `--app=`: Edge (siempre presente en Windows 11), Chrome o Brave.
pub fn browser() -> Option<PathBuf> {
    let pf = env("ProgramFiles");
    let pf86 = env("ProgramFiles(x86)");
    let local = env("LOCALAPPDATA");
    let rels = [
        (&pf86, r"Microsoft\Edge\Application\msedge.exe"),
        (&pf, r"Microsoft\Edge\Application\msedge.exe"),
        (&pf, r"Google\Chrome\Application\chrome.exe"),
        (&pf86, r"Google\Chrome\Application\chrome.exe"),
        (&local, r"Google\Chrome\Application\chrome.exe"),
        (&pf, r"BraveSoftware\Brave-Browser\Application\brave.exe"),
    ];
    rels.iter().filter_map(|(base, rel)| base.as_ref().map(|b| b.join(rel))).find(|p| p.is_file())
}

/// Descarga `url` a `dest`; `true` si el archivo existe y no está vacío.
fn download(url: &str, dest: &Path) -> bool {
    let r = unsafe { URLDownloadToFileW(None, &HSTRING::from(url), &HSTRING::from(dest.to_string_lossy().as_ref()), 0, None) };
    r.is_ok() && std::fs::metadata(dest).is_ok_and(|m| m.len() > 0)
}

/// Descarga el favicon del sitio y lo guarda como `.ico` (Google primero; si no lo tiene, DuckDuckGo).
/// Si falla, el acceso directo usa el icono del navegador.
fn fetch_icon(host: &str, name: &str) -> Option<PathBuf> {
    let dir = icon_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{name}.ico"));
    let tmp = dir.join(format!("{name}.download"));

    if download(&format!("https://www.google.com/s2/favicons?domain={host}&sz=128"), &tmp) {
        if let Some(ico) = std::fs::read(&tmp).ok().and_then(|png| png_to_ico(&png)) {
            let _ = std::fs::remove_file(&tmp);
            std::fs::write(&path, ico).ok()?;
            return Some(path);
        }
    }
    // respaldo: DuckDuckGo entrega un .ico (o un PNG) directamente
    if download(&format!("https://icons.duckduckgo.com/ip3/{host}.ico"), &tmp) {
        let bytes = std::fs::read(&tmp).ok()?;
        let _ = std::fs::remove_file(&tmp);
        let ico = if bytes.starts_with(&[0, 0, 1, 0]) { Some(bytes) } else { png_to_ico(&bytes) }?;
        std::fs::write(&path, ico).ok()?;
        return Some(path);
    }
    let _ = std::fs::remove_file(&tmp);
    None
}

/// Carpeta de favicons de paquetes ya descargados.
fn pkg_icon_dir() -> Option<PathBuf> {
    Some(env("APPDATA")?.join(r"hypr-win\pkgicons"))
}

/// Favicon de `host` si ya está en la caché (sin red).
pub fn favicon_if_cached(host: &str) -> Option<PathBuf> {
    let dir = pkg_icon_dir()?;
    ["png", "ico"].iter().map(|e| dir.join(format!("{host}.{e}"))).find(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0))
}

/// Descarga el favicon de `host` a la caché (Google y, si no lo tiene, DuckDuckGo). Bloquea mientras descarga.
pub fn cached_favicon(host: &str) -> Option<PathBuf> {
    if let Some(p) = favicon_if_cached(host) {
        return Some(p);
    }
    let dir = pkg_icon_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let png = dir.join(format!("{host}.png"));
    if download(&format!("https://www.google.com/s2/favicons?domain={host}&sz=128"), &png) {
        return Some(png);
    }
    let _ = std::fs::remove_file(&png);
    let ico = dir.join(format!("{host}.ico"));
    if download(&format!("https://icons.duckduckgo.com/ip3/{host}.ico"), &ico) {
        return Some(ico);
    }
    let _ = std::fs::remove_file(&ico);
    None
}

fn make_shortcut(lnk: &Path, target: &Path, args: &str, icon: Option<&Path>, description: &str) -> Result<(), String> {
    let e = |e: windows::core::Error| e.message();
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(e)?;
        link.SetPath(&HSTRING::from(target.to_string_lossy().as_ref())).map_err(e)?;
        link.SetArguments(&HSTRING::from(args)).map_err(e)?;
        link.SetDescription(&HSTRING::from(description)).map_err(e)?;
        if let Some(i) = icon {
            link.SetIconLocation(&HSTRING::from(i.to_string_lossy().as_ref()), 0).map_err(e)?;
        }
        let file: IPersistFile = link.cast().map_err(e)?;
        file.Save(&HSTRING::from(lnk.to_string_lossy().as_ref()), true).map_err(e)?;
    }
    Ok(())
}

/// Crea la aplicación web `name` para `url`. Devuelve la ruta del acceso directo.
pub fn create(name: &str, url: &str) -> Result<PathBuf, String> {
    let name = sanitize_name(name);
    if name.is_empty() {
        return Err("el nombre no es válido".into());
    }
    let url = normalize_url(url).ok_or("la dirección no es válida")?;
    let browser = browser().ok_or("no se encontró Edge, Chrome ni Brave")?;
    let dir = dir().ok_or("no se encontró la carpeta del menú Inicio")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let icon = fetch_icon(&host_of(&url), &name);
    let lnk = dir.join(format!("{name}.lnk"));
    make_shortcut(&lnk, &browser, &format!("--app={url}"), icon.as_deref(), &format!("Aplicación web: {url}"))?;
    Ok(lnk)
}

pub fn remove(app: &WebApp) {
    let _ = std::fs::remove_file(&app.path);
    if let Some(dir) = icon_dir() {
        let _ = std::fs::remove_file(dir.join(format!("{}.ico", app.name)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_normalized_and_validated() {
        assert_eq!(normalize_url("youtube.com").as_deref(), Some("https://youtube.com"));
        assert_eq!(normalize_url("  http://localhost.test/x ").as_deref(), Some("http://localhost.test/x"));
        assert_eq!(normalize_url("open.spotify.com/browse").as_deref(), Some("https://open.spotify.com/browse"));
        assert_eq!(normalize_url("nodomain"), None);
        assert_eq!(normalize_url("dos palabras.com"), None);
        assert_eq!(normalize_url(""), None);
        assert_eq!(normalize_url("a..com").as_deref(), Some("https://a..com")); // lo valida el navegador
    }

    #[test]
    fn host_extraction() {
        assert_eq!(host_of("https://www.youtube.com/watch?v=1"), "www.youtube.com");
        assert_eq!(host_of("http://user@host.com:8080/x"), "host.com");
        assert_eq!(host_of("classroom.google.com"), "classroom.google.com");
    }

    #[test]
    fn names_are_file_safe() {
        assert_eq!(sanitize_name(" You/Tube: <1> "), "YouTube 1");
        assert_eq!(sanitize_name("***"), "");
    }

    #[test]
    fn png_is_wrapped_in_ico() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        png.extend(128u32.to_be_bytes());
        png.extend(128u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0]);
        let ico = png_to_ico(&png).unwrap();
        assert_eq!(&ico[..6], &[0, 0, 1, 0, 1, 0]);
        assert_eq!((ico[6], ico[7]), (128, 128));
        assert_eq!(u32::from_le_bytes(ico[14..18].try_into().unwrap()) as usize, png.len());
        assert_eq!(u32::from_le_bytes(ico[18..22].try_into().unwrap()), 22);
        assert_eq!(&ico[22..], &png[..]);
        assert!(png_to_ico(b"no es un png").is_none());
    }
}
