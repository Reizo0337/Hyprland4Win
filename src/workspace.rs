//! Modelo de workspaces (lógica pura, sin Win32). Las ventanas se identifican por su HWND como `isize`.

use crate::keybinds::Dir;
use crate::layout::{LayoutKind, Rect};

#[derive(Clone, Debug)]
pub struct Workspace {
    pub windows: Vec<isize>,
    pub layout: LayoutKind,
    pub focused: Option<isize>,
    pub fullscreen: Option<isize>,
}

impl Workspace {
    pub fn new(layout: LayoutKind) -> Self {
        Self { windows: Vec::new(), layout, focused: None, fullscreen: None }
    }

    /// Foco válido: el recordado si sigue en la lista, si no la última ventana.
    pub fn focus_target(&self) -> Option<isize> {
        self.focused
            .filter(|f| self.windows.contains(f))
            .or_else(|| self.windows.last().copied())
    }
}

#[derive(Clone, Debug)]
pub struct Model {
    pub ws: Vec<Workspace>,
    pub active: usize,
}

impl Model {
    pub fn new(count: usize, layout: LayoutKind) -> Self {
        Self { ws: (0..count).map(|_| Workspace::new(layout)).collect(), active: 0 }
    }

    pub fn find(&self, h: isize) -> Option<usize> {
        self.ws.iter().position(|w| w.windows.contains(&h))
    }

    pub fn active_ws(&self) -> &Workspace {
        &self.ws[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        &mut self.ws[self.active]
    }

    /// Añade al workspace `n` y lo marca como enfocado ahí.
    pub fn add(&mut self, h: isize, n: usize) {
        let w = &mut self.ws[n];
        w.windows.push(h);
        w.focused = Some(h);
    }

    /// Quita la ventana; devuelve el índice del workspace donde estaba.
    pub fn remove(&mut self, h: isize) -> Option<usize> {
        let i = self.find(h)?;
        let w = &mut self.ws[i];
        w.windows.retain(|&x| x != h);
        if w.fullscreen == Some(h) {
            w.fullscreen = None;
        }
        w.focused = w.focus_target();
        Some(i)
    }
}

/// Vecino más cercano de `from` en la dirección dada (centros de los rectángulos).
pub fn neighbor(rects: &[Rect], from: usize, dir: Dir) -> Option<usize> {
    let (cx, cy) = rects.get(from)?.center();
    rects
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .filter_map(|(i, r)| {
            let (x, y) = r.center();
            let (dx, dy) = (x - cx, y - cy);
            let (along, across) = match dir {
                Dir::Left => (-dx, dy.abs()),
                Dir::Right => (dx, dy.abs()),
                Dir::Up => (-dy, dx.abs()),
                Dir::Down => (dy, dx.abs()),
            };
            (along > 0).then_some((i, along + 2 * across))
        })
        .min_by_key(|&(_, score)| score)
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbor_picks_nearest_in_direction() {
        let r = [
            Rect::new(0, 0, 100, 200),
            Rect::new(100, 0, 100, 100),
            Rect::new(100, 100, 100, 100),
        ];
        assert_eq!(neighbor(&r, 0, Dir::Right), Some(1));
        assert_eq!(neighbor(&r, 1, Dir::Down), Some(2));
        assert_eq!(neighbor(&r, 2, Dir::Left), Some(0));
        assert_eq!(neighbor(&r, 0, Dir::Left), None);
    }

    #[test]
    fn model_add_remove_keeps_focus_valid() {
        let mut m = Model::new(3, LayoutKind::Dwindle);
        m.add(1, 0);
        m.add(2, 0);
        assert_eq!(m.ws[0].focused, Some(2));
        assert_eq!(m.remove(2), Some(0));
        assert_eq!(m.ws[0].focused, Some(1));
        assert_eq!(m.remove(1), Some(0));
        assert_eq!(m.ws[0].focused, None);
        assert_eq!(m.remove(9), None);
    }
}
