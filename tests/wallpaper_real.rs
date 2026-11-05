//! Fondo de escritorio real: lectura, generación de un degradado, miniaturas y aplicar + restaurar.
//! La prueba de aplicar cambia el fondo un instante y lo restaura siempre (también si falla).

use hypr_win::wallpaper::{self, Kind};
use hypr_win::{icons, theme};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

fn com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
}

#[test]
fn reads_monitors_current_wallpaper_and_images() {
    com();
    let ids = wallpaper::monitor_ids();
    let now = wallpaper::current();
    println!("{} monitores, {} fondos leídos, ajuste actual: {:?}", ids.len(), now.len(), wallpaper::fit());
    assert!(!ids.is_empty(), "IDesktopWallpaper no devolvió monitores");
    assert_eq!(ids.len(), now.len());
    let imgs = wallpaper::images("", 200);
    println!("{} imágenes encontradas (usuario + Windows)", imgs.len());
    assert!(imgs.iter().all(|i| i.path.is_file() && !i.name.is_empty()));
}

#[test]
fn generated_wallpaper_is_a_valid_png_with_a_thumbnail() {
    com();
    let bg = hypr_win::config::parse_color("#1e1e2e").unwrap();
    let accent = hypr_win::config::parse_color("#89b4fa").unwrap();
    let path = wallpaper::generate(Kind::Aurora, "HyprWin Prueba", bg, accent).expect("generar el fondo");
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _guard = Cleanup(path.clone());
    let bytes = std::fs::read(&path).unwrap();
    println!("PNG generado: {} KB", bytes.len() / 1024);
    assert_eq!(&bytes[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    assert!(bytes.len() > 10_000, "el PNG parece vacío");
    assert!(bytes.len() < 8_000_000, "el PNG no está comprimido");

    let thumb = icons::thumbnail_from_file(&path.to_string_lossy(), 96).expect("miniatura");
    assert_eq!((thumb.w, thumb.h), (96, 96));
    let full = icons::from_image_file(&path.to_string_lossy()).expect("cargar la imagen");
    assert_eq!((full.w, full.h), (1920, 1080));
}

#[test]
fn applies_and_restores_the_original_wallpaper() {
    com();
    let before = wallpaper::current();
    if before.is_empty() || wallpaper::has_original() {
        eprintln!("se omite: no se puede leer el fondo o ya hay una copia pendiente");
        return;
    }
    // pase lo que pase, se vuelve al fondo anterior
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = wallpaper::restore_original();
        }
    }
    let _restore = Restore;

    let bg = hypr_win::config::parse_color("#2e3440").unwrap();
    let accent = hypr_win::config::parse_color("#88c0d0").unwrap();
    let png = wallpaper::generate(Kind::Gradient, "HyprWin Prueba Fondo", bg, accent).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(png.clone());

    wallpaper::set(&png.to_string_lossy(), None).expect("aplicar el fondo");
    assert!(wallpaper::has_original(), "debería haberse guardado el fondo original");
    let want = png.to_string_lossy().to_lowercase();
    // Windows aplica el fondo de forma asíncrona: se espera a que cambie (como máximo 8 s)
    let started = std::time::Instant::now();
    let mut after = wallpaper::current();
    while after.iter().filter(|p| p.to_lowercase() != want).count() > 1 && started.elapsed() < std::time::Duration::from_secs(8) {
        std::thread::sleep(std::time::Duration::from_millis(200));
        after = wallpaper::current();
    }
    println!("cambio visible tras {:?}", started.elapsed());
    println!("tras aplicar: {} monitores con el fondo nuevo", after.iter().filter(|p| p.to_lowercase() == want).count());
    for (i, p) in after.iter().enumerate() {
        println!("  [{i}] ahora: {:?}", p.rsplit('\\').next().unwrap_or(""));
    }
    println!("  esperado: {:?}", png.file_name().unwrap());
    let applied = after.iter().filter(|p| p.to_lowercase() == want).count();
    assert!(applied >= 2, "no se aplicó a los monitores conectados: {applied}");

    wallpaper::restore_original().expect("restaurar");
    let restored = wallpaper::current();
    assert_eq!(restored, before, "el fondo restaurado no coincide con el original");
    assert!(!wallpaper::has_original());
}

#[test]
fn theme_wallpaper_kinds_are_known() {
    for t in theme::builtins() {
        let key = t.wallpaper.as_deref().unwrap_or("");
        assert!(Kind::parse(key).is_some(), "{}: tipo de fondo desconocido {key:?}", t.name);
    }
}

/// Solo lectura: WinRT responde en este proceso y la API de bloqueo es alcanzable. No cambia la pantalla de bloqueo.
#[test]
fn lock_screen_api_is_reachable_without_changing_anything() {
    com();
    let current = wallpaper::current_lock_image();
    println!("imagen de bloqueo actual detectada: {}", current.is_some());
    if let Some(p) = &current {
        assert!(std::path::Path::new(p).is_absolute(), "ruta no absoluta: {p}");
    }
    // abrir un StorageFile de una imagen existente (lo mismo que hace set_lock_screen antes de aplicarla)
    let img = wallpaper::images("", 5).into_iter().next().expect("alguna imagen de Windows");
    let file = windows::Storage::StorageFile::GetFileFromPathAsync(&windows::core::HSTRING::from(img.path.to_string_lossy().as_ref()))
        .expect("pedir el archivo")
        .join()
        .expect("abrir el archivo");
    assert!(file.Path().is_ok());
    // un archivo que no existe se rechaza antes de tocar nada
    assert!(wallpaper::set_lock_screen(r"C:\no\existe.jpg").is_err());
}
