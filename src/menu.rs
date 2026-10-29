//! Menú del sistema (Super+Espacio), al estilo del menú de Omarchy: una paleta por niveles con
//! aplicaciones, instalación de programas (winget), aplicaciones web, atajos de teclado y sistema.

use std::path::PathBuf;

use crate::apps;
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
}

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
}

impl MenuHandler {
    pub fn new(mon: Rect) -> Self {
        Self { page: Page::Root, stack: Vec::new(), targets: Vec::new(), notice: String::new(), webapps: Vec::new(), mon }
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
                add(glyph_row("Panel de control", "Wi-Fi, Bluetooth y sonido", '\u{E713}', 0x8E8E93), Target::OpenPanel, "wifi bluetooth volumen bateria");
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
                add(glyph_row("Actualizar aplicaciones", "winget upgrade --all", '\u{E895}', 0x30D158).hint("Consola"), Target::UpdateAll, "winget actualizar");
                if packages::choco_available() {
                    add(glyph_row("Actualizar con Chocolatey", "choco upgrade all (administrador)", '\u{E895}', 0xFF9F0A).hint("Admin"), Target::ChocoUpgrade, "choco actualizar");
                }
                add(glyph_row("Salir de hypr-win", "Restaura las ventanas y la barra de tareas", '\u{E711}', 0x8E8E93), Target::Quit, "cerrar");
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

        let filtered = if matches!(self.page, Page::WebName | Page::WebUrl(_) | Page::Capture(_)) || (self.page == Page::Install && !q.is_empty()) {
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
                packages::run_in_console(&format!("Instalando {name}"), &packages::install_id_command(id));
                Outcome::Close
            }
            Target::PkgFound(Manager::Winget, id) => {
                packages::run_in_console(&format!("Instalando {id}"), &packages::install_id_command(&id));
                Outcome::Close
            }
            Target::PkgFound(Manager::Choco, id) => {
                packages::run_in_console_elevated(&packages::choco_install_command(&id));
                Outcome::Close
            }
            Target::PkgChoco(id, _name) => {
                packages::run_in_console_elevated(&packages::choco_install_command(id));
                Outcome::Close
            }
            Target::ChocoUpgrade => {
                packages::run_in_console_elevated(&packages::choco_upgrade_all_command());
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
                packages::run_in_console("Actualizando aplicaciones", &packages::upgrade_all_command());
                Outcome::Close
            }
            Target::Quit => {
                crate::app::request_quit();
                Outcome::Close
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
