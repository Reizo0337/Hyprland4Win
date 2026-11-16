//! Menú del sistema (Super+Espacio), al estilo del menú de Omarchy: una paleta por niveles con
//! aplicaciones, instalación de programas (winget), aplicaciones web, atajos de teclado y sistema.

use std::path::PathBuf;

use crate::apps;
use crate::config::lit_str;
use crate::theme::{self, ThemeDef};
use crate::settings::{self, Kind as SettingKind};
use crate::wallpaper::{self, Fit, Kind as WpKind};
use crate::widget_draw::{Kind as WKind, Size as WSize};
use crate::installer::{self, Job, Kind};
use crate::keybinds::{self, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_WIN};
use crate::launcher::AppsHandler;
use crate::layout::Rect;
use crate::packages::{self, Manager};
use crate::pkgsearch::{self, State};
use crate::palette::{self, Handler, Outcome, Row, RowIcon};
use crate::search;
use crate::webapp::{self, WebApp};

/// Webs sugeridas: (nombre, dirección).
pub const WEB_PRESETS: &[(&str, &str)] = &[
    ("YouTube", "youtube.com"),
    ("Spotify", "open.spotify.com"),
    ("Classroom", "classroom.google.com"),
    ("WhatsApp", "web.whatsapp.com"),
    ("Gmail", "mail.google.com"),
    ("Google Calendar", "calendar.google.com"),
    ("Google Drive", "drive.google.com"),
    ("Google Meet", "meet.google.com"),
    ("ChatGPT", "chatgpt.com"),
    ("Notion", "notion.so"),
    ("Netflix", "netflix.com"),
    ("Twitch", "twitch.tv"),
    ("GitHub", "github.com"),
];

#[derive(Clone, PartialEq, Debug)]
enum Page {
    Root,
    Install,
    WebApps,
    WebRemove,
    WebName,
    WebUrl(String),
    Shortcuts,
    Capture(usize),
    System,
    Confirm(Sys),
    Themes,
    ThemeEdit,
    ThemeValue(Prop),
    ThemeName,
    ThemeDelete,
    Wallpaper,
    WpFit,
    WpInterval,
    WpFps,
    WpFolder,
    Settings,
    SettingValue(usize),
    Widgets,
    WidgetCity,
    WidgetNote,
    Login,
}

/// Propiedades editables de un tema.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prop {
    Background,
    Foreground,
    Dim,
    Accent,
    AccentText,
    Font,
    Icons,
    AppIcons,
    Opacity,
}

impl Prop {
    const ALL: [Prop; 9] = [Prop::Background, Prop::Foreground, Prop::Dim, Prop::Accent, Prop::AccentText, Prop::Font, Prop::Icons, Prop::AppIcons, Prop::Opacity];

    fn label(self) -> &'static str {
        match self {
            Prop::Background => "Fondo",
            Prop::Foreground => "Texto",
            Prop::Dim => "Texto secundario",
            Prop::Accent => "Acento",
            Prop::AccentText => "Texto sobre el acento",
            Prop::Font => "Fuente",
            Prop::Icons => "Iconos",
            Prop::AppIcons => "Iconos de las aplicaciones",
            Prop::Opacity => "Opacidad de la barra",
        }
    }

    fn is_color(self) -> bool {
        !matches!(self, Prop::Font | Prop::Icons | Prop::AppIcons | Prop::Opacity)
    }
}

/// Colores de acento sugeridos (los del sistema de Apple).
const ACCENT_PRESETS: &[(&str, &str)] = &[
    ("Azul", "#0a84ff"),
    ("Verde", "#30d158"),
    ("Naranja", "#ff9f0a"),
    ("Rojo", "#ff453a"),
    ("Morado", "#bf5af2"),
    ("Rosa", "#ff375f"),
    ("Turquesa", "#40c8e0"),
    ("Amarillo", "#ffd60a"),
    ("Gris", "#8e8e93"),
];

const FONT_PRESETS: &[&str] = &["Segoe UI Variable Text", "Segoe UI", "Cascadia Code", "Consolas", "Arial", "Georgia", "Verdana", "Trebuchet MS"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Sys {
    Sleep,
    SignOut,
    Restart,
    Shutdown,
}

#[derive(Clone)]
enum Target {
    Go(Page),
    OpenApps,
    OpenPanel,
    Pkg(&'static str, &'static str),
    PkgChoco(&'static str, &'static str),
    /// Resultado de una búsqueda en vivo: gestor e identificador exacto.
    PkgFound(Manager, String),
    ChocoUpgrade,
    WebLaunch(PathBuf),
    WebPreset(&'static str, &'static str),
    WebNext(String),
    WebCreate(String, String),
    WebDelete(usize),
    Shortcut(usize),
    OpenConfig,
    Lock,
    Confirm(Sys),
    Do(Sys),
    UpdateAll,
    Quit,
    UpdateCheck,
    ToggleAutostart,
    UpdateInstall,
    Setting(usize),
    SetSetting(usize, String),
    AddWidget(WKind, WSize),
    RemoveWidget(u32),
    ToggleWidgets,
    ResetWidgets,
    SetCity(String),
    SetNote(String),
    AppendNote(String),
    ClearNote,
    LockFromWallpaper,
    LockFromTheme,
    ToggleClearLogon,
    OpenUri(&'static str),
    WpApply(String),
    WpGenerate(WpKind),
    WpRandom,
    WpNextMonitor,
    WpSetFit(Fit),
    WpSetInterval(u32),
    WpAnimate(String),
    WpAnimateStop,
    WpSetFps(u32),
    WpSetFolder(String),
    WpToggleFollow,
    WpToggleLock,
    WpRestore,
    WpRestoreLock,
    ApplyTheme(String),
    SetBarStyle(String),
    EditProp(Prop),
    SetProp(Prop, String),
    OpenIconsFolder,
    ReloadIcons,
    NewTheme(String),
    DeleteTheme(String),
    Noop,
}

pub struct MenuHandler {
    page: Page,
    stack: Vec<Page>,
    targets: Vec<Target>,
    notice: String,
    /// Aplicaciones web de la última lista (para eliminar por índice).
    webapps: Vec<WebApp>,
    mon: Rect,
    /// Monitor al que se aplica el fondo (`None` = todos).
    wp_target: Option<usize>,
}

impl MenuHandler {
    pub fn new(mon: Rect) -> Self {
        Self { page: Page::Root, stack: Vec::new(), targets: Vec::new(), notice: String::new(), webapps: Vec::new(), mon, wp_target: None }
    }

    fn go(&mut self, page: Page) {
        let from = std::mem::replace(&mut self.page, page);
        self.stack.push(from);
        self.notice.clear();
    }
}

/// Abre el menú centrado en el monitor `mon`, o lo cierra si ya estaba abierto.
pub fn toggle(mon: Rect) {
    if palette::is_open() {
        palette::close(true);
    } else {
        palette::open(Box::new(MenuHandler::new(mon)), mon);
    }
}

fn glyph_row(title: &str, sub: &str, glyph: char, rgb: u32) -> Row {
    Row::new(title).subtitle(sub).glyph(glyph, rgb)
}

/// Filtra y ordena `items` por la consulta (la mejor coincidencia de título o texto extra primero).
fn filter(query: &str, items: Vec<(Row, Target, String)>) -> Vec<(Row, Target)> {
    let q = query.trim();
    if q.is_empty() {
        return items.into_iter().map(|(r, t, _)| (r, t)).collect();
    }
    let mut scored: Vec<(i32, (Row, Target))> = items
        .into_iter()
        .filter_map(|(r, t, extra)| {
            let s = search::score(q, &r.title).or_else(|| search::score(q, &extra))?;
            Some((s, (r, t)))
        })
        .collect();
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.into_iter().map(|(_, x)| x).collect()
}

impl Handler for MenuHandler {
    fn placeholder(&self) -> String {
        match &self.page {
            Page::Root => "Buscar en el menú",
            Page::Install => "Buscar o instalar un programa",
            Page::WebApps => "Aplicaciones web",
            Page::WebRemove => "Elegir la aplicación web a eliminar",
            Page::WebName => "Nombre de la aplicación (p. ej. YouTube)",
            Page::WebUrl(_) => "Dirección (p. ej. youtube.com)",
            Page::Shortcuts => "Buscar un atajo",
            Page::Capture(_) => "Pulsa la nueva combinación…",
            Page::System => "Sistema",
            Page::Confirm(_) => "¿Seguro?",
            Page::Themes => "Buscar un tema",
            Page::ThemeEdit => "Elegir qué cambiar",
            Page::ThemeValue(Prop::Font) => "Nombre de la fuente (p. ej. Cascadia Code)",
            Page::ThemeValue(Prop::Opacity) => "Opacidad de 20 a 100",
            Page::ThemeValue(_) => "Color hexadecimal (p. ej. #0a84ff)",
            Page::ThemeName => "Nombre del tema nuevo",
            Page::ThemeDelete => "Elegir el tema a eliminar",
            Page::Wallpaper => "Buscar una imagen o una opción",
            Page::WpFit => "Cómo se ajusta la imagen",
            Page::WpInterval => "Cada cuánto cambia el fondo",
            Page::WpFps => "Fotogramas por segundo del fondo animado",
            Page::WpFolder => "Ruta de la carpeta (p. ej. D:\\Fondos)",
            Page::Settings => "Buscar un ajuste",
            Page::Widgets => "Buscar un widget",
            Page::WidgetCity => "Ciudad (p. ej. Madrid)",
            Page::WidgetNote => "Texto de la nota",
            Page::Login => "Inicio de sesión",
            Page::SettingValue(i) => {
                return match &settings::ALL[*i].kind {
                    SettingKind::Int { min, max, unit } => format!("Valor de {min} a {max} {unit}").trim().to_string(),
                    SettingKind::Float { min, max } => format!("Número de {min} a {max}"),
                    SettingKind::Bool => "Sí o no".to_string(),
                    SettingKind::Text => "Texto".to_string(),
                    SettingKind::List => "Nombres separados por comas".to_string(),
                };
            }
        }
        .into()
    }

    fn crumb(&self) -> String {
        match &self.page {
            Page::Root => "",
            Page::Install => "Instalar",
            Page::WebApps | Page::WebRemove => "Apps web",
            Page::WebName | Page::WebUrl(_) => "Nueva app web",
            Page::Shortcuts => "Atajos",
            Page::Capture(_) => "Atajo",
            Page::System | Page::Confirm(_) => "Sistema",
            Page::Themes | Page::ThemeDelete => "Temas",
            Page::ThemeEdit | Page::ThemeName => "Editar tema",
            Page::ThemeValue(p) => p.label(),
            Page::Wallpaper | Page::WpFit | Page::WpInterval | Page::WpFps | Page::WpFolder => "Fondo",
            Page::Settings => "Ajustes",
            Page::SettingValue(i) => settings::ALL[*i].label,
            Page::Widgets => "Widgets",
            Page::WidgetCity | Page::WidgetNote => "Widgets",
            Page::Login => "Inicio de sesión",
        }
        .into()
    }

    fn show_on_empty(&self) -> bool {
        true
    }

    fn empty_message(&self) -> String {
        match &self.page {
            Page::WebName => "Escribe un nombre para la aplicación".into(),
            Page::WebUrl(_) => "Escribe la dirección de la página (p. ej. youtube.com)".into(),
            Page::ThemeName => "Escribe un nombre para el tema".into(),
            Page::Wallpaper => "No hay imágenes: pon algunas en Imágenes\\Wallpapers o elige otra carpeta".into(),
            Page::WpFolder => "Escribe la ruta de una carpeta que exista".into(),
            Page::SettingValue(_) => "Escribe el nuevo valor".into(),
            Page::WidgetCity => "Escribe el nombre de tu ciudad".into(),
            Page::WidgetNote => "Escribe el texto de la nota".into(),
            Page::ThemeValue(Prop::Opacity) => "Escribe un número entre 20 y 100".into(),
            Page::ThemeValue(Prop::Font) => "Escribe el nombre de una fuente instalada".into(),
            Page::ThemeValue(_) => "Escribe un color: #rrggbb o #rgb".into(),
            _ => "Sin resultados".into(),
        }
    }

    fn rows(&mut self, query: &str) -> Vec<Row> {
        let q = query.trim().to_string();
        let mut items: Vec<(Row, Target, String)> = Vec::new();
        let mut add = |r: Row, t: Target, extra: &str| items.push((r, t, extra.to_string()));
        let notice = self.notice.clone();

        match self.page.clone() {
            Page::Root => {
                add(glyph_row("Aplicaciones", "Buscar y abrir una aplicación", '\u{E71D}', 0x0A84FF).hint("›"), Target::OpenApps, "abrir lanzar apps");
                add(glyph_row("Instalar", "Programas con winget", '\u{E896}', 0x30D158).hint("›"), Target::Go(Page::Install), "winget descargar programas");
                add(glyph_row("Aplicaciones web", "Convierte una página en una aplicación", '\u{E774}', 0xFF9F0A).hint("›"), Target::Go(Page::WebApps), "web youtube spotify classroom");
                add(glyph_row("Atajos", "Personaliza los atajos de teclado", '\u{E765}', 0xBF5AF2).hint("›"), Target::Go(Page::Shortcuts), "teclado teclas combinaciones");
                add(glyph_row("Widgets", "Reloj, calendario, tiempo, música, sistema y más", '\u{E80F}', 0x30D158).hint("›"), Target::Go(Page::Widgets), "escritorio tarjetas");
                add(glyph_row("Inicio de sesión", "Imagen de bloqueo y fondo nítido", '\u{E77B}', 0x0A84FF).hint("›"), Target::Go(Page::Login), "login pantalla bloqueo contraseña usuario");
                add(glyph_row("Ajustes", "Huecos, animaciones, barra, dock y más", '\u{E713}', 0x8E8E93).hint("›"), Target::Go(Page::Settings), "configuracion opciones gaps");
                add(glyph_row("Fondo de pantalla", "Imágenes, degradados del tema y cambio automático", '\u{E91B}', 0x5E5CE6).hint("›"), Target::Go(Page::Wallpaper), "wallpaper fondo escritorio imagen");
                add(glyph_row("Temas", "Colores y fuente de toda la interfaz", '\u{E790}', 0xFF375F).hint("›"), Target::Go(Page::Themes), "colores estilo apariencia oscuro claro");
                add(glyph_row("Panel de control", "Wi-Fi, Bluetooth y sonido", '\u{E701}', 0x8E8E93), Target::OpenPanel, "wifi bluetooth volumen bateria");
                add(glyph_row("Sistema", "Bloquear, suspender, reiniciar…", '\u{E7E8}', 0xFF453A).hint("›"), Target::Go(Page::System), "apagar reiniciar bloquear actualizar salir");
            }
            Page::Install => {
                let manager = packages::preferred();
                if manager.is_none() {
                    add(
                        glyph_row("No hay gestor de paquetes", "Instala «Instalador de aplicaciones» (winget) o Chocolatey", '\u{E783}', 0xFF453A).section("Aviso"),
                        Target::Noop,
                        "",
                    );
                }
                if q.chars().count() == 1 {
                    add(glyph_row("Escribe al menos dos letras", "Se buscará en winget y en Chocolatey", '\u{E721}', 0x8E8E93), Target::Noop, "");
                }
                if q.chars().count() >= 2 {
                    let installed = apps::entries();
                    // winget primero y Chocolatey debajo
                    for (mgr, available, section) in [
                        (Manager::Winget, packages::winget_available(), "winget"),
                        (Manager::Choco, packages::choco_available(), "Chocolatey"),
                    ] {
                        if !available {
                            continue;
                        }
                        match pkgsearch::lookup(mgr, &q) {
                            State::Pending => add(glyph_row(&format!("Buscando en {section}…"), "", '\u{E895}', 0x8E8E93).section(section), Target::Noop, ""),
                            State::Failed(e) => add(glyph_row(&format!("No se pudo buscar en {section}"), &e, '\u{E783}', 0xFF453A).section(section), Target::Noop, ""),
                            State::Done(found) if found.is_empty() => {
                                add(glyph_row(&format!("Sin resultados en {section}"), "", '\u{E711}', 0x8E8E93).section(section), Target::Noop, "")
                            }
                            State::Done(found) => {
                                for f in found {
                                    let icon = packages::CURATED
                                        .iter()
                                        .find(|p| p.id == f.id)
                                        .map(|p| RowIcon::Favicon(p.site.to_string()))
                                        .or_else(|| installed.iter().find(|e| e.name.eq_ignore_ascii_case(&f.name)).map(|e| RowIcon::App(e.id.clone())))
                                        .unwrap_or(RowIcon::Letter);
                                    let hint = if mgr == Manager::Choco { "Instalar (admin)" } else { "Instalar" };
                                    let sub = if f.name == f.id { f.version.clone() } else { format!("{}  ·  {}", f.id, f.version) };
                                    add(Row::new(f.name.clone()).subtitle(sub).hint(hint).icon(icon).section(section), Target::PkgFound(mgr, f.id.clone()), "");
                                }
                            }
                        }
                    }
                }
                // icono: el de la app si ya está instalada; si no, el favicon del fabricante (en segundo plano)
                let installed = apps::entries();
                for p in packages::CURATED.iter().filter(|_| q.is_empty()) {
                    let icon = installed
                        .iter()
                        .find(|e| e.name.eq_ignore_ascii_case(p.name))
                        .map(|e| RowIcon::App(e.id.clone()))
                        .unwrap_or_else(|| RowIcon::Favicon(p.site.to_string()));
                    let (sub, hint, target) = match manager {
                        Some(Manager::Choco) => (p.choco, "Chocolatey", Target::PkgChoco(p.choco, p.name)),
                        _ => (p.id, "Instalar", Target::Pkg(p.id, p.name)),
                    };
                    add(Row::new(p.name).subtitle(sub).hint(hint).icon(icon).section(p.section), target, p.id);
                }
            }
            Page::WebApps => {
                if !notice.is_empty() {
                    add(glyph_row(&notice, "", '\u{E73E}', 0x30D158).section("Hecho"), Target::Noop, "");
                }
                let mine = webapp::list();
                for a in &mine {
                    add(Row::new(a.name.clone()).hint("Abrir").section("Mis aplicaciones web"), Target::WebLaunch(a.path.clone()), "");
                }
                add(glyph_row("Nueva aplicación web…", "Nombre y dirección", '\u{E710}', 0x0A84FF).section("Crear"), Target::Go(Page::WebName), "crear añadir");
                if !mine.is_empty() {
                    add(glyph_row("Eliminar una aplicación web…", "", '\u{E74D}', 0xFF453A).section("Crear"), Target::Go(Page::WebRemove), "borrar quitar");
                }
                for (name, url) in WEB_PRESETS {
                    if !mine.iter().any(|a| a.name.eq_ignore_ascii_case(name)) {
                        add(glyph_row(name, url, '\u{E774}', 0xFF9F0A).hint("Crear").section("Sugeridas"), Target::WebPreset(name, url), url);
                    }
                }
            }
            Page::WebRemove => {
                self.webapps = webapp::list();
                for (i, a) in self.webapps.iter().enumerate() {
                    add(glyph_row(&a.name, "", '\u{E74D}', 0xFF453A).hint("Eliminar"), Target::WebDelete(i), "");
                }
            }
            Page::WebName => {
                if !q.is_empty() {
                    add(glyph_row(&format!("Continuar con «{q}»"), "Paso 1 de 2: nombre", '\u{E72A}', 0x0A84FF).hint("Siguiente"), Target::WebNext(q.clone()), &q);
                }
            }
            Page::WebUrl(name) => {
                if !notice.is_empty() {
                    add(glyph_row(&notice, "", '\u{E783}', 0xFF453A).section("Error"), Target::Noop, "");
                }
                if let Some(url) = webapp::normalize_url(&q) {
                    add(glyph_row(&format!("Crear «{name}»"), &url, '\u{E710}', 0x30D158).hint("Crear"), Target::WebCreate(name.clone(), url), &q);
                }
            }
            Page::Shortcuts => {
                for (i, (raw, b)) in crate::app::bindings_raw().into_iter().enumerate() {
                    let keys = raw.split(',').next().unwrap_or("").trim().to_string();
                    add(glyph_row(&keybinds::describe(&b.action), &keys, '\u{E765}', 0x8E8E93).hint("Cambiar"), Target::Shortcut(i), &keys);
                }
                add(glyph_row("Abrir config.toml", "Editar los atajos a mano", '\u{E8A5}', 0x0A84FF).section("Avanzado"), Target::OpenConfig, "archivo configuracion");
            }
            Page::Capture(i) => {
                let what = crate::app::bindings_raw().get(i).map(|(_, b)| keybinds::describe(&b.action)).unwrap_or_default();
                if !notice.is_empty() {
                    add(glyph_row(&notice, "", '\u{E783}', 0xFF453A).section("Aviso"), Target::Noop, "");
                }
                add(glyph_row(&format!("Pulsa la combinación para «{what}»"), "Esc para cancelar", '\u{E765}', 0xBF5AF2), Target::Noop, "");
            }
            Page::System => {
                add(glyph_row("Bloquear", "Bloquear la sesión", '\u{E72E}', 0x0A84FF), Target::Lock, "bloquear sesion");
                add(glyph_row("Suspender", "", '\u{E708}', 0x5E5CE6).hint("›"), Target::Confirm(Sys::Sleep), "dormir");
                add(glyph_row("Cerrar sesión", "", '\u{E7E8}', 0xFF9F0A).hint("›"), Target::Confirm(Sys::SignOut), "logout");
                add(glyph_row("Reiniciar", "", '\u{E777}', 0xFF9F0A).hint("›"), Target::Confirm(Sys::Restart), "reboot");
                add(glyph_row("Apagar", "", '\u{E7E8}', 0xFF453A).hint("›"), Target::Confirm(Sys::Shutdown), "shutdown");
                add(glyph_row("Actualizar aplicaciones", "winget upgrade --all, en segundo plano", '\u{E895}', 0x30D158).hint("Segundo plano"), Target::UpdateAll, "winget actualizar");
                if packages::choco_available() {
                    add(glyph_row("Actualizar con Chocolatey", "choco upgrade all (administrador, en segundo plano)", '\u{E895}', 0xFF9F0A).hint("Admin"), Target::ChocoUpgrade, "choco actualizar");
                }
                if let Some(r) = crate::updater::available() {
                    let mb = r.size as f32 / 1_048_576.0;
                    add(
                        glyph_row(&format!("Actualizar a v{}", r.version), &format!("Descarga {mb:.1} MB, verifica la suma SHA-256 y reinicia hypr-win"), '\u{E896}', 0x30D158).hint("Instalar").section("Actualizaciones"),
                        Target::UpdateInstall,
                        "actualizar update version nueva instalar",
                    );
                }
                let auto = crate::autostart::is_enabled();
                add(
                    glyph_row("Iniciar con Windows", if auto { "hypr-win arranca solo al iniciar sesión" } else { "Arrancar hypr-win al iniciar sesión" }, '\u{E7E8}', 0x30D158)
                        .hint(if auto { "Activado" } else { "Desactivado" })
                        .section("Inicio"),
                    Target::ToggleAutostart,
                    "inicio arrancar automatico windows sesion autostart",
                );
                add(
                    glyph_row("Buscar actualizaciones", &format!("Versión instalada: v{}", crate::updater::CURRENT), '\u{E895}', 0x0A84FF).section("Actualizaciones"),
                    Target::UpdateCheck,
                    "actualizar update version buscar",
                );
                add(glyph_row("Salir de hypr-win", "Restaura las ventanas y la barra de tareas", '\u{E711}', 0x8E8E93), Target::Quit, "cerrar");
            }
            Page::Themes => {
                let active = app_bar().theme;
                // diseño de la barra: independiente de los colores (no cambia con el tema)
                let style = app_bar().style;
                for (name, key, subtitle) in [("Minimal 1", "bar", "Barra continua"), ("Minimal 2", "bubbles", "Burbujas separadas"), ("Tinta", "ink", "Numerales kanji y sello rojo")] {
                    let hint = if style == key { "Activo" } else { "Aplicar" };
                    add(Row::new(name).subtitle(subtitle).hint(hint).section("Diseño de la barra"), Target::SetBarStyle(key.into()), "diseño barra estilo burbuja");
                }
                for d in theme::all() {
                    let (bg, ac, fg) = (color(&d.background), color(&d.accent), color(&d.foreground));
                    let kind = if theme::is_builtin(&d.name) && !theme::user_themes().iter().any(|u| u.name == d.name) { "Predefinido" } else { "Tuyo" };
                    let is_active = d.name.eq_ignore_ascii_case(&active);
                    let hint = if is_active { "Activo" } else { "Aplicar" };
                    let section = if kind == "Tuyo" { "Mis temas" } else { "Predefinidos" };
                    add(Row::new(d.name.clone()).subtitle(kind).hint(hint).icon(RowIcon::Swatch(bg, ac, fg)).section(section), Target::ApplyTheme(d.name.clone()), kind);
                }
                add(glyph_row("Nuevo tema…", "Parte del tema actual y lo editas", '\u{E710}', 0x0A84FF).section("Crear"), Target::Go(Page::ThemeName), "crear");
                add(glyph_row("Editar el tema actual…", "Colores, fuente, iconos y opacidad", '\u{E70F}', 0xBF5AF2).section("Crear"), Target::Go(Page::ThemeEdit), "editar colores");
                if !theme::user_themes().is_empty() {
                    add(glyph_row("Eliminar un tema…", "", '\u{E74D}', 0xFF453A).section("Crear"), Target::Go(Page::ThemeDelete), "borrar quitar");
                }
            }
            Page::ThemeEdit => {
                if !notice.is_empty() {
                    add(glyph_row(&notice, "", '\u{E73E}', 0x30D158).section("Hecho"), Target::Noop, "");
                }
                let bar = app_bar();
                for prop in Prop::ALL {
                    let value = match prop {
                        Prop::Background => bar.background.clone(),
                        Prop::Foreground => bar.foreground.clone(),
                        Prop::Dim => bar.dim.clone(),
                        Prop::Accent => bar.accent.clone(),
                        Prop::AccentText => bar.accent_text.clone(),
                        Prop::Font => bar.font.clone(),
                        Prop::Icons => crate::iconset::label(&bar.icons).to_string(),
                        Prop::AppIcons => app_style_label(&bar.app_icons).to_string(),
                        Prop::Opacity => format!("{} %", bar.opacity),
                    };
                    let row = Row::new(prop.label()).subtitle(if theme::is_builtin(&bar.theme) { "Se guardará como un tema tuyo" } else { "Se guarda en tu tema" }).hint(value.clone());
                    let row = if prop.is_color() {
                        row.icon(RowIcon::Color(color(&value)))
                    } else if prop == Prop::Font {
                        row.glyph('\u{E8D2}', 0x8E8E93)
                    } else if matches!(prop, Prop::Icons | Prop::AppIcons) {
                        row.glyph('\u{E71D}', 0x8E8E93)
                    } else {
                        row.glyph('\u{E706}', 0x8E8E93)
                    };
                    add(row.section("Propiedades"), Target::EditProp(prop), prop.label());
                }
            }
            Page::ThemeValue(prop) => {
                match prop {
                    Prop::Font => {
                        if !q.is_empty() {
                            add(glyph_row(&format!("Usar «{q}»"), "Debe estar instalada", '\u{E8D2}', 0x30D158).hint("Aplicar"), Target::SetProp(prop, q.clone()), &q);
                        } else {
                            for f in FONT_PRESETS {
                                add(glyph_row(f, "", '\u{E8D2}', 0x8E8E93).hint("Aplicar").section("Sugeridas"), Target::SetProp(prop, f.to_string()), f);
                            }
                        }
                    }
                    Prop::Icons => {
                        let current = app_bar().icons;
                        let hint = |on: bool| if on { "Actual" } else { "Aplicar" };
                        add(
                            glyph_row(crate::iconset::SYSTEM_LABEL, "Segoe Fluent Icons, los de Windows", '\u{E71D}', 0x8E8E93).hint(hint(crate::iconset::find(&current).is_none())).section("Iconos"),
                            Target::SetProp(prop, String::new()),
                            "windows segoe",
                        );
                        for pack in &crate::iconset::PACKS {
                            add(
                                glyph_row(pack.label, pack.style, '\u{E71D}', 0x0A84FF).hint(hint(pack.key.eq_ignore_ascii_case(&current))).section("Iconos"),
                                Target::SetProp(prop, pack.key.to_string()),
                                &format!("{} {}", pack.label, pack.style),
                            );
                        }
                    }
                    Prop::AppIcons => {
                        let current = app_bar().app_icons;
                        let current = if current.is_empty() { "original" } else { current.as_str() };
                        for (key, label, sub) in crate::icons::APP_STYLES {
                            let hint = if key.eq_ignore_ascii_case(current) { "Actual" } else { "Aplicar" };
                            add(glyph_row(label, sub, '\u{E71D}', 0x0A84FF).hint(hint).section("Estilo"), Target::SetProp(prop, key.to_string()), label);
                        }
                        add(glyph_row("Abrir la carpeta de iconos propios", "Un .png o .ico con el nombre de la app (p. ej. Spotify.png)", '\u{E8B7}', 0x8E8E93).hint("Abrir").section("Tus iconos"), Target::OpenIconsFolder, "carpeta propios");
                        add(glyph_row("Recargar los iconos", "Después de añadir o quitar iconos propios", '\u{E895}', 0x30D158).section("Tus iconos"), Target::ReloadIcons, "actualizar");
                    }
                    Prop::Opacity => {
                        if let Some(n) = q.parse::<u8>().ok().filter(|n| (20..=100).contains(n)) {
                            add(glyph_row(&format!("Aplicar {n} %"), "Más bajo = más translúcida", '\u{E706}', 0x30D158).hint("Aplicar"), Target::SetProp(prop, n.to_string()), &q);
                        }
                    }
                    _ => {
                        if let Some(hex) = theme::normalize_hex(&q) {
                            add(Row::new(format!("Aplicar {hex}")).subtitle("Se guarda en tu tema").hint("Aplicar").icon(RowIcon::Color(color(&hex))), Target::SetProp(prop, hex), &q);
                        } else if q.is_empty() && prop == Prop::Accent {
                            for (name, hex) in ACCENT_PRESETS {
                                add(Row::new(*name).subtitle(*hex).hint("Aplicar").icon(RowIcon::Color(color(hex))).section("Colores de Apple"), Target::SetProp(prop, hex.to_string()), hex);
                            }
                        }
                    }
                }
            }
            Page::ThemeName => {
                if !q.is_empty() {
                    add(glyph_row(&format!("Crear «{q}»"), "Parte del tema actual", '\u{E710}', 0x30D158).hint("Crear"), Target::NewTheme(q.clone()), &q);
                }
            }
            Page::Wallpaper => {
                let cfg = crate::app::wallpaper_cfg();
                let monitors = wallpaper::connected_monitors();
                if monitors.len() > 1 {
                    let target = match self.wp_target {
                        Some(i) if i < monitors.len() => format!("Monitor {}", i + 1),
                        _ => "Todos los monitores".to_string(),
                    };
                    add(glyph_row("Aplicar a", &target, '\u{E7F4}', 0x0A84FF).hint("Cambiar").section("Destino"), Target::WpNextMonitor, "monitor pantalla destino");
                }
                let bar = app_bar();
                let name = if bar.theme.is_empty() { "tu tema".to_string() } else { bar.theme.clone() };
                let (bg, ac, fg) = (color(&bar.background), color(&bar.accent), color(&bar.foreground));
                for kind in WpKind::ALL {
                    add(
                        Row::new(kind.label()).subtitle(format!("Con los colores de «{name}»")).hint("Generar").icon(RowIcon::Swatch(bg, ac, fg)).section("Fondo del tema"),
                        Target::WpGenerate(kind),
                        "generar degradado tema",
                    );
                }
                let playing = crate::livewall::current();
                for kind in WpKind::ALL.into_iter().filter(|k| wallpaper::is_animatable(*k)) {
                    let src = crate::livewall::Source::Procedural(kind);
                    let on = playing.as_ref() == Some(&src);
                    add(
                        Row::new(src.label()).subtitle(format!("Se mueve con los colores de «{name}»")).hint(if on { "En marcha" } else { "Animar" }).icon(RowIcon::Swatch(bg, ac, fg)).section("Fondo animado"),
                        Target::WpAnimate(src.key()),
                        "animado movimiento vivo live",
                    );
                }
                for gif in wallpaper::animated_files(&cfg.dir, 100) {
                    let key = gif.path.to_string_lossy().into_owned();
                    let on = playing.as_ref().is_some_and(|s| s.key() == key);
                    add(Row::new(gif.name.clone()).subtitle(gif.source).hint(if on { "En marcha" } else { "Animar" }).icon(RowIcon::File(key.clone())).section("Fondo animado"), Target::WpAnimate(key), "gif animado movimiento");
                }
                {
                    let on = playing == Some(crate::livewall::Source::Samurai);
                    add(
                        Row::new("Samurái · noche").subtitle("Luna, torii, niebla y un samurái; pixel art").hint(if on { "En marcha" } else { "Animar" }).icon(RowIcon::Swatch(bg, ac, fg)).section("Fondo animado"),
                        Target::WpAnimate("samurai".into()),
                        "samurai katana japones torii luna animado",
                    );
                }
                if let Some(src) = &playing {
                    add(glyph_row("Detener el fondo animado", &format!("Ahora: {}", src.label()), '\u{E74D}', 0xFF453A).section("Fondo animado"), Target::WpAnimateStop, "parar quitar apagar animado");
                    add(glyph_row("Fotogramas por segundo", &format!("{} fps (los GIF usan los suyos)", cfg.animated_fps), '\u{E916}', 0x8E8E93).hint("›").section("Fondo animado"), Target::Go(Page::WpFps), "fps velocidad rendimiento");
                } else {
                    add(glyph_row("Para usar un GIF", "Ponlo en tu carpeta de imágenes; los .gif aparecen aquí", '\u{E946}', 0x8E8E93).section("Fondo animado"), Target::Noop, "");
                }
                for img in wallpaper::images(&cfg.dir, 300) {
                    let path = img.path.to_string_lossy().into_owned();
                    add(Row::new(img.name.clone()).subtitle(img.source).hint("Aplicar").icon(RowIcon::File(path.clone())).section("Imágenes"), Target::WpApply(path), img.source);
                }
                let fit = Fit::parse(&cfg.fit).unwrap_or(Fit::Fill);
                add(glyph_row("Fondo al azar", "Una imagen de tus carpetas", '\u{E8B1}', 0x30D158).section("Opciones"), Target::WpRandom, "aleatorio sorpresa");
                add(glyph_row("Ajuste de la imagen", fit.label(), '\u{E740}', 0x8E8E93).hint("›").section("Opciones"), Target::Go(Page::WpFit), "rellenar ajustar expandir mosaico centrar");
                let every = if cfg.interval_minutes == 0 { "Desactivado".to_string() } else { format!("Cada {} min", cfg.interval_minutes) };
                add(glyph_row("Cambio automático", &every, '\u{E916}', 0x8E8E93).hint("›").section("Opciones"), Target::Go(Page::WpInterval), "intervalo presentacion slideshow");
                let follow = if cfg.follow_theme { "Sí: genera el fondo del tema al cambiar de tema" } else { "No" };
                add(glyph_row("Cambiar el fondo con el tema", follow, '\u{E790}', 0xFF375F).hint(if cfg.follow_theme { "Activado" } else { "Desactivado" }).section("Opciones"), Target::WpToggleFollow, "seguir tema");
                add(
                    glyph_row("Aplicar también a la pantalla de bloqueo", "Win+L · al usarlo, Windows pasa de Spotlight a Imagen", '\u{E72E}', 0x0A84FF)
                        .hint(if cfg.lock_screen { "Activado" } else { "Desactivado" })
                        .section("Opciones"),
                    Target::WpToggleLock,
                    "bloqueo win+l login",
                );
                let folder = wallpaper::user_dir(&cfg.dir).map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                add(glyph_row("Carpeta de imágenes", &folder, '\u{E8B7}', 0x8E8E93).hint("›").section("Opciones"), Target::Go(Page::WpFolder), "directorio carpeta");
                if wallpaper::has_original_lock() {
                    add(glyph_row("Restaurar la pantalla de bloqueo anterior", "Vuelve a su imagen previa (como Imagen, no Spotlight)", '\u{E7A7}', 0xFF9F0A).section("Opciones"), Target::WpRestoreLock, "deshacer bloqueo");
                }
                if wallpaper::has_original() {
                    add(glyph_row("Restaurar el fondo original", "El que tenías antes de usar hypr-win", '\u{E7A7}', 0xFF9F0A).section("Opciones"), Target::WpRestore, "deshacer volver");
                }
            }
            Page::Settings => {
                let cfg = crate::app::config_snapshot();
                for (i, st) in settings::ALL.iter().enumerate() {
                    let rgb = match st.group {
                        "Ventanas" => 0x0A84FF,
                        "Sistema" => 0xFF9F0A,
                        "Barra" => 0xBF5AF2,
                        _ => 0x30D158,
                    };
                    let glyph = if matches!(st.kind, SettingKind::Bool) { '\u{E73E}' } else { '\u{E713}' };
                    let sub = if st.restart { format!("{} · requiere reiniciar", st.hint) } else { st.hint.to_string() };
                    add(Row::new(st.label).subtitle(sub).hint(settings::get(&cfg, st)).glyph(glyph, rgb).section(st.group), Target::Setting(i), st.key);
                }
                add(glyph_row("Abrir config.toml", "Todas las demás opciones y los comentarios", '\u{E8A5}', 0x0A84FF).section("Avanzado"), Target::OpenConfig, "archivo editar");
            }
            Page::SettingValue(i) => {
                let st = &settings::ALL[i];
                if !notice.is_empty() {
                    add(glyph_row(&notice, "", '\u{E783}', 0xFF453A).section("Aviso"), Target::Noop, "");
                }
                if !q.is_empty() {
                    match settings::parse(st, &q) {
                        Ok(lit) => add(glyph_row(&format!("Aplicar «{q}»"), &format!("{} = {lit}", st.key), '\u{E73E}', 0x30D158).hint("Aplicar"), Target::SetSetting(i, lit), &q),
                        Err(e) => add(glyph_row(&e, "", '\u{E783}', 0xFF453A), Target::Noop, ""),
                    }
                } else {
                    let cur = settings::get(&crate::app::config_snapshot(), st);
                    add(glyph_row(&format!("Ahora: {cur}"), st.hint, '\u{E713}', 0x8E8E93), Target::Noop, "");
                }
            }
            Page::Widgets => {
                let on = crate::widgets::visible();
                add(glyph_row("Mostrar los widgets", "Detrás de las ventanas; Super+Shift+W los trae delante", '\u{E7B3}', 0x0A84FF).hint(if on { "Visibles" } else { "Ocultos" }).section("Escritorio"), Target::ToggleWidgets, "ocultar mostrar");
                for p in crate::widgets::list() {
                    let (glyph, rgb) = p.kind.glyph();
                    add(
                        glyph_row(&format!("{} · {}", p.kind.label(), p.size.label()), &format!("Monitor {} · arrástralo para moverlo", p.monitor + 1), glyph, rgb).hint("Quitar").section("En el escritorio"),
                        Target::RemoveWidget(p.id),
                        p.kind.label(),
                    );
                }
                for kind in WKind::ALL {
                    for &size in kind.sizes() {
                        let (glyph, rgb) = kind.glyph();
                        add(glyph_row(kind.label(), widget_hint(kind, size), glyph, rgb).hint("Añadir").section("Añadir"), Target::AddWidget(kind, size), &format!("{} {}", kind.label(), size.label()));
                    }
                }
                let city = crate::widgets::city();
                add(glyph_row("Ciudad del tiempo", if city.is_empty() { "Sin configurar" } else { &city }, '\u{E81D}', 0x0A84FF).hint("Cambiar").section("Datos"), Target::Go(Page::WidgetCity), "tiempo clima ciudad");
                let note = crate::widgets::note();
                add(glyph_row("Editar la nota", if note.trim().is_empty() { "Vacía" } else { note.lines().next().unwrap_or("") }, '\u{E70B}', 0xFFD60A).hint("Editar").section("Datos"), Target::Go(Page::WidgetNote), "nota texto");
                add(glyph_row("Restablecer las posiciones", "Vuelve a colocarlos en cascada", '\u{E7A7}', 0xFF9F0A).section("Datos"), Target::ResetWidgets, "posicion reset");
            }
            Page::WidgetCity => {
                if !q.is_empty() {
                    add(glyph_row(&format!("Usar «{q}»"), "Se busca en Open-Meteo (solo se envía el nombre)", '\u{E81D}', 0x30D158).hint("Aplicar"), Target::SetCity(q.clone()), &q);
                }
                if !crate::widgets::city().is_empty() {
                    add(glyph_row("Quitar la ciudad", "", '\u{E74D}', 0xFF453A), Target::SetCity(String::new()), "borrar");
                }
            }
            Page::WidgetNote => {
                if !q.is_empty() {
                    add(glyph_row(&format!("Guardar «{q}»"), "Sustituye la nota", '\u{E74E}', 0x30D158).hint("Guardar"), Target::SetNote(q.clone()), &q);
                    add(glyph_row("Añadir como línea nueva", "Se suma al final de la nota", '\u{E710}', 0x0A84FF).hint("Añadir"), Target::AppendNote(q.clone()), &q);
                }
                if !crate::widgets::note().trim().is_empty() {
                    add(glyph_row("Borrar la nota", crate::widgets::note().lines().next().unwrap_or(""), '\u{E74D}', 0xFF453A), Target::ClearNote, "vaciar");
                }
            }
            Page::Login => {
                let current = wallpaper::current_lock_image()
                    .map(|p| std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(p))
                    .unwrap_or_else(|| "Windows Spotlight u otra".to_string());
                add(glyph_row("Imagen de la pantalla de bloqueo", &current, '\u{E91B}', 0x8E8E93).section("Ahora"), Target::Noop, "");
                let clear = match crate::login::clear_background() {
                    Some(true) => "Activado",
                    _ => "Desactivado",
                };
                add(glyph_row("Usar el fondo actual del escritorio", "También en el inicio de sesión, que usa esta imagen", '\u{E7F4}', 0x0A84FF).section("Imagen"), Target::LockFromWallpaper, "bloqueo imagen");
                add(glyph_row("Generar una imagen con el tema", "Aurora con los colores del tema activo", '\u{E790}', 0xFF375F).section("Imagen"), Target::LockFromTheme, "bloqueo tema");
                if wallpaper::has_original_lock() {
                    add(glyph_row("Restaurar la pantalla de bloqueo anterior", "Como Imagen, no Spotlight", '\u{E7A7}', 0xFF9F0A).section("Imagen"), Target::WpRestoreLock, "deshacer");
                }
                add(
                    glyph_row("Fondo nítido en el inicio de sesión", "Quita el desenfoque (directiva de Windows; pide administrador)", '\u{E7B3}', 0xBF5AF2).hint(clear).section("Directiva"),
                    Target::ToggleClearLogon,
                    "desenfoque acrilico nitido",
                );
                add(glyph_row("Ajustes de la pantalla de bloqueo", "Spotlight, estado rápido, mostrar en el inicio de sesión…", '\u{E713}', 0x8E8E93).hint("Abrir").section("Windows"), Target::OpenUri("ms-settings:lockscreen"), "spotlight");
                add(glyph_row("Foto de la cuenta", "Tu imagen de usuario en el inicio de sesión", '\u{E77B}', 0x8E8E93).hint("Abrir").section("Windows"), Target::OpenUri("ms-settings:yourinfo"), "avatar usuario");
                add(glyph_row("El diseño lo dibuja Windows", "La ventana de contraseña y los botones no se pueden sustituir con APIs seguras", '\u{E946}', 0x8E8E93).section("Límites"), Target::Noop, "");
            }
            Page::WpFit => {
                let current = Fit::parse(&crate::app::wallpaper_cfg().fit).unwrap_or(Fit::Fill);
                for f in Fit::ALL {
                    add(glyph_row(f.label(), "", '\u{E740}', 0x8E8E93).hint(if f == current { "Actual" } else { "Aplicar" }), Target::WpSetFit(f), f.key());
                }
            }
            Page::WpInterval => {
                let current = crate::app::wallpaper_cfg().interval_minutes;
                for n in [0u32, 5, 10, 15, 30, 60, 120, 240] {
                    let title = if n == 0 { "Desactivado".to_string() } else if n < 60 { format!("Cada {n} minutos") } else { format!("Cada {} h", n / 60) };
                    add(glyph_row(&title, "", '\u{E916}', 0x8E8E93).hint(if n == current { "Actual" } else { "Aplicar" }), Target::WpSetInterval(n), "");
                }
            }
            Page::WpFps => {
                let current = crate::app::wallpaper_cfg().animated_fps;
                for n in [10u32, 15, 24, 30, 45, 60] {
                    add(glyph_row(&format!("{n} fps"), if n <= 15 { "Menos consumo" } else if n >= 45 { "Más fluido, más consumo" } else { "" }, '\u{E916}', 0x8E8E93).hint(if n == current { "Actual" } else { "Aplicar" }), Target::WpSetFps(n), "");
                }
            }
            Page::WpFolder => {
                let cfg = crate::app::wallpaper_cfg();
                if std::path::Path::new(&q).is_dir() {
                    add(glyph_row(&format!("Usar «{q}»"), "Carpeta con tus imágenes", '\u{E8B7}', 0x30D158).hint("Aplicar"), Target::WpSetFolder(q.clone()), &q);
                }
                if !cfg.dir.is_empty() {
                    add(glyph_row("Volver a la carpeta predeterminada", "Imágenes\\Wallpapers", '\u{E7A7}', 0xFF9F0A), Target::WpSetFolder(String::new()), "predeterminada");
                }
            }
            Page::ThemeDelete => {
                for u in theme::user_themes() {
                    add(Row::new(u.name.clone()).hint("Eliminar").icon(RowIcon::Swatch(color(&u.background), color(&u.accent), color(&u.foreground))), Target::DeleteTheme(u.name.clone()), "");
                }
            }
            Page::Confirm(sys) => {
                let verb = match sys {
                    Sys::Sleep => "suspender",
                    Sys::SignOut => "cerrar sesión",
                    Sys::Restart => "reiniciar",
                    Sys::Shutdown => "apagar",
                };
                add(glyph_row(&format!("Sí, {verb}"), "", '\u{E73E}', 0xFF453A), Target::Do(sys), "");
                add(glyph_row("Cancelar", "", '\u{E711}', 0x8E8E93), Target::Go(Page::Root), "");
            }
        }

        let filtered = if matches!(self.page, Page::WebName | Page::WebUrl(_) | Page::Capture(_) | Page::ThemeValue(_) | Page::ThemeName | Page::WpFolder | Page::SettingValue(_) | Page::WidgetCity | Page::WidgetNote) || (self.page == Page::Install && !q.is_empty()) {
            items.into_iter().map(|(r, t, _)| (r, t)).collect()
        } else {
            filter(&q, items)
        };
        self.targets = filtered.iter().map(|(_, t)| t.clone()).collect();
        filtered.into_iter().map(|(r, _)| r).collect()
    }

    fn activate(&mut self, index: usize, _query: &str) -> Outcome {
        let Some(target) = self.targets.get(index).cloned() else { return Outcome::Stay };
        match target {
            Target::Go(Page::Root) => {
                // «Cancelar» de una confirmación: volver al nivel anterior
                self.page = self.stack.pop().unwrap_or(Page::Root);
                self.notice.clear();
                Outcome::Refresh
            }
            Target::Go(p) => {
                self.go(p);
                Outcome::Refresh
            }
            Target::OpenApps => Outcome::Switch(Box::new(AppsHandler::default())),
            Target::OpenPanel => {
                crate::panel::toggle(self.mon, crate::bar::height_px());
                Outcome::Close
            }
            Target::Pkg(id, name) => {
                installer::start(Job { manager: Manager::Winget, kind: Kind::Install, id: id.to_string(), name: name.to_string() });
                Outcome::Close
            }
            Target::PkgFound(mgr, id) => {
                installer::start(Job { manager: mgr, kind: Kind::Install, name: id.clone(), id });
                Outcome::Close
            }
            Target::PkgChoco(id, name) => {
                installer::start(Job { manager: Manager::Choco, kind: Kind::Install, id: id.to_string(), name: name.to_string() });
                Outcome::Close
            }
            Target::ChocoUpgrade => {
                installer::start(Job { manager: Manager::Choco, kind: Kind::UpgradeAll, id: String::new(), name: "Aplicaciones".into() });
                Outcome::Close
            }
            Target::WebLaunch(path) => {
                apps::open_uri(&path.to_string_lossy());
                Outcome::Close
            }
            Target::WebPreset(name, url) => {
                self.notice = match webapp::create(name, url) {
                    Ok(_) => format!("«{name}» creada: ya está en el lanzador y el dock"),
                    Err(e) => format!("No se pudo crear «{name}»: {e}"),
                };
                Outcome::Refresh
            }
            Target::WebNext(name) => {
                self.go(Page::WebUrl(name));
                Outcome::Refresh
            }
            Target::WebCreate(name, url) => match webapp::create(&name, &url) {
                Ok(_) => {
                    self.stack = vec![Page::Root];
                    self.page = Page::WebApps;
                    self.notice = format!("«{name}» creada: ya está en el lanzador y el dock");
                    Outcome::Refresh
                }
                Err(e) => {
                    self.notice = format!("No se pudo crear: {e}");
                    Outcome::Rerun
                }
            },
            Target::WebDelete(i) => {
                if let Some(a) = self.webapps.get(i) {
                    webapp::remove(a);
                }
                self.page = self.stack.pop().unwrap_or(Page::WebApps);
                Outcome::Refresh
            }
            Target::Shortcut(i) => {
                self.go(Page::Capture(i));
                Outcome::Refresh
            }
            Target::OpenConfig => {
                if let Some(p) = crate::config::path() {
                    apps::open_uri(&p.to_string_lossy());
                }
                Outcome::Close
            }
            Target::OpenIconsFolder => {
                if let Some(dir) = crate::icons::custom_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    apps::open_uri(&dir.to_string_lossy());
                }
                Outcome::Close
            }
            Target::ReloadIcons => Outcome::Run(Box::new(|| {
                crate::dock::reload_icons();
                toast_ok("Iconos recargados", "Los del dock se han vuelto a leer");
            })),
            Target::Lock => {
                packages::run_hidden("rundll32.exe", "user32.dll,LockWorkStation");
                Outcome::Close
            }
            Target::Confirm(s) => {
                self.go(Page::Confirm(s));
                Outcome::Refresh
            }
            Target::Do(s) => {
                match s {
                    Sys::Sleep => packages::run_hidden("rundll32.exe", "powrprof.dll,SetSuspendState 0,1,0"),
                    Sys::SignOut => packages::run_hidden("shutdown.exe", "/l"),
                    Sys::Restart => packages::run_hidden("shutdown.exe", "/r /t 0"),
                    Sys::Shutdown => packages::run_hidden("shutdown.exe", "/s /t 0"),
                }
                Outcome::Close
            }
            Target::UpdateAll => {
                installer::start(Job { manager: Manager::Winget, kind: Kind::UpgradeAll, id: String::new(), name: "Aplicaciones".into() });
                Outcome::Close
            }
            Target::ToggleAutostart => Outcome::Run(Box::new(|| {
                let on = !crate::autostart::is_enabled();
                match if on { crate::autostart::enable() } else { crate::autostart::disable() } {
                    Ok(()) => toast_ok(if on { "Inicio con Windows activado" } else { "Inicio con Windows desactivado" }, if on { "hypr-win arrancará al iniciar sesión" } else { "Ya no arranca solo" }),
                    Err(e) => crate::toast::upsert(crate::toast::next_id(), '\u{E783}', crate::toast::RED, "No se pudo cambiar el inicio", &e, Some(std::time::Duration::from_secs(8))),
                }
            })),
            Target::UpdateCheck => Outcome::Run(Box::new(|| crate::updater::check_async(true))),
            Target::UpdateInstall => Outcome::Run(Box::new(crate::updater::install_async)),
            Target::Quit => {
                crate::app::request_quit();
                Outcome::Close
            }
            Target::SetBarStyle(style) => {
                self.notice.clear();
                Outcome::Run(Box::new(move || {
                    if crate::app::apply_bar_style(&style).is_ok() {
                        let _ = crate::config::save_value("bar", "style", &lit_str(&style));
                    }
                }))
            }
            Target::ApplyTheme(name) => {
                self.notice.clear();
                Outcome::Run(Box::new(move || {
                    if let Some(def) = theme::find(&name) {
                        let _ = crate::app::apply_theme(&def);
                        let _ = crate::config::save_value("bar", "theme", &lit_str(&def.name));
                        follow_theme_wallpaper(&def);
                        // un tema puede traer su propio fondo animado (p. ej. «Bushidō»); los demás no tocan el actual
                        if let Some(a) = def.animated.as_deref().filter(|a| !a.trim().is_empty()) {
                            let r = crate::app::set_animated(a).map(|_| {
                                let _ = crate::config::save_value("wallpaper", "animated", &lit_str(a));
                                format!("Fondo animado: {}", crate::livewall::Source::parse(a).map(|s| s.label()).unwrap_or_default())
                            });
                            notify(r);
                        }
                    }
                }))
            }
            Target::AddWidget(kind, size) => Outcome::Run(Box::new(move || match crate::widgets::add(kind, size) {
                Ok(_) => toast_ok(&format!("Widget «{}» añadido", kind.label()), "Arrástralo donde quieras"),
                Err(e) => notify(Err(e)),
            })),
            Target::RemoveWidget(id) => Outcome::Run(Box::new(move || crate::widgets::remove(id))),
            Target::ToggleWidgets => Outcome::Run(Box::new(|| crate::widgets::set_visible(!crate::widgets::visible()))),
            Target::ResetWidgets => Outcome::Run(Box::new(crate::widgets::reset_positions)),
            Target::SetCity(city) => {
                self.page = self.stack.pop().unwrap_or(Page::Widgets);
                Outcome::Run(Box::new(move || crate::widgets::set_city(&city)))
            }
            Target::SetNote(text) => {
                self.page = self.stack.pop().unwrap_or(Page::Widgets);
                Outcome::Run(Box::new(move || crate::widgets::set_note(&text)))
            }
            Target::AppendNote(line) => {
                self.page = self.stack.pop().unwrap_or(Page::Widgets);
                Outcome::Run(Box::new(move || {
                    let old = crate::widgets::note();
                    let text = if old.trim().is_empty() { line } else { format!("{}\n{line}", old.trim_end()) };
                    crate::widgets::set_note(&text);
                }))
            }
            Target::ClearNote => {
                self.page = self.stack.pop().unwrap_or(Page::Widgets);
                Outcome::Run(Box::new(|| crate::widgets::set_note("")))
            }
            Target::LockFromWallpaper => Outcome::Run(Box::new(|| {
                let first = wallpaper::current().into_iter().find(|p| std::path::Path::new(p).is_file());
                match first {
                    Some(p) => notify(wallpaper::set_lock_screen(&p).map(|_| "Pantalla de bloqueo actualizada".to_string())),
                    None => notify(Err("no se pudo leer el fondo actual".into())),
                }
            })),
            Target::LockFromTheme => Outcome::Run(Box::new(|| {
                let bar = crate::app::current_bar().unwrap_or_default();
                let name = if bar.theme.is_empty() { "Tema".to_string() } else { bar.theme.clone() };
                let r = wallpaper::generate(WpKind::Aurora, &name, color(&bar.background), color(&bar.accent)).and_then(|p| wallpaper::set_lock_screen(&p.to_string_lossy()));
                notify(r.map(|_| "Pantalla de bloqueo actualizada".to_string()));
            })),
            Target::ToggleClearLogon => Outcome::Run(Box::new(|| {
                let now = crate::login::clear_background().unwrap_or(false);
                let id = crate::toast::next_id();
                crate::toast::upsert(id, '\u{E7B3}', crate::toast::BLUE, "Cambiando la directiva de Windows", "Confirma el aviso de administrador", None);
                match crate::login::set_clear_background(!now) {
                    installer::Outcome::Done | installer::Outcome::UpToDate => {
                        crate::toast::upsert(id, '\u{E73E}', crate::toast::GREEN, if now { "Desenfoque restaurado" } else { "Fondo nítido en el inicio de sesión" }, "Se verá la próxima vez que bloquees", Some(std::time::Duration::from_secs(6)))
                    }
                    installer::Outcome::Cancelled => crate::toast::upsert(id, '\u{E711}', crate::toast::RED, "Cancelado", "No se concedieron permisos de administrador", Some(std::time::Duration::from_secs(5))),
                    installer::Outcome::Failed(e) => crate::toast::upsert(id, '\u{E783}', crate::toast::RED, "No se pudo cambiar la directiva", &e, Some(std::time::Duration::from_secs(8))),
                }
            })),
            Target::OpenUri(uri) => {
                apps::open_uri(uri);
                Outcome::Close
            }
            Target::Setting(i) => {
                let st = &settings::ALL[i];
                if matches!(st.kind, SettingKind::Bool) {
                    // los interruptores se cambian al momento
                    let now = settings::bool_value(&crate::app::config_snapshot(), st);
                    return Outcome::Run(Box::new(move || apply_setting(i, if now { "false" } else { "true" }.to_string())));
                }
                self.go(Page::SettingValue(i));
                Outcome::Refresh
            }
            Target::SetSetting(i, literal) => {
                self.page = self.stack.pop().unwrap_or(Page::Settings);
                Outcome::Run(Box::new(move || apply_setting(i, literal)))
            }
            Target::WpNextMonitor => {
                let n = wallpaper::connected_monitors().len();
                self.wp_target = match self.wp_target {
                    None if n > 0 => Some(0),
                    Some(i) if i + 1 < n => Some(i + 1),
                    _ => None,
                };
                Outcome::Rerun
            }
            Target::WpApply(path) => {
                let target = self.wp_target;
                Outcome::Run(Box::new(move || notify(apply_wallpaper(&path, target).map(|s| format!("Fondo aplicado{s}")))))
            }
            Target::WpGenerate(kind) => {
                let target = self.wp_target;
                Outcome::Run(Box::new(move || {
                    let bar = crate::app::current_bar().unwrap_or_default();
                    let name = if bar.theme.is_empty() { "Tema".to_string() } else { bar.theme.clone() };
                    let r = wallpaper::generate(kind, &name, color(&bar.background), color(&bar.accent))
                        .and_then(|p| apply_wallpaper(&p.to_string_lossy(), target));
                    notify(r.map(|s| format!("Fondo «{}» aplicado{s}", kind.label())));
                }))
            }
            Target::WpRandom => {
                let target = self.wp_target;
                Outcome::Run(Box::new(move || {
                    let images = wallpaper::images(&crate::app::wallpaper_cfg().dir, 400);
                    if images.is_empty() {
                        notify(Err("no hay imágenes en tus carpetas".into()));
                        return;
                    }
                    let pick = &images[wallpaper::random_index(images.len())];
                    notify(apply_wallpaper(&pick.path.to_string_lossy(), target).map(|s| format!("Fondo: {}{s}", pick.name)));
                }))
            }
            Target::WpSetFit(f) => {
                self.page = self.stack.pop().unwrap_or(Page::Wallpaper);
                Outcome::Run(Box::new(move || {
                    let mut w = crate::app::wallpaper_cfg();
                    w.fit = f.key().to_string();
                    let _ = crate::config::save_value("wallpaper", "fit", &lit_str(f.key()));
                    crate::app::set_wallpaper_cfg(w);
                    notify(wallpaper::set_fit(f).map(|_| format!("Ajuste: {}", f.label())));
                }))
            }
            Target::WpAnimate(key) => Outcome::Run(Box::new(move || {
                let label = crate::livewall::Source::parse(&key).map(|s| s.label()).unwrap_or_default();
                let r = crate::app::set_animated(&key).map(|_| {
                    let _ = crate::config::save_value("wallpaper", "animated", &lit_str(&key));
                    format!("Fondo animado: {label}")
                });
                notify(r);
            })),
            Target::WpAnimateStop => Outcome::Run(Box::new(|| {
                let r = crate::app::set_animated("").map(|_| {
                    let _ = crate::config::save_value("wallpaper", "animated", &lit_str(""));
                    "Fondo animado detenido".to_string()
                });
                notify(r);
            })),
            Target::WpSetFps(n) => {
                self.page = self.stack.pop().unwrap_or(Page::Wallpaper);
                Outcome::Run(Box::new(move || {
                    let _ = crate::config::save_value("wallpaper", "animated_fps", &n.to_string());
                    crate::app::set_animated_fps(n);
                }))
            }
            Target::WpSetInterval(n) => {
                self.page = self.stack.pop().unwrap_or(Page::Wallpaper);
                Outcome::Run(Box::new(move || {
                    let mut w = crate::app::wallpaper_cfg();
                    w.interval_minutes = n;
                    let _ = crate::config::save_value("wallpaper", "interval_minutes", &n.to_string());
                    crate::app::set_wallpaper_cfg(w);
                }))
            }
            Target::WpSetFolder(dir) => {
                self.page = self.stack.pop().unwrap_or(Page::Wallpaper);
                Outcome::Run(Box::new(move || {
                    let mut w = crate::app::wallpaper_cfg();
                    w.dir = dir.clone();
                    let _ = crate::config::save_value("wallpaper", "dir", &lit_str(&dir));
                    crate::app::set_wallpaper_cfg(w);
                }))
            }
            Target::WpToggleFollow => Outcome::Run(Box::new(|| {
                let mut w = crate::app::wallpaper_cfg();
                w.follow_theme = !w.follow_theme;
                let _ = crate::config::save_value("wallpaper", "follow_theme", if w.follow_theme { "true" } else { "false" });
                crate::app::set_wallpaper_cfg(w);
            })),
            Target::WpToggleLock => Outcome::Run(Box::new(|| {
                let mut w = crate::app::wallpaper_cfg();
                w.lock_screen = !w.lock_screen;
                let _ = crate::config::save_value("wallpaper", "lock_screen", if w.lock_screen { "true" } else { "false" });
                crate::app::set_wallpaper_cfg(w);
            })),
            Target::WpRestoreLock => Outcome::Run(Box::new(|| notify(wallpaper::restore_lock_screen().map(|_| "Pantalla de bloqueo restaurada".to_string())))),
            Target::WpRestore => Outcome::Run(Box::new(|| notify(wallpaper::restore_original().map(|_| "Fondo original restaurado".to_string())))),
            Target::EditProp(p) => {
                self.go(Page::ThemeValue(p));
                Outcome::Refresh
            }
            Target::SetProp(p, v) => {
                let back = self.stack.pop().unwrap_or(Page::ThemeEdit);
                self.page = back;
                self.notice = format!("{} cambiado", p.label());
                Outcome::Run(Box::new(move || {
                    let _ = edit_current_theme(p, &v);
                }))
            }
            Target::NewTheme(name) => {
                let name = theme::file_name(&name);
                self.stack = vec![Page::Root, Page::Themes];
                self.page = Page::ThemeEdit;
                self.notice = format!("Tema «{name}» creado: cambia lo que quieras");
                Outcome::Run(Box::new(move || {
                    let bar = crate::app::current_bar().unwrap_or_default();
                    let mut def = theme::from_bar(&name, &bar);
                    def.name = name.clone();
                    if theme::save_user(&def).is_ok() {
                        let _ = crate::app::apply_theme(&def);
                        let _ = crate::config::save_value("bar", "theme", &lit_str(&def.name));
                        follow_theme_wallpaper(&def);
                    }
                }))
            }
            Target::DeleteTheme(name) => {
                self.page = self.stack.pop().unwrap_or(Page::Themes);
                Outcome::Run(Box::new(move || {
                    let active = crate::app::current_bar().map(|b| b.theme).unwrap_or_default();
                    theme::delete_user(&name);
                    if active.eq_ignore_ascii_case(&name) {
                        if let Some(def) = theme::builtins().into_iter().next() {
                            let _ = crate::app::apply_theme(&def);
                            let _ = crate::config::save_value("bar", "theme", &lit_str(&def.name));
                        }
                    }
                }))
            }
            Target::Noop => Outcome::Stay,
        }
    }

    fn back(&mut self) -> bool {
        match self.stack.pop() {
            Some(p) => {
                self.page = p;
                self.notice.clear();
                true
            }
            None => false,
        }
    }

    fn capturing(&self) -> bool {
        matches!(self.page, Page::Capture(_))
    }

    fn capture(&mut self, vk: u32, mods: u32) -> Outcome {
        let Page::Capture(index) = self.page else { return Outcome::Stay };
        let plain = mods & !MOD_NOREPEAT;
        if vk == 0x1B && plain == 0 {
            self.back();
            return Outcome::Refresh;
        }
        let Some(keys) = keybinds::format_keys(plain, vk) else {
            self.notice = "Esa tecla no se puede usar en un atajo".into();
            return Outcome::Rerun;
        };
        if plain & (MOD_WIN | MOD_CONTROL | MOD_ALT) == 0 {
            self.notice = "Incluye Super, Ctrl o Alt en la combinación".into();
            return Outcome::Rerun;
        }
        let all = crate::app::bindings_raw();
        if let Some((j, (_, other))) = all.iter().enumerate().find(|(j, (_, b))| *j != index && b.vk == vk && b.mods & !MOD_NOREPEAT == plain) {
            let _ = j;
            self.notice = format!("{keys} ya lo usa «{}»", keybinds::describe(&other.action));
            return Outcome::Rerun;
        }
        match crate::app::change_binding(index, &keys) {
            Ok(()) => {
                self.back();
                self.notice = format!("Atajo cambiado a {keys}");
            }
            Err(e) => self.notice = e,
        }
        Outcome::Refresh
    }
}

/// Configuración de la barra aplicada ahora (o la de por defecto si la aplicación está ocupada).
fn app_bar() -> crate::config::Bar {
    crate::app::current_bar().unwrap_or_default()
}

/// "#rrggbb" -> COLORREF (negro si no es válido).
fn color(hex: &str) -> u32 {
    crate::config::parse_color(hex).unwrap_or(0)
}

/// Nombre libre para una copia del tema `base` ("Mi Nord", "Mi Nord 2"...).
fn unique_user_name(base: &str) -> String {
    let taken = |n: &str| theme::user_themes().iter().any(|u| u.name.eq_ignore_ascii_case(n));
    let first = format!("Mi {base}");
    if !taken(&first) {
        return first;
    }
    (2..).map(|i| format!("{first} {i}")).find(|n| !taken(n)).unwrap_or(first)
}

/// Cambia una propiedad del tema activo. Si el activo es uno predefinido, se hace una copia propia para no
/// modificarlo. Se guarda, se aplica en vivo y queda como tema activo en `config.toml`.
fn edit_current_theme(prop: Prop, value: &str) -> Result<String, String> {
    let bar = crate::app::current_bar().ok_or("la aplicación está ocupada")?;
    let active = bar.theme.clone();
    let mut def: ThemeDef = theme::user_themes()
        .into_iter()
        .find(|u| u.name.eq_ignore_ascii_case(&active))
        .unwrap_or_else(|| {
            let mut d = theme::from_bar(&active, &bar);
            d.name = unique_user_name(if active.is_empty() { "tema" } else { &active });
            d
        });
    match prop {
        Prop::Background => def.background = value.into(),
        Prop::Foreground => def.foreground = value.into(),
        Prop::Dim => def.dim = value.into(),
        Prop::Accent => {
            def.accent = value.into();
            def.accent_text = theme::readable_on(value); // el texto sobre el acento se ajusta solo
        }
        Prop::AccentText => def.accent_text = value.into(),
        Prop::Font => def.font = Some(value.into()),
        Prop::Icons => def.icons = Some(value.into()),
        Prop::AppIcons => def.app_icons = Some(value.into()),
        Prop::Opacity => def.opacity = Some(value.parse().map_err(|_| "opacidad no válida".to_string())?),
    }
    theme::save_user(&def)?;
    crate::app::apply_theme(&def)?;
    crate::config::save_value("bar", "theme", &lit_str(&def.name))?;
    follow_theme_wallpaper(&def);
    Ok(def.name)
}

/// Aplica una imagen como fondo (respetando el ajuste configurado) y, si está activado, también como imagen de
/// la pantalla de bloqueo (Win+L). Devuelve el texto que se añade al aviso ("" o " y pantalla de bloqueo").
fn apply_wallpaper(path: &str, target: Option<usize>) -> Result<String, String> {
    let cfg = crate::app::wallpaper_cfg();
    if let Some(fit) = Fit::parse(&cfg.fit) {
        let _ = wallpaper::set_fit(fit);
    }
    wallpaper::set(path, target)?;
    // la pantalla de bloqueo es una sola: se cambia al aplicar a todos los monitores o al primero
    if cfg.lock_screen && matches!(target, None | Some(0)) {
        return Ok(match wallpaper::set_lock_screen(path) {
            Ok(()) => " y pantalla de bloqueo".to_string(),
            Err(e) => format!(" (la pantalla de bloqueo no: {e})"),
        });
    }
    Ok(String::new())
}

/// Si está activado "cambiar el fondo con el tema", genera y aplica el fondo del tema.
fn follow_theme_wallpaper(def: &ThemeDef) {
    if !crate::app::wallpaper_cfg().follow_theme {
        return;
    }
    let kind = def.wallpaper.as_deref().and_then(WpKind::parse).unwrap_or(WpKind::Gradient);
    let r = wallpaper::generate(kind, &def.name, color(&def.background), color(&def.accent)).and_then(|p| apply_wallpaper(&p.to_string_lossy(), None));
    if let Err(e) = r {
        notify(Err(e));
    }
}

/// Avisa del resultado con una notificación flotante.
fn notify(result: Result<String, String>) {
    let id = crate::toast::next_id();
    let ttl = Some(std::time::Duration::from_secs(4));
    match result {
        Ok(msg) => crate::toast::upsert(id, '\u{E73E}', crate::toast::GREEN, &msg, "", ttl),
        Err(e) => crate::toast::upsert(id, '\u{E783}', crate::toast::RED, "No se pudo cambiar el fondo", &e, Some(std::time::Duration::from_secs(9))),
    }
}

/// Guarda un ajuste en `config.toml` y lo aplica en vivo; avisa del resultado.
fn apply_setting(index: usize, literal: String) {
    let st = &settings::ALL[index];
    let id = crate::toast::next_id();
    let result = crate::config::save_value(st.section, st.key, &literal).and_then(|_| crate::app::reload_config());
    match result {
        Ok(restart) if restart.is_empty() => {
            let shown = settings::get(&crate::app::config_snapshot(), st);
            crate::toast::upsert(id, '\u{E73E}', crate::toast::GREEN, st.label, &shown, Some(std::time::Duration::from_secs(3)));
        }
        Ok(restart) => crate::toast::upsert(
            id,
            '\u{E783}',
            crate::toast::BLUE,
            "Guardado: reinicia hypr-win para aplicarlo",
            &restart.join(", "),
            Some(std::time::Duration::from_secs(8)),
        ),
        Err(e) => crate::toast::upsert(id, '\u{E783}', crate::toast::RED, &format!("No se pudo cambiar «{}»", st.label), &e, Some(std::time::Duration::from_secs(10))),
    }
}

/// Nombre legible del estilo de iconos de aplicación (`""` = originales).
fn app_style_label(key: &str) -> &'static str {
    crate::icons::APP_STYLES.iter().find(|(k, _, _)| k.eq_ignore_ascii_case(key)).map_or("Originales", |(_, l, _)| l)
}

/// Qué enseña cada tamaño de widget en el menú.
fn widget_hint(kind: WKind, size: WSize) -> &'static str {
    match (kind, size) {
        (WKind::Clock, WSize::Small) => "Pequeño · reloj analógico",
        (WKind::Clock, _) => "Mediano · hora digital y fecha",
        (WKind::Calendar, WSize::Small) => "Pequeño · el día de hoy",
        (WKind::Calendar, _) => "Grande · el mes entero",
        (WKind::System, _) => "Mediano · CPU, memoria, disco y red",
        (WKind::Weather, WSize::Small) => "Pequeño · temperatura actual",
        (WKind::Weather, _) => "Mediano · y los 3 días siguientes",
        (WKind::Media, _) => "Mediano · lo que suena, con controles",
        (WKind::Notes, WSize::Medium) => "Mediano · tu nota",
        (WKind::Notes, _) => "Grande · tu nota",
        (WKind::Shortcuts, _) => "Grande · los atajos de teclado",
    }
}

fn toast_ok(title: &str, sub: &str) {
    crate::toast::upsert(crate::toast::next_id(), '\u{E73E}', crate::toast::GREEN, title, sub, Some(std::time::Duration::from_secs(4)));
}
