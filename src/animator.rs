//! Motor de animaciones: un hilo propio sincronizado con el vsync del monitor.
//!
//! Duerme sin consumir CPU cuando no hay nada animándose. Los fotogramas intermedios se envían con
//! `SWP_ASYNCWINDOWPOS` para que una aplicación lenta no bloquee a las demás; el último es síncrono.

use std::sync::{Condvar, Mutex, MutexGuard, Once};
use std::time::Instant;

use crate::anim::{self, Item};
use crate::window;

struct Shared {
    items: Vec<(isize, Item)>,
    quit: bool,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared { items: Vec::new(), quit: false });
static WAKE: Condvar = Condvar::new();
static SPAWN: Once = Once::new();

fn lock() -> MutexGuard<'static, Shared> {
    SHARED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Empieza (o reemplaza) la animación de la ventana `h`.
pub fn start(h: isize, item: Item) {
    SPAWN.call_once(|| {
        let _ = std::thread::Builder::new().name("hypr-win-anim".into()).spawn(run);
    });
    let mut g = lock();
    g.items.retain(|&(x, _)| x != h);
    g.items.push((h, item));
    WAKE.notify_one();
}

pub fn cancel(h: isize) {
    lock().items.retain(|&(x, _)| x != h);
}

pub fn cancel_all() {
    lock().items.clear();
}

pub fn shutdown() {
    let mut g = lock();
    g.items.clear();
    g.quit = true;
    WAKE.notify_one();
}

fn run() {
    loop {
        {
            let mut g = lock();
            while g.items.is_empty() && !g.quit {
                g = WAKE.wait(g).unwrap_or_else(|e| e.into_inner());
            }
            if g.quit {
                return;
            }
        }
        window::wait_vsync();
        let frames = {
            let mut g = lock();
            anim::step(&mut g.items, Instant::now())
        };
        for f in frames {
            if !window::is_alive(f.h) {
                continue;
            }
            window::set_rect(f.h, f.rect, !f.done);
            if f.hide {
                window::hide(&[f.h]);
            }
        }
    }
}
