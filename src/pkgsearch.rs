//! Búsqueda de paquetes en vivo con `winget search` y `choco search`, en segundo plano.
//!
//! `lookup` devuelve al instante lo que haya en caché o "pendiente"; la primera vez lanza la búsqueda en
//! otro hilo (con una pequeña espera para no buscar en cada tecla) y avisa a la paleta al terminar.

use std::collections::HashMap;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::packages::{self, Manager};
use crate::search;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Espera sin teclear antes de lanzar la búsqueda.
const DEBOUNCE: Duration = Duration::from_millis(350);
const TIMEOUT: Duration = Duration::from_secs(25);
/// Máximo de resultados por gestor.
pub const MAX_RESULTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub id: String,
    pub version: String,
}

#[derive(Clone, Debug)]
pub enum State {
    Pending,
    Done(Vec<Found>),
    Failed(String),
}

type Key = (Manager, String);

static CACHE: Mutex<Option<HashMap<Key, State>>> = Mutex::new(None);
/// Última consulta pedida: una búsqueda en espera se descarta si ya no es la actual.
static LATEST: Mutex<String> = Mutex::new(String::new());

fn with_cache<R>(f: impl FnOnce(&mut HashMap<Key, State>) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// Estado de la búsqueda de `query` en `manager`; la inicia en segundo plano si hace falta.
pub fn lookup(manager: Manager, query: &str) -> State {
    let q = query.trim().to_string();
    *LATEST.lock().unwrap_or_else(|e| e.into_inner()) = q.clone();
    let key = (manager, q.clone());
    let existing = with_cache(|c| {
        if c.len() > 60 {
            c.retain(|_, s| matches!(s, State::Pending));
        }
        let found = c.get(&key).cloned();
        if found.is_none() {
            c.insert(key.clone(), State::Pending);
        }
        found
    });
    if let Some(s) = existing {
        return s;
    }
    let _ = std::thread::Builder::new().name("hypr-win-pkgsearch".into()).spawn(move || run_search(manager, q));
    State::Pending
}

fn run_search(manager: Manager, q: String) {
    std::thread::sleep(DEBOUNCE);
    // el usuario siguió escribiendo: esta consulta ya no interesa (se pedirá de nuevo si vuelve a ella)
    if *LATEST.lock().unwrap_or_else(|e| e.into_inner()) != q {
        with_cache(|c| c.remove(&(manager, q.clone())));
        return;
    }
    let state = match search_now(manager, &q) {
        Some(v) => State::Done(v),
        None => State::Failed("no se pudo ejecutar la búsqueda".into()),
    };
    with_cache(|c| c.insert((manager, q), state));
    crate::palette::refresh_rows_async();
}

/// Ejecuta la búsqueda y devuelve los resultados ya ordenados (bloquea).
pub fn search_now(manager: Manager, query: &str) -> Option<Vec<Found>> {
    let q = packages::sanitize_arg(query);
    if q.is_empty() {
        return Some(Vec::new());
    }
    let mut found = match manager {
        Manager::Winget => {
            let exe = packages::winget_path()?;
            parse_winget(&run_capture(&exe, &["search", &q, "--accept-source-agreements", "--disable-interactivity"])?)
        }
        Manager::Choco => {
            let exe = packages::choco_path()?;
            // coincidencia exacta primero (el listado general no pone el paquete que se llama igual arriba)
            let mut v = parse_choco(&run_capture(&exe, &["search", &q, "-r", "--exact"]).unwrap_or_default());
            for f in parse_choco(&run_capture(&exe, &["search", &q, "-r", "--page", "0", "--page-size", "40"])?) {
                if !v.iter().any(|x| x.id == f.id) {
                    v.push(f);
                }
            }
            v
        }
    };
    if manager == Manager::Choco {
        rank_by_query(&mut found, &q); // Chocolatey no ordena por relevancia
    }
    found.truncate(MAX_RESULTS);
    Some(found)
}

/// Ordena por coincidencia del id/nombre con la consulta (estable: empates en el orden original).
pub fn rank_by_query(found: &mut [Found], query: &str) {
    found.sort_by_key(|f| std::cmp::Reverse(search::score(query, &f.name).max(search::score(query, &f.id)).unwrap_or(-1)));
}

fn run_capture(exe: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    // se lee en otro hilo para que una salida larga no bloquee el proceso
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => break,
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    let bytes = reader.join().ok()?;
    Some(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
}

/// Parte una línea en columnas separadas por 2 o más espacios (los nombres llevan espacios sueltos).
fn split_columns(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut cur = String::new();
    let mut spaces = 0;
    for ch in line.chars() {
        if ch == ' ' {
            spaces += 1;
            continue;
        }
        if spaces >= 2 && !cur.is_empty() {
            cols.push(std::mem::take(&mut cur));
        } else if spaces == 1 {
            cur.push(' ');
        }
        spaces = 0;
        cur.push(ch);
    }
    if !cur.is_empty() {
        cols.push(cur);
    }
    cols
}

/// Tabla de `winget search`: tras la línea de guiones, columnas Nombre, Id, Versión, [Coincidencia], Origen.
/// Los títulos de columna dependen del idioma, por eso solo se usa la línea de guiones como referencia.
pub fn parse_winget(out: &str) -> Vec<Found> {
    let mut found = Vec::new();
    let mut after_header = false;
    for line in out.lines().map(|l| l.trim_end_matches('\r')) {
        if !after_header {
            after_header = line.len() >= 10 && line.chars().all(|c| c == '-');
            continue;
        }
        let cols = split_columns(line);
        if cols.len() >= 3 && !cols[1].contains(' ') {
            found.push(Found { name: cols[0].clone(), id: cols[1].clone(), version: cols[2].clone() });
        }
    }
    found
}

/// Salida `-r` de Chocolatey: `id|versión` por línea.
pub fn parse_choco(out: &str) -> Vec<Found> {
    out.lines()
        .filter_map(|l| {
            let (id, version) = l.trim().split_once('|')?;
            (!id.is_empty() && !id.contains(' ')).then(|| Found { name: id.to_string(), id: id.to_string(), version: version.to_string() })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINGET_ES: &str = "Nombre         Id                      Versión   Coincidencia   Origen\n\
------------------------------------------------------------------------\n\
Git            Git.Git                 2.56.0.3                 winget\n\
Visual Studio Code  Microsoft.VisualStudioCode  1.99.0           winget\n\
OpenCherry Desktop  4nrry.OpenCherryDesktop  0.0.1  Tag: git     winget\n";

    #[test]
    fn parses_localized_winget_table() {
        let v = parse_winget(WINGET_ES);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], Found { name: "Git".into(), id: "Git.Git".into(), version: "2.56.0.3".into() });
        assert_eq!(v[1].name, "Visual Studio Code");
        assert_eq!(v[1].id, "Microsoft.VisualStudioCode");
        assert_eq!(v[2].id, "4nrry.OpenCherryDesktop");
    }

    #[test]
    fn winget_without_results_or_with_noise_gives_nothing() {
        assert!(parse_winget("No se encontró ningún paquete que coincida con los criterios de entrada.\n").is_empty());
        assert!(parse_winget("\r   - \r   \\ \r").is_empty());
        assert!(parse_winget("").is_empty());
    }

    #[test]
    fn parses_choco_limit_output() {
        let v = parse_choco("act-cli|0.2.92\nChocolatey v2.5.0\ngit|2.45.0\nmalformada\n|1.0\n");
        assert_eq!(v.len(), 2);
        assert_eq!(v[1], Found { name: "git".into(), id: "git".into(), version: "2.45.0".into() });
    }

    #[test]
    fn choco_results_are_ranked_by_relevance() {
        let mut v = parse_choco("act-cli|1\nargocd-autopilot|1\ngit.install|2\ngit|3\nark-vcs|1\n");
        rank_by_query(&mut v, "git");
        assert_eq!(v[0].id, "git");
        assert_eq!(v[1].id, "git.install");
    }

    #[test]
    fn column_splitting_keeps_single_spaces() {
        assert_eq!(split_columns("Mi  App Pro   x.y  z"), vec!["Mi", "App Pro", "x.y", "z"]);
        assert!(split_columns("   ").is_empty());
    }
}
