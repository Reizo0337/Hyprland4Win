//! Actualizaciones desde las *releases* de GitHub.
//!
//! Al arrancar (como mucho una vez cada 24 h) y desde el menú (Sistema > Buscar actualizaciones) se consulta la
//! última release. Si es más nueva que la instalada se avisa; al confirmar se descarga `hypr-win.exe`, se
//! **verifica su SHA-256** (el `digest` que da GitHub o un `hypr-win.exe.sha256` junto al exe; sin suma no se
//! instala), se renombra el exe en uso a `.old` (Windows lo permite), se copia el nuevo en su lugar y se lanza
//! con `--wait-pid=<pid>` para que espere a que este proceso termine y recupere los atajos de teclado. Si algo
//! falla a medias se deshace el cambio.
//!
//! Solo se descarga desde `github.com/<repositorio>/releases/download/`. Todo lo puro (versiones, JSON,
//! SHA-256) está probado sin red.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

pub const REPO: &str = "Reizo0337/Hyprland4Win";
/// Nombre del exe en la release.
pub const ASSET: &str = "hypr-win.exe";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

const API_LATEST: &str = "https://api.github.com/repos/Reizo0337/Hyprland4Win/releases/latest";
const DOWNLOAD_PREFIX: &str = "https://github.com/Reizo0337/Hyprland4Win/releases/download/";
/// Un exe de verdad pesa varios MB; un tope evita descargar basura sin fin.
const MAX_EXE_BYTES: usize = 64 * 1024 * 1024;
const MIN_EXE_BYTES: usize = 1024 * 1024;
const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);

// ---------- versiones ----------

/// "v1.5.0" / "1.5" -> [1, 5, 0]. Las versiones con sufijo (`-beta`) no se aceptan.
pub fn parse_version(s: &str) -> Option<[u32; 3]> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut parts = s.split('.');
    let mut v = [0u32; 3];
    for slot in v.iter_mut() {
        match parts.next() {
            Some(p) => *slot = p.parse().ok()?,
            None => break,
        }
    }
    parts.next().is_none().then_some(v).filter(|_| !s.is_empty())
}

/// ¿`candidate` es una versión estrictamente más nueva que `current`?
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

// ---------- release ----------

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// Sin la `v` ("1.5.0").
    pub version: String,
    pub url: String,
    pub size: u64,
    /// Suma SHA-256 (hex) que da GitHub en el asset, si la da.
    pub sha256: Option<String>,
    /// Si no hay `digest`, el `hypr-win.exe.sha256` de la release.
    pub sha_url: Option<String>,
    /// Página de la release (para abrirla en el navegador).
    pub page: String,
    pub notes: String,
}

fn trusted(url: &str) -> bool {
    url.starts_with(DOWNLOAD_PREFIX) && !url.contains("..")
}

/// Interpreta la respuesta de `/releases/latest`.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let r: ApiRelease = serde_json::from_str(json).map_err(|e| format!("respuesta de GitHub no válida: {e}"))?;
    if r.draft || r.prerelease {
        return Err("la última release es un borrador o una preversión".into());
    }
    let version = r.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    if parse_version(&version).is_none() {
        return Err(format!("versión no válida en la release: {}", r.tag_name));
    }
    let exe = r.assets.iter().find(|a| a.name == ASSET).ok_or_else(|| format!("la release no incluye {ASSET}"))?;
    if !trusted(&exe.browser_download_url) {
        return Err("la dirección de descarga no es de este repositorio".into());
    }
    let sha256 = exe.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).and_then(valid_hash);
    let sha_name = format!("{ASSET}.sha256");
    let sha_url = r.assets.iter().find(|a| a.name == sha_name).map(|a| a.browser_download_url.clone()).filter(|u| trusted(u));
    Ok(Release { version, url: exe.browser_download_url.clone(), size: exe.size, sha256, sha_url, page: r.html_url, notes: r.body })
}

/// Primer "token" de 64 dígitos hexadecimales del texto (formato de `sha256sum`: `<hash>  <archivo>`), en minúsculas.
pub fn valid_hash(text: &str) -> Option<String> {
    let t = text.split_whitespace().next()?;
    (t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit())).then(|| t.to_ascii_lowercase())
}

// ---------- SHA-256 (FIPS 180-4) ----------

#[allow(clippy::chunks_exact_to_as_chunks)] // as_chunks es muy reciente; así compila en más versiones de Rust
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    // relleno: 0x80, ceros y la longitud en bits (64 bits, big endian)
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

// ---------- estado ----------

static AVAILABLE: Mutex<Option<Release>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

fn set_available(r: Option<Release>) {
    *AVAILABLE.lock().unwrap_or_else(|e| e.into_inner()) = r;
}

/// La release más nueva que la instalada, si ya se comprobó y hay una.
pub fn available() -> Option<Release> {
    AVAILABLE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn data_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("hypr-win"))
}

fn last_check_file() -> Option<PathBuf> {
    Some(data_dir()?.join("update-check.txt"))
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// ¿Pasó el intervalo desde la última comprobación (`last` y `now` en segundos Unix)?
pub fn is_due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|l| now.saturating_sub(l) >= CHECK_EVERY.as_secs() || l > now)
}

fn read_last_check() -> Option<u64> {
    std::fs::read_to_string(last_check_file()?).ok()?.trim().parse().ok()
}

fn write_last_check() {
    if let Some(f) = last_check_file() {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(f, now_secs().to_string());
    }
}

// ---------- avisos ----------

fn toast(id: u64, glyph: char, rgb: u32, title: &str, sub: &str, ttl_secs: Option<u64>) {
    crate::toast::upsert(id, glyph, rgb, title, sub, ttl_secs.map(Duration::from_secs));
}

// ---------- comprobar ----------

fn fetch_latest() -> Result<Release, String> {
    let json = crate::net::http_get_text(API_LATEST).ok_or("no se pudo contactar con GitHub")?;
    parse_release(&json)
}

/// Consulta la última release en segundo plano. `manual`: también avisa si no hay nada nuevo o si falla.
pub fn check_async(manual: bool) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("hypr-win-update-check".into()).spawn(move || {
        let id = crate::toast::next_id();
        match fetch_latest() {
            Ok(r) => {
                write_last_check();
                if is_newer(&r.version, CURRENT) {
                    let v = r.version.clone();
                    set_available(Some(r));
                    toast(id, '\u{E896}', crate::toast::BLUE, &format!("Actualización disponible: v{v}"), "Súper+Espacio > Sistema > Actualizar", Some(12));
                } else {
                    set_available(None);
                    if manual {
                        toast(id, '\u{E73E}', crate::toast::GREEN, "hypr-win está al día", &format!("Versión instalada: v{CURRENT}"), Some(5));
                    }
                }
            }
            Err(e) if manual => toast(id, '\u{E783}', crate::toast::RED, "No se pudo buscar actualizaciones", &e, Some(9)),
            Err(e) => eprintln!("aviso: comprobación de actualizaciones: {e}"),
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

/// Al arrancar: comprueba en segundo plano si está activado y toca (cada 24 h), sin competir con el arranque.
pub fn check_on_start(enabled: bool) {
    if !enabled || !is_due(read_last_check(), now_secs()) {
        return;
    }
    let _ = std::thread::Builder::new().name("hypr-win-update-start".into()).spawn(|| {
        std::thread::sleep(Duration::from_secs(20));
        check_async(false);
    });
}

// ---------- instalar ----------

fn expected_hash(r: &Release) -> Result<String, String> {
    if let Some(h) = &r.sha256 {
        return Ok(h.clone());
    }
    let url = r.sha_url.as_deref().ok_or("la release no trae suma SHA-256: no se instala por seguridad")?;
    let text = crate::net::http_get_text(url).ok_or("no se pudo descargar la suma SHA-256")?;
    valid_hash(&text).ok_or_else(|| "la suma SHA-256 de la release no es válida".to_string())
}

/// Comprueba que lo descargado es el exe esperado: tamaño razonable, cabecera de ejecutable y suma SHA-256.
pub fn verify(bytes: &[u8], expected_sha256: &str) -> Result<(), String> {
    if bytes.len() < MIN_EXE_BYTES {
        return Err("la descarga es demasiado pequeña para ser el programa".into());
    }
    if &bytes[..2] != b"MZ" {
        return Err("la descarga no es un ejecutable de Windows".into());
    }
    let got = sha256_hex(bytes);
    if !got.eq_ignore_ascii_case(expected_sha256) {
        return Err("la suma SHA-256 no coincide: la descarga está dañada o alterada".into());
    }
    Ok(())
}

/// Descarga, verifica y aplica la actualización disponible. Al terminar bien, hypr-win se cierra y arranca el nuevo.
pub fn install_async() {
    let Some(release) = available() else { return };
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("hypr-win-update-install".into()).spawn(move || {
        let id = crate::toast::next_id();
        let mb = release.size as f32 / 1_048_576.0;
        toast(id, '\u{E896}', crate::toast::BLUE, &format!("Descargando v{}…", release.version), &format!("{mb:.1} MB"), None);
        match download_and_apply(&release) {
            Ok(()) => toast(id, '\u{E73E}', crate::toast::GREEN, &format!("Actualizado a v{}", release.version), "Reiniciando hypr-win…", Some(4)),
            Err(e) => {
                toast(id, '\u{E783}', crate::toast::RED, "No se pudo actualizar", &e, Some(10));
                BUSY.store(false, Ordering::SeqCst);
            }
        }
    });
}

fn download_and_apply(r: &Release) -> Result<(), String> {
    let want = expected_hash(r)?;
    let bytes = crate::net::http_get_limited(&r.url, MAX_EXE_BYTES).ok_or("falló la descarga")?;
    verify(&bytes, &want)?;
    let staged = data_dir().ok_or("no se encontró APPDATA")?.join("update");
    std::fs::create_dir_all(&staged).map_err(|e| e.to_string())?;
    let new_exe = staged.join(format!("hypr-win-{}.exe", r.version));
    std::fs::write(&new_exe, &bytes).map_err(|e| format!("no se pudo guardar la descarga: {e}"))?;
    apply(&new_exe)?;
    // el proceso nuevo ya está esperando: salimos limpiamente (restaura ventanas y barra de tareas)
    crate::app::request_quit();
    Ok(())
}

/// `<exe>.old`: el exe anterior, que se borra en el siguiente arranque.
fn old_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// Argumentos con los que se relanza (los actuales, sin un `--wait-pid` previo).
fn passthrough_args(args: impl Iterator<Item = String>) -> Vec<String> {
    args.filter(|a| !a.starts_with("--wait-pid=")).collect()
}

fn apply(new_exe: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    // Windows no deja sobrescribir un exe en uso, pero sí renombrarlo
    std::fs::rename(&exe, &old).map_err(|e| {
        format!("no se puede escribir en {}: {e}. Descarga la release a mano desde https://github.com/{REPO}/releases", exe.parent().map_or(String::new(), |p| p.display().to_string()))
    })?;
    if let Err(e) = std::fs::copy(new_exe, &exe) {
        let _ = std::fs::remove_file(&exe);
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("no se pudo copiar el programa nuevo: {e}"));
    }
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let spawned = std::process::Command::new(&exe)
        .args(passthrough_args(std::env::args().skip(1)))
        .arg(format!("--wait-pid={}", std::process::id()))
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn();
    if let Err(e) = spawned {
        let _ = std::fs::remove_file(&exe);
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("no se pudo iniciar la versión nueva: {e}"));
    }
    let _ = std::fs::remove_file(new_exe);
    Ok(())
}

// ---------- arranque ----------

/// Valor de `--wait-pid=<n>`.
pub fn wait_pid_arg(mut args: impl Iterator<Item = String>) -> Option<u32> {
    args.find_map(|a| a.strip_prefix("--wait-pid=").and_then(|n| n.parse().ok()))
}

/// Primera cosa que hace el programa al arrancar: si lo lanzó una actualización, espera a que termine el
/// proceso anterior (así los atajos de teclado ya están libres) y borra el exe viejo.
pub fn on_startup() {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    if let Some(pid) = wait_pid_arg(std::env::args().skip(1)) {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
                let _ = WaitForSingleObject(h, 20_000);
                let _ = CloseHandle(h);
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_path(&exe));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "tag_name": "v1.6.0", "draft": false, "prerelease": false,
        "html_url": "https://github.com/Reizo0337/Hyprland4Win/releases/tag/v1.6.0",
        "body": "Novedades",
        "assets": [
            {"name": "hypr-win.exe.sha256", "browser_download_url": "https://github.com/Reizo0337/Hyprland4Win/releases/download/v1.6.0/hypr-win.exe.sha256", "size": 90},
            {"name": "hypr-win.exe", "size": 6000000,
             "browser_download_url": "https://github.com/Reizo0337/Hyprland4Win/releases/download/v1.6.0/hypr-win.exe",
             "digest": "sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"}
        ]}"#;

    #[test]
    fn versions_parse_and_compare_numerically() {
        assert_eq!(parse_version("v1.5.0"), Some([1, 5, 0]));
        assert_eq!(parse_version("1.5"), Some([1, 5, 0]));
        assert_eq!(parse_version("2"), Some([2, 0, 0]));
        for bad in ["", "v", "1.2.3.4", "1.x", "1.5.0-beta", "abc"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(is_newer("1.10.0", "1.9.9") && is_newer("v2.0.0", "1.99.99") && is_newer("1.5.1", "1.5.0"));
        assert!(!is_newer("1.5.0", "1.5.0") && !is_newer("1.4.9", "1.5.0"));
        assert!(!is_newer("nope", "1.5.0") && !is_newer("1.6.0", "nope"));
        assert!(parse_version(CURRENT).is_some(), "la versión de Cargo debe ser válida: {CURRENT}");
    }

    #[test]
    fn a_release_is_parsed_with_its_digest_and_checksum_file() {
        let r = parse_release(SAMPLE).unwrap();
        assert_eq!(r.version, "1.6.0");
        assert_eq!(r.size, 6_000_000);
        assert!(r.url.ends_with("/v1.6.0/hypr-win.exe"));
        assert_eq!(r.sha256.as_deref(), Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert!(r.sha_url.as_deref().is_some_and(|u| u.ends_with("hypr-win.exe.sha256")));
        assert!(r.page.contains("/releases/tag/"));
    }

    #[test]
    fn bad_releases_are_rejected() {
        assert!(parse_release("no es json").is_err());
        assert!(parse_release(&SAMPLE.replace("\"draft\": false", "\"draft\": true")).is_err());
        assert!(parse_release(&SAMPLE.replace("\"prerelease\": false", "\"prerelease\": true")).is_err());
        assert!(parse_release(&SAMPLE.replace("v1.6.0\", \"draft", "latest\", \"draft")).is_err(), "etiqueta sin versión");
        // sin el exe entre los assets
        assert!(parse_release(&SAMPLE.replace("\"name\": \"hypr-win.exe\"", "\"name\": \"otro.exe\"")).is_err());
        // una descarga que no es de este repositorio no se acepta
        let evil = SAMPLE.replace("https://github.com/Reizo0337/Hyprland4Win/releases/download/v1.6.0/hypr-win.exe\"", "https://evil.example/hypr-win.exe\"");
        assert!(parse_release(&evil).is_err());
        let sneaky = SAMPLE.replace("Hyprland4Win/releases/download/v1.6.0/hypr-win.exe\"", "Hyprland4Win/releases/download/../../../evil/hypr-win.exe\"");
        assert!(parse_release(&sneaky).is_err());
    }

    #[test]
    fn a_missing_digest_leaves_only_the_checksum_file() {
        let no_digest = SAMPLE.replace("\"digest\": \"sha256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD\"", "\"digest\": null");
        let r = parse_release(&no_digest).unwrap();
        assert!(r.sha256.is_none() && r.sha_url.is_some());
        let none = no_digest.replace("hypr-win.exe.sha256", "readme.txt");
        let r = parse_release(&none).unwrap();
        assert!(r.sha256.is_none() && r.sha_url.is_none(), "sin ninguna suma no se instalará");
        assert!(expected_hash(&r).is_err());
    }

    #[test]
    fn sha256_matches_the_standard_test_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // más de un bloque y justo en el borde del relleno (55, 56 y 64 bytes)
        assert_eq!(sha256_hex(&vec![b'a'; 1_000_000]), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
        assert_eq!(sha256_hex(&[b'a'; 55]).len(), 64);
        assert_ne!(sha256_hex(&[b'a'; 55]), sha256_hex(&[b'a'; 56]));
        assert_ne!(sha256_hex(&[b'a'; 56]), sha256_hex(&[b'a'; 64]));
    }

    #[test]
    fn checksums_are_read_from_sha256sum_text() {
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(valid_hash(&format!("{h}  hypr-win.exe\n")).as_deref(), Some(h));
        assert_eq!(valid_hash(&h.to_uppercase()).as_deref(), Some(h));
        for bad in ["", "abc", &h[..63], &format!("{}g", &h[..63])] {
            assert_eq!(valid_hash(bad), None, "{bad}");
        }
    }

    #[test]
    fn downloads_are_verified_before_installing() {
        let mut exe = vec![0u8; MIN_EXE_BYTES + 10];
        exe[0] = b'M';
        exe[1] = b'Z';
        let good = sha256_hex(&exe);
        assert!(verify(&exe, &good).is_ok());
        assert!(verify(&exe, &good.to_uppercase()).is_ok());
        let wrong = sha256_hex(b"otra cosa");
        assert!(verify(&exe, &wrong).unwrap_err().contains("SHA-256"));
        assert!(verify(&exe[..100], &good).unwrap_err().contains("pequeña"));
        let mut not_exe = exe.clone();
        not_exe[0] = b'<';
        assert!(verify(&not_exe, &sha256_hex(&not_exe)).unwrap_err().contains("ejecutable"));
    }

    #[test]
    fn the_check_interval_and_clock_skew() {
        let day = CHECK_EVERY.as_secs();
        assert!(is_due(None, 1000));
        assert!(!is_due(Some(1000), 1000 + day - 1));
        assert!(is_due(Some(1000), 1000 + day));
        assert!(is_due(Some(5000), 1000), "una hora guardada en el futuro no bloquea las comprobaciones");
    }

    #[test]
    fn relaunch_arguments_and_wait_pid() {
        let args = || ["mi.toml".to_string(), "--wait-pid=42".to_string(), "--x".to_string()].into_iter();
        assert_eq!(wait_pid_arg(args()), Some(42));
        assert_eq!(wait_pid_arg(["mi.toml".to_string()].into_iter()), None);
        assert_eq!(wait_pid_arg(["--wait-pid=x".to_string()].into_iter()), None);
        assert_eq!(passthrough_args(args()), vec!["mi.toml".to_string(), "--x".to_string()]);
        assert_eq!(old_path(Path::new(r"C:\a\hypr-win.exe")), PathBuf::from(r"C:\a\hypr-win.exe.old"));
    }
}
