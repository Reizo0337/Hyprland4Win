//! Descarga HTTP(S) sencilla con WinINet (API documentada de Windows). `URLDownloadToFile` falla
//! (`0x800C0007`) con respuestas JSON, así que para datos se usa esta función.

use std::ffi::c_void;

use windows::core::{w, HSTRING};
use windows::Win32::Networking::WinInet::{
    InternetCloseHandle, InternetOpenUrlW, InternetOpenW, InternetReadFile, INTERNET_FLAG_NO_CACHE_WRITE, INTERNET_FLAG_RELOAD,
    INTERNET_OPEN_TYPE_PRECONFIG,
};

/// Tamaño máximo que se acepta de una respuesta.
const MAX_BYTES: usize = 4 * 1024 * 1024;

/// Descarga `url` y devuelve el cuerpo (bloquea; úsalo en un hilo).
pub fn http_get(url: &str) -> Option<Vec<u8>> {
    http_get_limited(url, MAX_BYTES)
}

/// Como `http_get` pero con otro tope de tamaño (para descargar el programa al actualizar).
pub fn http_get_limited(url: &str, max_bytes: usize) -> Option<Vec<u8>> {
    unsafe {
        let session = InternetOpenW(w!("hypr-win"), INTERNET_OPEN_TYPE_PRECONFIG.0, None, None, 0);
        if session.is_null() {
            return None;
        }
        let request = InternetOpenUrlW(session, &HSTRING::from(url), None, INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE, None);
        if request.is_null() {
            let _ = InternetCloseHandle(session);
            return None;
        }
        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        let ok = loop {
            let mut read = 0u32;
            if InternetReadFile(request, chunk.as_mut_ptr() as *mut c_void, chunk.len() as u32, &mut read).is_err() {
                break false;
            }
            if read == 0 {
                break true;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > max_bytes {
                break false;
            }
        };
        let _ = InternetCloseHandle(request);
        let _ = InternetCloseHandle(session);
        ok.then_some(body)
    }
}

/// Como `http_get`, pero devuelve el texto (UTF-8).
pub fn http_get_text(url: &str) -> Option<String> {
    String::from_utf8(http_get(url)?).ok()
}
