//! Búsqueda real con winget y Chocolatey (solo lectura; necesita red). Se omite si falta el gestor.

use std::time::{Duration, Instant};

use hypr_win::packages::{self, Manager};
use hypr_win::pkgsearch::{self, State};

#[test]
fn winget_search_returns_parsed_results() {
    if !packages::winget_available() {
        eprintln!("sin winget: se omite");
        return;
    }
    let found = pkgsearch::search_now(Manager::Winget, "git").expect("ejecutar winget");
    println!("winget: {} resultados; primeros: {:?}", found.len(), found.iter().take(3).map(|f| &f.id).collect::<Vec<_>>());
    assert!(!found.is_empty(), "winget search git no devolvió nada (¿sin red?)");
    assert!(found.len() <= pkgsearch::MAX_RESULTS);
    assert!(found.iter().all(|f| !f.id.is_empty() && !f.id.contains(' ') && !f.version.is_empty()));
    assert!(found.iter().any(|f| f.id == "Git.Git"), "Git.Git debería aparecer");
}

#[test]
fn chocolatey_search_is_ranked_by_relevance() {
    if !packages::choco_available() {
        eprintln!("sin Chocolatey: se omite");
        return;
    }
    let found = pkgsearch::search_now(Manager::Choco, "git").expect("ejecutar choco");
    println!("choco: {} resultados; primeros: {:?}", found.len(), found.iter().take(3).map(|f| &f.id).collect::<Vec<_>>());
    assert!(!found.is_empty());
    // gracias al reordenado, el primero contiene la consulta aunque choco lo devuelva en otro orden
    assert!(found[0].id.contains("git"), "primer resultado poco relevante: {}", found[0].id);
}

#[test]
fn lookup_runs_in_background_and_caches() {
    if !packages::winget_available() {
        return;
    }
    // la primera llamada no bloquea: devuelve "pendiente"
    let t0 = Instant::now();
    assert!(matches!(pkgsearch::lookup(Manager::Winget, "vlc"), State::Pending));
    assert!(t0.elapsed() < Duration::from_millis(200), "lookup no debe bloquear");
    // y acaba entregando los resultados
    let deadline = Instant::now() + Duration::from_secs(40);
    let state = loop {
        match pkgsearch::lookup(Manager::Winget, "vlc") {
            State::Pending if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            s => break s,
        }
    };
    match state {
        State::Done(v) => println!("lookup: {} resultados en {:?}", v.len(), t0.elapsed()),
        other => panic!("estado inesperado: {other:?}"),
    }
    // la segunda vez sale de la caché al instante
    let t1 = Instant::now();
    assert!(matches!(pkgsearch::lookup(Manager::Winget, "vlc"), State::Done(_)));
    assert!(t1.elapsed() < Duration::from_millis(50));
}

#[test]
fn stale_queries_are_discarded() {
    if !packages::winget_available() {
        return;
    }
    // el usuario escribe "zz" y enseguida otra cosa: la primera búsqueda nunca llega a ejecutarse
    let _ = pkgsearch::lookup(Manager::Winget, "zzqq");
    let _ = pkgsearch::lookup(Manager::Winget, "zzqqx");
    std::thread::sleep(Duration::from_millis(900));
    // al volver a pedir la antigua se reinicia (no quedó atascada en "pendiente" para siempre)
    assert!(matches!(pkgsearch::lookup(Manager::Winget, "zzqq"), State::Pending));
}
