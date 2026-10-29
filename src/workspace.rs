//! Modelo de workspaces y monitores (lógica pura, sin Win32).
//! Las ventanas se identifican por su HWND como `isize`.
//!
//! Estilo Hyprland: los workspaces son globales y cada monitor muestra uno.

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
    /// Workspace visible en cada monitor.
    pub mon_ws: Vec<usize>,
    pub focused_mon: usize,
}

impl Model {
    /// El monitor `i` muestra inicialmente el workspace `i`. Requiere `monitors <= count`.
    pub fn new(count: usize, layout: LayoutKind, monitors: usize) -> Self {
        let monitors = monitors.clamp(1, count);
        Self {
            ws: (0..count).map(|_| Workspace::new(layout)).collect(),
            mon_ws: (0..monitors).collect(),
            focused_mon: 0,
        }
    }

    pub fn active(&self) -> usize {
        self.mon_ws[self.focused_mon]
    }

    pub fn active_ws(&self) -> &Workspace {
        &self.ws[self.active()]
    }

    pub fn active_mut(&mut self) -> &mut Workspace {
        let i = self.active();
        &mut self.ws[i]
    }

    pub fn find(&self, h: isize) -> Option<usize> {
        self.ws.iter().position(|w| w.windows.contains(&h))
    }

    /// Monitor que muestra el workspace `ws`, si alguno.
    pub fn visible_on(&self, ws: usize) -> Option<usize> {
        self.mon_ws.iter().position(|&w| w == ws)
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

/// Índice del rectángulo que contiene el punto, o el más cercano si ninguno.
pub fn monitor_at(rects: &[Rect], x: i32, y: i32) -> usize {
    let dist = |r: &Rect| {
        let dx = (r.x - x).max(0).max(x - r.right() + 1) as i64;
        let dy = (r.y - y).max(0).max(y - r.bottom() + 1) as i64;
        dx * dx + dy * dy
    };
    rects.iter().enumerate().min_by_key(|(_, r)| dist(r)).map_or(0, |(i, _)| i)
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
        let mut m = Model::new(3, LayoutKind::Dwindle, 1);
        m.add(1, 0);
        m.add(2, 0);
        assert_eq!(m.ws[0].focused, Some(2));
        assert_eq!(m.remove(2), Some(0));
        assert_eq!(m.ws[0].focused, Some(1));
        assert_eq!(m.remove(1), Some(0));
        assert_eq!(m.ws[0].focused, None);
        assert_eq!(m.remove(9), None);
    }

    #[test]
    fn monitors_start_on_their_own_workspace() {
        let m = Model::new(9, LayoutKind::Dwindle, 2);
        assert_eq!(m.mon_ws, vec![0, 1]);
        assert_eq!(m.visible_on(1), Some(1));
        assert_eq!(m.visible_on(5), None);
        // más monitores que workspaces: se recorta
        assert_eq!(Model::new(1, LayoutKind::Dwindle, 3).mon_ws, vec![0]);
    }

    #[test]
    fn monitor_at_point() {
        let mons = [Rect::new(0, 0, 1920, 1080), Rect::new(1920, 0, 1920, 1080)];
        assert_eq!(monitor_at(&mons, 100, 100), 0);
        assert_eq!(monitor_at(&mons, 2500, 500), 1);
        assert_eq!(monitor_at(&mons, -32000, -32000), 0);
        assert_eq!(monitor_at(&mons, 9000, 0), 1);
    }
}
