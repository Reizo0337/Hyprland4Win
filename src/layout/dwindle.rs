use super::{Gaps, Layout, Rect};

/// Divisiones binarias: la primera ventana toma la mitad del espacio y el resto
/// se subdivide en la otra mitad. El eje se alterna según la proporción del
/// espacio restante (como hace dwindle en Hyprland).
pub struct Dwindle;

impl Layout for Dwindle {
    fn arrange(&self, area: Rect, count: usize, gaps: Gaps) -> Vec<Rect> {
        let mut out = Vec::with_capacity(count);
        let mut rest = area.shrink(gaps.outer);
        for i in 0..count {
            if i + 1 == count {
                out.push(rest);
                break;
            }
            let (first, second) = split(rest, gaps.inner);
            out.push(first);
            rest = second;
        }
        out
    }
}

fn split(r: Rect, gap: i32) -> (Rect, Rect) {
    if r.w >= r.h {
        let w1 = ((r.w - gap) / 2).max(1);
        let w2 = (r.w - gap - w1).max(1);
        (Rect::new(r.x, r.y, w1, r.h), Rect::new(r.x + w1 + gap, r.y, w2, r.h))
    } else {
        let h1 = ((r.h - gap) / 2).max(1);
        let h2 = (r.h - gap - h1).max(1);
        (Rect::new(r.x, r.y, r.w, h1), Rect::new(r.x, r.y + h1 + gap, r.w, h2))
    }
}
