//! Interpolación de rectángulos para animaciones (lógica pura, sin Win32).

use std::time::Instant;

use crate::layout::Rect;

/// Curva cúbica de Bézier como en CSS: (0,0) -> (x1,y1) -> (x2,y2) -> (1,1). Devuelve y para x = t.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let bez = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * a * u * u * s + 3.0 * b * u * s * s + s * s * s
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

/// Curva "decelerate" de Fluent (la que usa Windows 11): arranque rápido y frenada suave, sin rebote.
pub fn ease(t: f32) -> f32 {
    cubic_bezier(0.1, 0.9, 0.2, 1.0, t)
}

fn lerp(a: i32, b: i32, t: f32) -> i32 {
    a + ((b - a) as f32 * t).round() as i32
}

pub fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.w, b.w, t), lerp(a.h, b.h, t))
}

/// Animación de una ventana (rectángulos de ventana, ya con los bordes invisibles incluidos).
#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub from: Rect,
    pub to: Rect,
    pub start: Instant,
    pub dur_ms: u32,
    /// Al terminar, mover la ventana fuera de pantalla (workspace que se va).
    pub then_hide: bool,
}

impl Item {
    pub fn new(from: Rect, to: Rect, dur_ms: u32) -> Self {
        Self { from, to, start: Instant::now(), dur_ms, then_hide: false }
    }

    /// Retrasa el arranque `ms` milisegundos (la ventana se queda en `from` hasta entonces).
    pub fn delayed(mut self, ms: u32) -> Self {
        self.start += std::time::Duration::from_millis(ms as u64);
        self
    }

    pub fn hiding(mut self) -> Self {
        self.then_hide = true;
        self
    }

    /// Rectángulo en el instante `now` y si la animación ya terminó.
    pub fn at(&self, now: Instant) -> (Rect, bool) {
        if self.dur_ms == 0 {
            return (self.to, true);
        }
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32() * 1000.0;
        let t = (elapsed / self.dur_ms as f32).min(1.0);
        if t >= 1.0 {
            (self.to, true)
        } else {
            (lerp_rect(self.from, self.to, ease(t)), false)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub h: isize,
    pub rect: Rect,
    pub done: bool,
    /// Último fotograma de una animación de salida: hay que ocultar la ventana.
    pub hide: bool,
}

/// Avanza todas las animaciones al instante `now`; las terminadas se eliminan de la lista.
pub fn step(items: &mut Vec<(isize, Item)>, now: Instant) -> Vec<Frame> {
    let mut out = Vec::with_capacity(items.len());
    items.retain(|&(h, it)| {
        let (rect, done) = it.at(now);
        out.push(Frame { h, rect, done, hide: done && it.then_hide });
        !done
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn easing_is_monotonic_bounded_and_decelerates() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        let mut prev = 0.0;
        for i in 1..=100 {
            let v = ease(i as f32 / 100.0);
            assert!(v >= prev && v <= 1.0, "no monótona en {i}");
            prev = v;
        }
        // decelerate: a mitad de tiempo ya se ha recorrido casi todo
        assert!(ease(0.5) > 0.85);
    }

    #[test]
    fn item_reaches_target() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(200, 100, 300, 50);
        let it = Item::new(a, b, 100);
        assert_eq!(it.at(it.start), (a, false));
        assert_eq!(it.at(it.start + Duration::from_millis(500)), (b, true));
        let (mid, _) = it.at(it.start + Duration::from_millis(50));
        assert!(mid.x > 0 && mid.x < 200);
    }

    #[test]
    fn delayed_item_waits_at_origin() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(100, 0, 10, 10);
        let it = Item::new(a, b, 100).delayed(50);
        assert_eq!(it.at(it.start - std::time::Duration::from_millis(20)), (a, false));
        assert_eq!(it.at(it.start + std::time::Duration::from_millis(200)), (b, true));
    }

    #[test]
    fn step_removes_finished_and_flags_hide() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(100, 0, 10, 10);
        let mut items = vec![(1, Item::new(a, b, 100).hiding()), (2, Item::new(a, b, 1000))];
        let t0 = items[0].1.start;
        let f = step(&mut items, t0 + Duration::from_millis(200));
        assert_eq!(f.len(), 2);
        assert!(f[0].done && f[0].hide && f[0].rect == b);
        assert!(!f[1].done && !f[1].hide);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, 2);
    }
}
