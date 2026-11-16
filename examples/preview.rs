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
    let theme_name = std::env::args().nth(2);
    let mut cfg = Config::default_config();
    if let Some(n) = &theme_name {
        let n = if n == "claro" { "Apple Claro" } else { n.as_str() };
        let def = hypr_win::theme::find(n).unwrap_or_else(|| panic!("no existe el tema {n}"));
        hypr_win::theme::apply_to_bar(&def, &mut cfg.bar);
    }
    let theme = Theme::from_cfg(&cfg.bar).unwrap();
    let suffix = theme_name.map(|n| format!("-{}", n.replace(' ', "_"))).unwrap_or_default();

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
    let mk = |bubbles: bool, music: Option<bar::MusicView<'static>>| BarView {
        occupied: &occupied,
        hl: 1.0,
        focused: true,
        title: "hypr-win — Visual Studio Code",
        right: &clock,
        status: &status,
        bubbles,
        ink: cfg.bar.style == "ink" && !bubbles,
        music,
    };
    bar::render(&mut canvas, &theme, &font, cfg.bar.opacity, &mk(cfg.bar.style == "bubbles", None));
    save(&canvas, &format!("{dir}/bar{suffix}.png"));
    // el diseño de burbujas (Minimal 2), sea cual sea el configurado
    bar::render(&mut canvas, &theme, &font, cfg.bar.opacity, &mk(true, None));
    save(&canvas, &format!("{dir}/bar-bubbles{suffix}.png"));
    // con música sonando, en varios instantes de la animación (la onda se mueve)
    for (bubbles, name) in [(false, "bar"), (true, "bar-bubbles")] {
        for (k, t) in [0.4f32, 1.4, 2.6].into_iter().enumerate() {
            bar::render(&mut canvas, &theme, &font, cfg.bar.opacity, &mk(bubbles, Some(bar::MusicView { title: "Midnight City", t })));
            save(&canvas, &format!("{dir}/{name}-music{k}{suffix}.png"));
        }
    }

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
                RowIcon::File(path) => icons::thumbnail_from_file(path, 96),
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

    // páginas de temas del menú
    {
        let swatches: Vec<Row> = hypr_win::theme::builtins()
            .iter()
            .take(6)
            .enumerate()
            .map(|(i, d)| {
                let c = |h: &str| hypr_win::config::parse_color(h).unwrap();
                Row::new(d.name.clone())
                    .subtitle("Predefinido")
                    .hint(if i == 0 { "Activo" } else { "Aplicar" })
                    .icon(RowIcon::Swatch(c(&d.background), c(&d.accent), c(&d.foreground)))
                    .section("Predefinidos")
            })
            .collect();
        save_palette("menu-themes", "Temas", "Buscar un tema", "", &swatches, true, 2);
        let col = |h: &str| RowIcon::Color(hypr_win::config::parse_color(h).unwrap());
        let edit = vec![
            Row::new("Fondo").subtitle("Se guardará como un tema tuyo").hint("#1e1e20").icon(col("#1e1e20")).section("Propiedades"),
            Row::new("Texto").subtitle("Se guardará como un tema tuyo").hint("#f5f5f7").icon(col("#f5f5f7")).section("Propiedades"),
            Row::new("Acento").subtitle("Se guardará como un tema tuyo").hint("#0a84ff").icon(col("#0a84ff")).section("Propiedades"),
            Row::new("Fuente").subtitle("Se guardará como un tema tuyo").hint("Segoe UI Variable Text").glyph('\u{E8D2}', 0x8E8E93).section("Propiedades"),
            Row::new("Opacidad de la barra").subtitle("Se guardará como un tema tuyo").hint("80 %").glyph('\u{E706}', 0x8E8E93).section("Propiedades"),
        ];
        save_palette("menu-theme-edit", "Editar tema", "Elegir qué cambiar", "", &edit, true, 2);
        let accents: Vec<Row> = [("Azul", "#0a84ff"), ("Verde", "#30d158"), ("Naranja", "#ff9f0a"), ("Morado", "#bf5af2"), ("Rosa", "#ff375f")]
            .iter()
            .map(|(n, h)| Row::new(*n).subtitle(*h).hint("Aplicar").icon(col(h)).section("Colores de Apple"))
            .collect();
        save_palette("menu-theme-accent", "Acento", "Color hexadecimal (p. ej. #0a84ff)", "", &accents, true, 3);
    }

    // fondo de pantalla: degradados generados y la página del menú con miniaturas reales
    {
        use hypr_win::wallpaper::{self, Kind};
        let c = |h: &str| hypr_win::config::parse_color(h).unwrap();
        let (bg, ac, fg) = (c(&cfg.bar.background), c(&cfg.bar.accent), c(&cfg.bar.foreground));
        for kind in Kind::ALL {
            let (w, h) = (640usize, 360usize);
            let mut canvas = Canvas::new(w as i32, h as i32).unwrap();
            canvas.fill_from(&wallpaper::render(kind, w, h, bg, ac));
            canvas.save_png(&format!("{dir}/wp-{}{suffix}.png", kind.key()));
        }
        let mut rows: Vec<Row> = Kind::ALL
            .iter()
            .map(|k| Row::new(k.label()).subtitle("Con los colores del tema").hint("Generar").icon(RowIcon::Swatch(bg, ac, fg)).section("Fondo del tema"))
            .collect();
        for img in wallpaper::images("", 3) {
            rows.push(Row::new(img.name.clone()).subtitle(img.source).hint("Aplicar").icon(RowIcon::File(img.path.to_string_lossy().into_owned())).section("Imágenes"));
        }
        save_palette("menu-wallpaper", "Fondo", "Buscar una imagen o una opción", "", &rows, true, 4);
    }

    // ajustes
    {
        use hypr_win::settings::{self, Kind};
        let rows: Vec<Row> = settings::ALL
            .iter()
            .take(9)
            .map(|st| {
                let rgb = match st.group {
                    "Ventanas" => 0x0A84FF,
                    "Sistema" => 0xFF9F0A,
                    "Barra" => 0xBF5AF2,
                    _ => 0x30D158,
                };
                let glyph = if matches!(st.kind, Kind::Bool) { '\u{E73E}' } else { '\u{E713}' };
                let sub = if st.restart { format!("{} · requiere reiniciar", st.hint) } else { st.hint.to_string() };
                Row::new(st.label).subtitle(sub).hint(settings::get(&cfg, st)).glyph(glyph, rgb).section(st.group)
            })
            .collect();
        save_palette("menu-settings", "Ajustes", "Buscar un ajuste", "", &rows, true, 1);
    }

    // páginas de widgets e inicio de sesión
    {
        use hypr_win::widget_draw::{Kind, Size};
        let mut rows = vec![Row::new("Mostrar los widgets").subtitle("Detrás de las ventanas").hint("Visibles").glyph('\u{E7B3}', 0x0A84FF).section("Escritorio")];
        for k in Kind::ALL {
            let (g, rgb) = k.glyph();
            for &s in k.sizes().iter().take(1) {
                let _: Size = s;
                rows.push(Row::new(k.label()).subtitle("Tamaño inicial").hint("Añadir").glyph(g, rgb).section("Añadir"));
            }
        }
        save_palette("menu-widgets", "Widgets", "Buscar un widget", "", &rows, true, 1);
        let login = vec![
            Row::new("Imagen de la pantalla de bloqueo").subtitle("Windows Spotlight u otra").glyph('\u{E91B}', 0x8E8E93).section("Ahora"),
            Row::new("Usar el fondo actual del escritorio").subtitle("También en el inicio de sesión").glyph('\u{E7F4}', 0x0A84FF).section("Imagen"),
            Row::new("Fondo nítido en el inicio de sesión").subtitle("Directiva de Windows; pide administrador").hint("Desactivado").glyph('\u{E7B3}', 0xBF5AF2).section("Directiva"),
            Row::new("El diseño lo dibuja Windows").subtitle("No se puede sustituir con APIs seguras").glyph('\u{E946}', 0x8E8E93).section("Límites"),
        ];
        save_palette("menu-login", "Inicio de sesión", "Inicio de sesión", "", &login, true, 1);
    }

    // widgets de escritorio
    {
        use hypr_win::widget_draw::{self as wd, Day, Kind, Media, Now, Stats, Weather, WFonts};
        let wf = WFonts::new(&theme).unwrap();
        let now = Now { year: 2026, month: 10, day: 29, weekday: 3, hour: 19, min: 45, sec: 12 };
        let shortcuts: Vec<(String, String)> = cfg
            .bindings()
            .unwrap()
            .iter()
            .zip(cfg.binds.iter())
            .take(11)
            .map(|(b, raw)| (raw.split(',').next().unwrap_or("").trim().to_string(), hypr_win::keybinds::describe(&b.action)))
            .collect();
        let data = wd::Data {
            stats: Stats { cpu: 34.0, mem_pct: 62.0, mem_used_gb: 9.8, mem_total_gb: 16.0, disk_pct: 71.0, disk_free_gb: 142.0, net_down: 2.4 * 1024.0 * 1024.0, net_up: 120.0 * 1024.0 },
            weather: Some(Weather {
                city: "Madrid".into(),
                temp: 21,
                code: 2,
                days: vec![
                    Day { label: "Vie".into(), hi: 23, lo: 12, code: 0 },
                    Day { label: "Sáb".into(), hi: 19, lo: 11, code: 63 },
                    Day { label: "Dom".into(), hi: 17, lo: 9, code: 3 },
                ],
            }),
            media: Some(Media { title: "Blinding Lights".into(), artist: "The Weeknd".into(), playing: true, thumb: None }),
            note: "Comprar leche, llamar al dentista y preparar la presentación del viernes. Revisar el correo antes de las 10.".into(),
            shortcuts,
            city: "Madrid".into(),
        };
        for kind in Kind::ALL {
            for &size in kind.sizes() {
                let (w, h) = wd::canvas_size(size);
                let mut c = Canvas::new(w, h).unwrap();
                wd::render(&mut c, &theme, &wf, kind, size, &data, &now, None);
                save(&c, &format!("{dir}/widget-{:?}-{}{suffix}.png", kind, size.label()).to_lowercase());
            }
        }
    }

    // notificaciones de instalación en segundo plano
    {
        use hypr_win::toast::{self, ToastView};
        let tf = toast::Fonts::new(&theme).unwrap();
        let views = [
            ToastView { glyph: '\u{E896}', rgb: toast::BLUE, title: "Instalando Visual Studio Code", subtitle: "en segundo plano · winget" },
            ToastView { glyph: '\u{E73E}', rgb: toast::GREEN, title: "Git instalado", subtitle: "Ya aparece en el lanzador" },
            ToastView { glyph: '\u{E783}', rgb: toast::RED, title: "Falló: instalando Zoom", subtitle: "No se encontró un instalador aplicable" },
        ];
        let (w, h) = toast::canvas_size(views.len());
        let mut c = Canvas::new(w, h).unwrap();
        toast::render(&mut c, &theme, &tf, &views);
        save(&c, &format!("{dir}/toast{suffix}.png"));
    }

    // dock con iconos reales del sistema
    let mut shown: Vec<(String, Option<icons::Icon>, bool)> = Vec::new();
    for q in cfg.dock.pinned.iter().map(String::as_str).chain(["Visual Studio Code", "Notepad"]) {
        if let Some(&i) = search::rank(q, &all_names, 1).first() {
            let e = &entries[i];
            // los estilos `mono` y `accent` recolorean el icono, igual que hace el dock real
            let tint = match theme.app_icons.as_str() {
                "mono" => Some(theme.fg),
                "accent" => Some(theme.accent),
                _ => None,
            };
            let icon = icons::from_apps_folder(&e.id).map(|i| tint.and_then(|t| i.tinted(t)).unwrap_or(i));
            shown.push((e.name.clone(), icon, shown.len().is_multiple_of(2)));
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
    // Alt+Tab propio (las mismas apps del dock, con títulos de ventana de ejemplo)
    {
        use hypr_win::switcher::{self, ItemView};
        let sf = switcher::Fonts::new(&theme).unwrap();
        let sm = switcher::Metrics::new();
        let titles = ["hypr-win — Visual Studio Code", "Documentos", "Brave", "Terminal", "Spotify", "Notas", "Calculadora"];
        let views: Vec<ItemView> = shown.iter().zip(titles).map(|((_, icon, _), t)| ItemView { title: t, icon: icon.as_ref(), minimized: t == "Notas" }).collect();
        let cols = switcher::columns(views.len(), 1920, &sm);
        let (sw, sh) = switcher::canvas_size(views.len(), cols, &sm);
        let mut sc = Canvas::new(sw, sh).unwrap();
        switcher::render(&mut sc, &theme, &sf, &sm, &views, 1, cols);
        save(&sc, &format!("{dir}/switcher{suffix}.png"));
    }
    println!("listo en {dir} ({} apps en el dock)", shown.len());
}
