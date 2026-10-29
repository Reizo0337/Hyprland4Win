//! Los favicons de los programas de la lista de instalación se descargan y se pueden cargar como iconos.
//! Necesita red: si no hay conexión, la prueba se omite.

use hypr_win::{icons, packages, webapp};

#[test]
fn package_favicons_download_and_load() {
    let mut loaded = 0;
    let mut tried = 0;
    for p in packages::CURATED.iter().take(8) {
        tried += 1;
        let Some(path) = webapp::cached_favicon(p.site) else { continue };
        let icon = icons::from_image_file(&path.to_string_lossy()).unwrap_or_else(|| panic!("no se pudo cargar {}", path.display()));
        assert!(icon.w > 0 && icon.h > 0);
        loaded += 1;
        // la segunda vez sale de la caché, sin red
        assert_eq!(webapp::favicon_if_cached(p.site).as_deref(), Some(path.as_path()));
    }
    println!("{loaded} de {tried} favicons descargados y cargados");
    if loaded == 0 {
        eprintln!("sin red: se omite");
    } else {
        assert!(loaded * 2 >= tried, "fallaron demasiados favicons: {loaded}/{tried}");
    }
}
