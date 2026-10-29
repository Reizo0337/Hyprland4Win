//! Renderiza la barra y el lanzador a PNG (sin abrir ventanas ni tocar el escritorio).
//!
//!     cargo run --example preview -- <carpeta-de-salida> [claro]

use std::io::Write;

use hypr_win::audio::Volume;
use hypr_win::bar::{self, BarView};
use hypr_win::power::Battery;
use hypr_win::status::Status;
use hypr_win::wifi::{self, Auth, Network};
use hypr_win::{apps, dock, icons, panel, search};
use hypr_win::panel::Mode;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use hypr_win::config::Config;
use hypr_win::gfx::{Canvas, Font, Theme};
use hypr_win::palette::{self, Fonts, Row, RowIcon, View as PView};

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend(data);
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
}

/// PNG RGB sin comprimir (bloques "stored" de zlib): suficiente para inspeccionar a ojo.
fn write_png(path: &str, w: u32, h: u32, rgb: &[u8]) {
    let mut raw = Vec::with_capacity((w * h * 3 + h) as usize);
    for row in rgb.chunks((w * 3) as usize) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend((b.len() as u16).to_le_bytes());
        z.extend((!(b.len() as u16)).to_le_bytes());
        z.extend(*b);
    }
    let (mut a, mut s) = (1u32, 0u32);
    for &b in &raw {
        a = (a + b as u32) % 65521;
        s = (s + a) % 65521;
    }
    z.extend(((s << 16) | a).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    std::fs::File::create(path).unwrap().write_all(&png).unwrap();
}

/// Compone el lienzo (ARGB premultiplicado) sobre un "fondo de pantalla" degradado.
fn save(c: &Canvas, path: &str) {
    let mut rgb = Vec::with_capacity((c.w * c.h * 3) as usize);
    for (i, &p) in c.pixels().iter().enumerate() {
        let (x, y) = ((i as i32 % c.w) as f32 / c.w as f32, (i as i32 / c.w) as f32 / c.h as f32);
        let bg = [(70.0 + 90.0 * x) as u32, (90.0 + 40.0 * y) as u32, (150.0 - 40.0 * x) as u32];
        let (a, r, g, b) = (p >> 24, (p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
        let inv = 255 - a;
        for (src, dst) in [(r, bg[0]), (g, bg[1]), (b, bg[2])] {
            rgb.push((src + dst * inv / 255).min(255) as u8);
        }
    }
    write_png(path, c.w as u32, c.h as u32, &rgb);
}

fn sample_networks() -> Vec<Network> {
    let n = |ssid: &str, signal, secured, connected, known| Network {
        ssid: ssid.into(),
        signal,
        secured,
        connected,
        known,
        auth: if secured { Auth::Psk } else { Auth::Open },
    };
    vec![
        n("Casa", 88, true, true, true),
        n("Vecino_5G", 61, true, false, false),
        n("Cafe Central", 40, false, false, false),
        n("Oficina", 22, true, false, true),
    ]
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let dir = std::env::args().nth(1).expect("uso: preview <carpeta> [claro]");
    let light = std::env::args().nth(2).as_deref() == Some("claro");
    let mut cfg = Config::default_config();
    if light {
        cfg.bar.background = "#f2f2f7".into();
        cfg.bar.foreground = "#1d1d1f".into();
        cfg.bar.dim = "#86868b".into();
        cfg.bar.accent = "#007aff".into();
    }
    let theme = Theme::from_cfg(&cfg.bar).unwrap();
    let suffix = if light { "-claro" } else { "" };

    // estado de ejemplo
    let status = Status {
        wifi: wifi::Status { present: true, enabled: true, connected: true, ssid: Some("Casa".into()), signal: 88, denied: false },
        volume: Some(Volume { level: 0.6, muted: false }),
        battery: Some(Battery { percent: 21, charging: true, ac: true }),
        bluetooth: hypr_win::bluetooth::Status { present: true, on: true, connected: vec!["AirPods Pro".into(), "Teclado MX".into()] },
    };

    // barra
    let font = Font::new(&theme.font, theme.base_px(), false).unwrap();
    let mut canvas = Canvas::new(1280, bar::BAR_PREVIEW_HEIGHT).unwrap();
    let occupied = [true, true, false, true, false, false, false, false, false];
    let clock = bar::clock_text();
    let view = BarView { occupied: &occupied, hl: 1.0, focused: true, title: "hypr-win — Visual Studio Code", right: &clock, status: &status };
    bar::render(&mut canvas, &theme, &font, cfg.bar.opacity, &view);
    save(&canvas, &format!("{dir}/bar{suffix}.png"));

    // paleta (Win+K y menú Win+Espacio)
    apps::start_indexing();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while apps::entries().is_empty() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let entries = apps::entries();
    let all_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    let pal_fonts = Fonts::new(&theme).unwrap();
    let save_palette = |file: &str, crumb: &str, placeholder: &str, query: &str, rows: &[Row], show_list: bool, sel: usize| {
        let icons_store: Vec<Option<icons::Icon>> =
            rows
            .iter()
            .map(|r| match &r.icon {
                RowIcon::App(id) => icons::from_apps_folder(id),
                RowIcon::Favicon(h) => hypr_win::webapp::cached_favicon(h).and_then(|p| icons::from_image_file(&p.to_string_lossy())),
                _ => None,
            })
            .collect();
        let icons: Vec<Option<&icons::Icon>> = icons_store.iter().map(|i| i.as_ref()).collect();
        let view = PView { crumb, placeholder, query, rows, icons: &icons, sel, top: 0, show_list, empty_msg: "Sin resultados" };
        let (w, h) = palette::canvas_size(&view);
        let mut c = Canvas::new(w, h).unwrap();
        palette::render(&mut c, &theme, &pal_fonts, &view);
        save(&c, &format!("{dir}/{file}{suffix}.png"));
    };
    // Spotlight: solo la barra, y con una consulta
    save_palette("spot-empty", "", "Buscar aplicaciones", "", &[], false, 0);
    let ranked: Vec<usize> = search::rank("e", &all_names, 6);
    let rows: Vec<Row> = ranked
        .iter()
        .enumerate()
        .map(|(i, &k)| {
            Row::new(entries[k].name.clone()).hint("Aplicación").icon(RowIcon::App(entries[k].id.clone())).section(if i == 0 { "Mejor resultado" } else { "Aplicaciones" })
        })
        .collect();
    save_palette("spot-query", "", "Buscar aplicaciones", "e", &rows, true, 0);
    // Menú: raíz, instalar, apps web, atajos y captura
    let g = |t: &str, s: &str, ch: char, rgb: u32| Row::new(t).subtitle(s).glyph(ch, rgb);
    let root = vec![
        g("Aplicaciones", "Buscar y abrir una aplicación", '\u{E71D}', 0x0A84FF).hint("›"),
        g("Instalar", "Programas con winget", '\u{E896}', 0x30D158).hint("›"),
        g("Aplicaciones web", "Convierte una página en una aplicación", '\u{E774}', 0xFF9F0A).hint("›"),
        g("Atajos", "Personaliza los atajos de teclado", '\u{E765}', 0xBF5AF2).hint("›"),
        g("Panel de control", "Wi-Fi, Bluetooth y sonido", '\u{E713}', 0x8E8E93),
        g("Sistema", "Bloquear, suspender, reiniciar…", '\u{E7E8}', 0xFF453A).hint("›"),
    ];
    save_palette("menu-root", "", "Buscar en el menú", "", &root, true, 1);
    let mut install = vec![g("Instalar «vlc» con winget", "winget install (se abre una consola)", '\u{E896}', 0x30D158).hint("Consola").section("Tu búsqueda")];
    for p in hypr_win::packages::CURATED.iter().take(6) {
        install.push(Row::new(p.name).subtitle(p.id).hint("Instalar").icon(RowIcon::Favicon(p.site.to_string())).section(p.section));
    }
    save_palette("menu-install", "Instalar", "Buscar o instalar un programa", "", &install, true, 1);
    // todos los favicons de la lista (comprobación de que cada dominio devuelve algo)
    for p in hypr_win::packages::CURATED {
        let ok = hypr_win::webapp::cached_favicon(p.site).is_some();
        println!("favicon {:<22} {:<26} {}", p.name, p.site, if ok { "ok" } else { "NO" });
    }
    let webs = vec![
        Row::new("YouTube").hint("Abrir").section("Mis aplicaciones web"),
        g("Nueva aplicación web…", "Nombre y dirección", '\u{E710}', 0x0A84FF).section("Crear"),
        g("Spotify", "open.spotify.com", '\u{E774}', 0xFF9F0A).hint("Crear").section("Sugeridas"),
        g("Classroom", "classroom.google.com", '\u{E774}', 0xFF9F0A).hint("Crear").section("Sugeridas"),
    ];
    save_palette("menu-web", "Apps web", "Aplicaciones web", "", &webs, true, 2);
    let keys = vec![
        g("Abrir wt.exe", "Super+Return", '\u{E765}', 0x8E8E93).hint("Cambiar"),
        g("Cerrar la ventana", "Super+Q", '\u{E765}', 0x8E8E93).hint("Cambiar"),
        g("Pantalla completa", "Super+F", '\u{E765}', 0x8E8E93).hint("Cambiar"),
        g("Menú", "Super+Space", '\u{E765}', 0x8E8E93).hint("Cambiar"),
    ];
    save_palette("menu-keys", "Atajos", "Buscar un atajo", "", &keys, true, 1);
    save_palette(
        "menu-capture",
        "Atajo",
        "Pulsa la nueva combinación…",
        "",
        &[g("Pulsa la combinación para «Cerrar la ventana»", "Esc para cancelar", '\u{E765}', 0xBF5AF2)],
        true,
        0,
    );

    // panel de control: principal y contraseña
    let pf = panel::Fonts::new(&theme).unwrap();
    let nets = sample_networks();
    for (file, mode, message) in [
        ("panel", Mode::Main, ""),
        ("panel-password", Mode::Password { ssid: "Vecino_5G".into(), input: "hunter2x".into() }, ""),
    ] {
        let view = panel::View { status: &status, networks: &nets, device: "Altavoces (Realtek Audio)", mode: &mode, message, scanning: false };
        let (w, h) = panel::canvas_size(&view);
        let mut c = Canvas::new(w, h).unwrap();
        panel::render(&mut c, &theme, &pf, &view);
        save(&c, &format!("{dir}/{file}{suffix}.png"));
    }

    // panel cuando Windows oculta las redes (sin permiso de ubicación)
    {
        let mut denied = status.clone();
        denied.wifi = wifi::Status { present: true, enabled: true, connected: true, ssid: None, signal: 0, denied: true };
        let view = panel::View { status: &denied, networks: &[], device: "Altavoces (Realtek Audio)", mode: &Mode::Main, message: "", scanning: false };
        let (w, h) = panel::canvas_size(&view);
        let mut c = Canvas::new(w, h).unwrap();
        panel::render(&mut c, &theme, &pf, &view);
        save(&c, &format!("{dir}/panel-denied{suffix}.png"));
    }

    // dock con iconos reales del sistema
    let mut shown: Vec<(String, Option<icons::Icon>, bool)> = Vec::new();
    for q in cfg.dock.pinned.iter().map(String::as_str).chain(["Visual Studio Code", "Notepad"]) {
        if let Some(&i) = search::rank(q, &all_names, 1).first() {
            let e = &entries[i];
            shown.push((e.name.clone(), icons::from_apps_folder(&e.id), shown.len().is_multiple_of(2)));
        }
    }
    let dfonts = dock::Fonts::new(&theme).unwrap();
    let metrics = dock::Metrics::new(&cfg.dock);
    let mut items: Vec<dock::ItemView> = Vec::new();
    for (i, (name, icon, running)) in shown.iter().enumerate() {
        if i == 5 {
            items.push(dock::ItemView { name: "", icon: None, running: false, bounce: 0.0, separator: true });
        }
        items.push(dock::ItemView { name, icon: icon.as_ref(), running: *running, bounce: 0.0, separator: false });
    }
    let n_icons = items.iter().filter(|i| !i.separator).count();
    let (w, h) = metrics.canvas_size(n_icons, items.len() - n_icons);
    let mut c = Canvas::new(w, h).unwrap();
    dock::render(&mut c, &theme, &dfonts, &metrics, &items, None, 0.0);
    save(&c, &format!("{dir}/dock{suffix}.png"));
    dock::render(&mut c, &theme, &dfonts, &metrics, &items, Some(w as f32 / 2.0 - 40.0), 1.0);
    save(&c, &format!("{dir}/dock-hover{suffix}.png"));
    println!("listo en {dir} ({} apps en el dock)", shown.len());
}
