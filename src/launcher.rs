//! Lanzador de aplicaciones (Super+K): la paleta estilo Spotlight con la lista de aplicaciones del
//! sistema. Empieza como una barra de búsqueda y muestra los resultados al escribir.

use crate::apps::{self, Entry};
use crate::config::Bar;
use crate::layout::Rect;
use crate::palette::{self, Handler, Outcome, Row, RowIcon};
use crate::search;

pub(crate) use crate::palette::tile_color;

/// Clase de la ventana (para que el gestor no la tile).
pub const CLASS_NAME: &str = palette::CLASS_NAME;

pub fn create(cfg: &Bar) -> Result<(), String> {
    palette::create(cfg)
}

pub fn is_open() -> bool {
    palette::is_open()
}

/// Abre el lanzador centrado en el monitor `mon`, o lo cierra si ya estaba abierto.
pub fn toggle(mon: Rect) {
    if palette::is_open() {
        palette::close(true);
        return;
    }
    apps::start_indexing(); // refresca el índice por si se instaló algo; el actual sigue valiendo
    palette::open(Box::new(AppsHandler::default()), mon);
}

#[derive(Default)]
pub struct AppsHandler {
    results: Vec<Entry>,
    /// Mostrar todas las apps aunque no se haya escrito nada (↓ o Tab).
    browse: bool,
}

impl Handler for AppsHandler {
    fn placeholder(&self) -> String {
        "Buscar aplicaciones".into()
    }

    fn show_on_empty(&self) -> bool {
        self.browse
    }

    fn empty_message(&self) -> String {
        if apps::entries().is_empty() { "Indexando aplicaciones…".into() } else { "Sin resultados".into() }
    }

    fn rows(&mut self, query: &str) -> Vec<Row> {
        let all = apps::entries();
        let names: Vec<String> = all.iter().map(|e| e.name.clone()).collect();
        self.results = search::rank(query, &names, 200).into_iter().map(|i| all[i].clone()).collect();
        let searching = !query.trim().is_empty();
        self.results
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let section = if searching && i == 0 { "Mejor resultado" } else { "Aplicaciones" };
                Row::new(e.name.clone()).hint("Aplicación").icon(RowIcon::App(e.id.clone())).section(section)
            })
            .collect()
    }

    fn activate(&mut self, index: usize, _query: &str) -> Outcome {
        if let Some(e) = self.results.get(index) {
            apps::launch(e);
        }
        Outcome::Close
    }

    fn back(&mut self) -> bool {
        std::mem::take(&mut self.browse)
    }

    fn expand(&mut self) -> bool {
        self.browse = true;
        true
    }
}
