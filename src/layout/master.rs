use super::{Gaps, Layout, Rect};

/// Una ventana master a la izquierda y el resto apilado en vertical a la derecha.
pub struct Master {
    pub ratio: f32,
}

impl Layout for Master {
    fn arrange(&self, area: Rect, count: usize, gaps: Gaps) -> Vec<Rect> {
        let inner = area.shrink(gaps.outer);
        match count {
            0 => Vec::new(),
            1 => vec![inner],
            _ => {
                let avail = (inner.w - gaps.inner).max(2);
                let master_w = ((avail as f32 * self.ratio) as i32).clamp(1, avail - 1);
                let stack_w = (avail - master_w).max(1);
                let mut out = vec![Rect::new(inner.x, inner.y, master_w, inner.h)];
                let stack = Rect::new(inner.x + master_w + gaps.inner, inner.y, stack_w, inner.h);
                out.extend(stack_vertical(stack, count - 1, gaps.inner));
                out
            }
        }
    }
}

fn stack_vertical(r: Rect, n: usize, gap: i32) -> Vec<Rect> {
    let n_i = n as i32;
    let total = (r.h - gap * (n_i - 1)).max(n_i);
    let base = total / n_i;
    let mut y = r.y;
    (0..n)
        .map(|i| {
            // el último absorbe el resto para no dejar huecos
            let h = (if i + 1 == n { r.y + r.h - y } else { base }).max(1);
            let rect = Rect::new(r.x, y, r.w, h);
            y += h + gap;
            rect
        })
        .collect()
}
