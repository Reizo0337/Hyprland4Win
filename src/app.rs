//! Pegamento entre el modelo de workspaces/monitores, los layouts, las animaciones y Win32.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::anim::Item;
use crate::animator;
use crate::appid::AppIdentity;
use crate::bar;
use crate::dock;
use crate::config::Config;
use crate::keybinds::{Action, Binding, Dir};
use crate::launcher;
use crate::panel;
use crate::layout::{self, Gaps, Rect};
use crate::window::{self, MonitorInfo};
use crate::workspace::{monitor_at, neighbor, Model};

/// Cómo entra una ventana al layout.
#[derive(Clone, Copy)]
enum Enter {
    /// Desde donde está ahora.
    None,
    /// Desde el 94 % de su tamaño (workspace sin eje libre para deslizar).
    PopIn,
    /// Directo a su sitio, sin animar (ventana recién creada: así no se ve el salto).
    Snap,
    /// Desde su destino desplazado (dx, dy): entrada de un workspace deslizándose.
    Slide(i32, i32),
}

/// Los workspaces sin hueco para deslizar aparecen al 94 % de su tamaño (efecto pop-in de Hyprland).
const POPIN_SCALE: f32 = 0.94;

/// Espera antes de que los vecinos ocupen el hueco de una ventana cerrada.
const CLOSE_DELAY_MS: u32 = 110;

pub struct App {
    cfg: Config,
    binds: Vec<Binding>,
    model: Model,
    /// Posición original de cada ventana gestionada, para restaurarla al salir.
    originals: HashMap<isize, Rect>,
    mons: Vec<MonitorInfo>,
    mouse: MouseFocus,
    /// Ventanas recién creadas: van directas a su hueco mientras las demás se animan.
    fresh: HashSet<isize>,
    /// Identidad de app de cada ventana (para el dock); se calcula una sola vez por ventana.
    identities: std::cell::RefCell<HashMap<isize, Option<AppIdentity>>>,
    /// Retraso (ms) del arranque de las animaciones; se usa al cerrar una ventana.
    next_delay: u32,
}

/// Estado del foco por ratón: solo actúa si el cursor se movió y se detuvo un instante sobre la ventana.
#[derive(Default)]
struct MouseFocus {
    last_pt: (i32, i32),
    armed: bool,
    candidate: isize,
    stable: u8,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

pub fn install(app: App) {
    APP.with(|a| *a.borrow_mut() = Some(app));
}

/// Ejecuta `f` sobre la app si no está ya prestada (los mensajes son reentrantes).
fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn adopt_existing() {
    with(|a| a.adopt_existing());
}

pub fn shutdown() {
    with(|a| a.restore_all());
}

pub fn on_hotkey(index: i32) {
    with(|a| {
        if let Some(b) = usize::try_from(index).ok().and_then(|i| a.binds.get(i)).cloned() {
            a.run(b.action);
        }
    });
}

pub fn on_event(event: u32, hwnd: isize) {
    with(|a| a.on_event(event, hwnd));
}

pub fn on_bar_click(bar: usize, ws: usize) {
    with(|a| {
        if bar < a.model.mon_ws.len() {
            a.model.focused_mon = bar;
            a.show_workspace(ws);
        }
    });
}

pub fn dock_activate(hwnds: Vec<isize>) {
    with(|a| a.dock_activate(hwnds));
}

pub fn on_settle() {
    with(|a| a.retile_visible(Enter::None));
}

pub fn on_mouse_tick() {
    with(|a| a.mouse_tick());
}

/// `reposition`: los monitores cambiaron (resolución, disposición); si no, solo el área de trabajo.
pub fn on_work_area_changed(reposition: bool) {
    with(|a| a.refresh_monitors(reposition));
}

pub fn binding_count() -> usize {
    with(|a| a.binds.len()).unwrap_or(0)
}

pub fn bindings() -> Vec<Binding> {
    with(|a| a.binds.clone()).unwrap_or_default()
}

pub fn build(cfg: Config, mons: Vec<MonitorInfo>) -> Result<App, String> {
    let binds = cfg.bindings()?;
    let model = Model::new(cfg.general.workspaces, cfg.layout_kind(), mons.len());
    let managed = model.mon_ws.len();
    let mut mons = mons;
    mons.truncate(managed);
    claim_taskbar_space(&mut mons, cfg.general.hide_taskbar, dock_reserve(&cfg));
    Ok(App { cfg, binds, model, originals: HashMap::new(), mons, mouse: MouseFocus::default(), fresh: HashSet::new(), identities: Default::default(), next_delay: 0 })
}

impl App {
    fn gaps(&self) -> Gaps {
        Gaps { inner: self.cfg.general.gaps_in, outer: self.cfg.general.gaps_out }
    }

    fn mon_rects(&self) -> Vec<Rect> {
        self.mons.iter().map(|m| m.rect).collect()
    }

    fn rects_for(&self, ws: usize, mon: usize) -> Vec<Rect> {
        let w = &self.model.ws[ws];
        layout::arrange(w.layout, self.mons[mon].work, w.windows.len(), self.gaps(), self.cfg.general.master_ratio)
    }

    fn refresh_monitors(&mut self, reposition: bool) {
        let mut now = window::monitors();
        if now.len() < self.mons.len() {
            eprintln!("aviso: cambió el número de monitores; reinicia hypr-win");
            return;
        }
        now.truncate(self.mons.len());
        if reposition {
            bar::reposition_all(&now);
            now = window::monitors();
            now.truncate(self.mons.len());
        }
        claim_taskbar_space(&mut now, self.cfg.general.hide_taskbar, dock_reserve(&self.cfg));
        let changed = now.iter().zip(&self.mons).any(|(a, b)| a.rect != b.rect || a.work != b.work);
        if changed {
            self.mons = now;
            dock::set_monitors(&self.mon_rects());
            self.retile_visible(Enter::None);
        }
    }

    fn adopt_existing(&mut self) {
        for h in window::enumerate() {
            self.track(h);
        }
        let fg = window::foreground();
        if let Some(ws) = self.model.find(fg) {
            self.model.ws[ws].focused = Some(fg);
            if let Some(mon) = self.model.visible_on(ws) {
                self.model.focused_mon = mon;
            }
        }
        self.fresh.clear();
        self.retile_visible(Enter::None);
        if self.cfg.general.focus_follows_mouse {
            unsafe {
                SetTimer(Some(bar::hwnd()), bar::TIMER_MOUSE, 40, None);
            }
        }
    }

    fn mouse_tick(&mut self) {
        if launcher::is_open() || panel::is_open() {
            return; // el lanzador y el panel necesitan conservar el foco
        }
        let Some(pt) = window::cursor_pos() else { return };
        let m = &mut self.mouse;
        if pt != m.last_pt {
            m.last_pt = pt;
            m.armed = true;
        }
        if !m.armed || window::mouse_button_down() {
            return;
        }
        let h = window::window_at(pt);
        if h != m.candidate {
            m.candidate = h;
            m.stable = 0;
            return;
        }
        m.stable = m.stable.saturating_add(1);
        if m.stable != 2 {
            return;
        }
        m.armed = false;
        let Some(ws) = self.model.find(h) else { return };
        let Some(mon) = self.model.visible_on(ws) else { return };
        if window::foreground() == h && mon == self.model.focused_mon {
            return;
        }
        self.model.ws[ws].focused = Some(h);
        self.model.focused_mon = mon;
        window::focus(h);
        self.update_bar();
    }

    /// Empieza a gestionar la ventana en el workspace visible del monitor donde está.
    fn track(&mut self, h: isize) -> Option<usize> {
        if self.model.find(h).is_some() || !window::is_manageable(window::to_hwnd(h)) {
            return None;
        }
        let cur = window::current_rect(h)?;
        if let Some(r) = window::normal_rect(window::to_hwnd(h)) {
            self.originals.insert(h, r);
        }
        let (cx, cy) = cur.center();
        let mon = monitor_at(&self.mon_rects(), cx, cy);
        let ws = self.model.mon_ws[mon];
        self.model.add(h, ws);
        self.fresh.insert(h);
        Some(ws)
    }

    fn update_bar(&self) {
        let occupied = self.model.ws.iter().map(|w| !w.windows.is_empty()).collect();
        let per_mon = self
            .model
            .mon_ws
            .iter()
            .map(|&ws| {
                let title = self.model.ws[ws]
                    .focus_target()
                    .map(|h| window::title(window::to_hwnd(h)))
                    .unwrap_or_default();
                (ws, title)
            })
            .collect();
        bar::set_state(per_mon, occupied, self.model.focused_mon);
        self.update_dock();
    }

    /// Agrupa las ventanas gestionadas por aplicación y se las pasa al dock.
    fn update_dock(&self) {
        if !self.cfg.dock.enabled {
            return;
        }
        let mut groups: Vec<dock::Running> = Vec::new();
        for ws in &self.model.ws {
            for &h in &ws.windows {
                let ident = self.identities.borrow_mut().entry(h).or_insert_with(|| window::app_identity(h)).clone();
                let Some(ident) = ident else { continue };
                match groups.iter_mut().find(|g| g.ident.key() == ident.key()) {
                    Some(g) => g.hwnds.push(h),
                    None => groups.push(dock::Running { ident, hwnds: vec![h] }),
                }
            }
        }
        dock::update(groups);
    }

    /// Clic en el dock: enfoca una ventana de la app (la siguiente si ya tiene el foco).
    fn dock_activate(&mut self, hwnds: Vec<isize>) {
        let fg = window::foreground();
        let pos = hwnds.iter().position(|&h| h == fg);
        let Some(&h) = hwnds.get(pos.map_or(0, |p| (p + 1) % hwnds.len())) else { return };
        let Some(ws) = self.model.find(h) else { return };
        self.model.ws[ws].focused = Some(h);
        match self.model.visible_on(ws) {
            Some(mon) => {
                self.model.focused_mon = mon;
                window::focus(h);
                self.update_bar();
            }
            None => self.show_workspace(ws),
        }
    }

    fn retile_visible(&mut self, enter: Enter) {
        for ws in self.model.mon_ws.clone() {
            self.retile_ws(ws, enter);
        }
    }

    /// Recalcula el layout del workspace `ws` (si algún monitor lo muestra) y anima las ventanas.
    fn retile_ws(&mut self, ws: usize, enter: Enter) {
        let Some(mon) = self.model.visible_on(ws) else { return };
        let w = &self.model.ws[ws];
        let fullscreen = w.fullscreen;
        let items: Vec<(isize, Rect)> = match fullscreen {
            Some(f) => vec![(f, self.mons[mon].work)],
            None => w.windows.iter().copied().zip(self.rects_for(ws, mon)).collect(),
        };
        for (h, vis) in items {
            let e = if self.fresh.remove(&h) { Enter::Snap } else { enter };
            self.move_to(h, vis, e);
        }
        if let Some(f) = fullscreen {
            window::raise(f);
        }
        self.update_bar();
    }

    /// Lleva la ventana a su rectángulo visible `vis`, con animación si está activada.
    fn move_to(&mut self, h: isize, vis: Rect, enter: Enter) {
        let to = window::target_rect(h, vis);
        if matches!(enter, Enter::Snap) {
            animator::cancel(h);
            window::apply(&[(h, to)]);
            return;
        }
        let from = match enter {
            Enter::None => window::current_rect(h).unwrap_or(to),
            Enter::PopIn => to.scaled(POPIN_SCALE),
            Enter::Snap => to,
            Enter::Slide(dx, dy) => Rect::new(to.x + dx, to.y + dy, to.w, to.h),
        };
        let dur = self.cfg.general.animation_ms;
        if from == to {
            animator::cancel(h);
            return;
        }
        if dur == 0 {
            animator::cancel(h);
            window::apply(&[(h, to)]);
            return;
        }
        if !matches!(enter, Enter::None) {
            window::apply(&[(h, from)]); // primer fotograma ya en pantalla
        }
        animator::start(h, Item::new(from, to, dur).delayed(self.next_delay));
    }

    /// Desplazamiento de entrada para el workspace nuevo de un monitor, si hay un eje libre:
    /// horizontal si no hay monitores a los lados, vertical si no los hay encima/debajo
    /// (así las ventanas nunca invaden otra pantalla). `None` -> pop-in.
    fn slide_offset(&self, mon: usize, forward: bool) -> Option<(i32, i32)> {
        if self.cfg.general.animation_ms == 0 {
            return None;
        }
        let r = self.mons[mon].rect;
        let others = || self.mons.iter().enumerate().filter(|&(i, _)| i != mon).map(|(_, m)| m.rect);
        let sign = if forward { 1 } else { -1 };
        let side = others().any(|o| o.y < r.bottom() && r.y < o.bottom());
        let stacked = others().any(|o| o.x < r.right() && r.x < o.right());
        if !side {
            Some((sign * r.w, 0))
        } else if !stacked {
            Some((0, sign * r.h))
        } else {
            None
        }
    }

    /// Las ventanas del workspace que se va salen deslizándose y se ocultan al terminar.
    fn slide_out(&mut self, ws: usize, dx: i32, dy: i32) {
        let dur = self.cfg.general.animation_ms;
        for h in self.model.ws[ws].windows.clone() {
            let Some(cur) = window::current_rect(h) else { continue };
            let to = Rect::new(cur.x + dx, cur.y + dy, cur.w, cur.h);
            animator::start(h, Item::new(cur, to, dur).hiding());
        }
    }

    fn hide_ws(&mut self, ws: usize) {
        let list = self.model.ws[ws].windows.clone();
        for h in &list {
            animator::cancel(*h);
        }
        window::hide(&list);
    }

    fn focus_active(&self) {
        if let Some(h) = self.model.active_ws().focus_target() {
            window::focus(h);
        }
    }

    fn on_event(&mut self, event: u32, h: isize) {
        match event {
            EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW | EVENT_SYSTEM_MINIMIZEEND | EVENT_OBJECT_NAMECHANGE => {
                if let Some(ws) = self.track(h) {
                    self.retile_ws(ws, Enter::None);
                    self.schedule_settle();
                } else if event == EVENT_OBJECT_NAMECHANGE {
                    self.update_bar();
                }
            }
            EVENT_OBJECT_DESTROY => self.untrack(h),
            EVENT_OBJECT_HIDE | EVENT_SYSTEM_MINIMIZESTART => {
                if event == EVENT_SYSTEM_MINIMIZESTART || !window::is_visible(h) {
                    self.untrack(h);
                }
            }
            EVENT_SYSTEM_FOREGROUND => {
                if let Some(ws) = self.track(h) {
                    self.retile_ws(ws, Enter::None);
                    self.schedule_settle();
                }
                if let Some(ws) = self.model.find(h) {
                    self.model.ws[ws].focused = Some(h);
                    match self.model.visible_on(ws) {
                        Some(mon) => {
                            self.model.focused_mon = mon;
                            self.update_bar();
                        }
                        // alt-tab hacia una ventana de un workspace oculto: lo traemos al monitor enfocado
                        None => self.show_workspace(ws),
                    }
                }
            }
            EVENT_SYSTEM_MOVESIZEEND => self.on_move_end(h),
            _ => {}
        }
    }

    /// Al soltar una ventana arrastrada: si cayó en otro monitor, pasa al workspace de ese monitor.
    fn on_move_end(&mut self, h: isize) {
        let Some(ws) = self.model.find(h) else { return };
        let Some(mon) = self.model.visible_on(ws) else { return };
        let Some(cur) = window::current_rect(h) else { return };
        let (cx, cy) = cur.center();
        let dest = monitor_at(&self.mon_rects(), cx, cy);
        if dest != mon {
            let target = self.model.mon_ws[dest];
            self.model.remove(h);
            self.model.add(h, target);
            self.retile_ws(ws, Enter::None);
            self.retile_ws(target, Enter::None);
        } else {
            self.retile_ws(ws, Enter::None);
        }
    }

    /// Muchas apps reajustan su propia posición justo después de abrirse: volvemos a colocar todo
    /// cuando se calman, en lugar de pelear fotograma a fotograma.
    fn schedule_settle(&self) {
        unsafe {
            SetTimer(Some(bar::hwnd()), bar::TIMER_SETTLE, 250, None);
        }
    }

    fn untrack(&mut self, h: isize) {
        let Some(ws) = self.model.find(h) else { return };
        let was_focused = self.model.ws[ws].focused == Some(h);
        self.model.remove(h);
        self.originals.remove(&h);
        self.identities.borrow_mut().remove(&h);
        animator::cancel(h);
        if let Some(mon) = self.model.visible_on(ws) {
            // los vecinos esperan a que termine el fundido de cierre nativo de Windows
            self.next_delay = CLOSE_DELAY_MS;
            self.retile_ws(ws, Enter::None);
            self.next_delay = 0;
            if was_focused && mon == self.model.focused_mon {
                self.focus_active();
            }
        }
        self.update_bar();
    }

    /// Super+N: si otro monitor ya muestra ese workspace, el foco va a ese monitor;
    /// si no, el monitor enfocado pasa a mostrarlo.
    fn show_workspace(&mut self, n: usize) {
        if n >= self.model.ws.len() {
            return;
        }
        let m = self.model.focused_mon;
        if let Some(k) = self.model.visible_on(n) {
            if k != m {
                self.model.focused_mon = k;
                self.focus_active();
                self.update_bar();
            }
            return;
        }
        let old = self.model.mon_ws[m];
        match self.slide_offset(m, n > old) {
            Some((dx, dy)) => {
                self.slide_out(old, -dx, -dy);
                self.model.mon_ws[m] = n;
                self.retile_ws(n, Enter::Slide(dx, dy));
            }
            None => {
                self.hide_ws(old);
                self.model.mon_ws[m] = n;
                self.retile_ws(n, Enter::PopIn);
            }
        }
        self.focus_active();
        self.update_bar();
    }

    fn move_focused_to(&mut self, n: usize) {
        let from = self.model.active();
        if n >= self.model.ws.len() || n == from {
            return;
        }
        let Some(h) = self.model.active_ws().focus_target() else { return };
        self.model.remove(h);
        self.model.add(h, n);
        animator::cancel(h);
        if self.model.visible_on(n).is_some() {
            self.retile_ws(n, Enter::None);
        } else {
            window::hide(&[h]);
        }
        self.retile_ws(from, Enter::None);
        self.focus_active();
        self.update_bar();
    }

    fn adjacent_monitor(&self, dir: Dir) -> Option<usize> {
        neighbor(&self.mon_rects(), self.model.focused_mon, dir)
    }

    fn focused_index(&self) -> Option<usize> {
        let w = self.model.active_ws();
        w.focus_target().and_then(|f| w.windows.iter().position(|&x| x == f))
    }

    fn focus_dir(&mut self, dir: Dir) {
        let ws = self.model.active();
        self.model.active_mut().fullscreen = None;
        if let Some(from) = self.focused_index() {
            if let Some(to) = neighbor(&self.rects_for(ws, self.model.focused_mon), from, dir) {
                let h = self.model.ws[ws].windows[to];
                self.model.active_mut().focused = Some(h);
                self.retile_ws(ws, Enter::None);
                window::focus(h);
                return;
            }
        }
        if let Some(mon) = self.adjacent_monitor(dir) {
            self.model.focused_mon = mon;
            self.focus_active();
            self.update_bar();
        }
    }

    fn move_dir(&mut self, dir: Dir) {
        let ws = self.model.active();
        self.model.active_mut().fullscreen = None;
        let Some(from) = self.focused_index() else { return };
        if let Some(to) = neighbor(&self.rects_for(ws, self.model.focused_mon), from, dir) {
            self.model.active_mut().windows.swap(from, to);
            self.retile_ws(ws, Enter::None);
        } else if let Some(mon) = self.adjacent_monitor(dir) {
            // al borde del monitor: la ventana salta al workspace del monitor vecino
            let target = self.model.mon_ws[mon];
            self.move_focused_to(target);
            self.model.focused_mon = mon;
            self.focus_active();
            self.update_bar();
        }
    }

    /// Workspace `d` pasos desde el activo (con vuelta), saltando los que otro monitor está mostrando.
    fn relative_workspace(&self, d: i32) -> Option<usize> {
        let len = self.model.ws.len() as i32;
        let from = self.model.active() as i32;
        (1..len)
            .map(|k| (from + d * k).rem_euclid(len) as usize)
            .find(|&n| self.model.visible_on(n).is_none())
    }

    /// Alt+Tab del mosaico: pasa el foco a la ventana siguiente/anterior del workspace.
    fn cycle_focus(&mut self, d: i32) {
        let ws = self.model.active();
        let Some(from) = self.focused_index() else { return };
        let n = self.model.ws[ws].windows.len() as i32;
        if n < 2 {
            return;
        }
        let h = self.model.ws[ws].windows[(from as i32 + d).rem_euclid(n) as usize];
        let w = self.model.active_mut();
        w.focused = Some(h);
        if w.fullscreen.is_some() {
            w.fullscreen = Some(h);
        }
        self.retile_ws(ws, Enter::None);
        window::focus(h);
    }

    fn toggle_fullscreen(&mut self) {
        let ws = self.model.active();
        let w = self.model.active_mut();
        let Some(f) = w.focus_target() else { return };
        w.fullscreen = if w.fullscreen == Some(f) { None } else { Some(f) };
        self.retile_ws(ws, Enter::None);
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Exec(cmd) => exec(&cmd),
            Action::Close => {
                if let Some(h) = self.model.active_ws().focus_target() {
                    window::close(h);
                }
            }
            Action::Focus(d) => self.focus_dir(d),
            Action::Move(d) => self.move_dir(d),
            Action::Workspace(n) => self.show_workspace(n),
            Action::MoveToWorkspace(n) => self.move_focused_to(n),
            Action::WorkspaceRel(d) => {
                if let Some(n) = self.relative_workspace(d) {
                    self.show_workspace(n);
                }
            }
            Action::MoveToWorkspaceRel(d) => {
                if let Some(n) = self.relative_workspace(d) {
                    self.move_focused_to(n);
                }
            }
            Action::Cycle(d) => self.cycle_focus(d),
            Action::Launcher => launcher::toggle(self.mons[self.model.focused_mon].rect),
            Action::Panel => panel::toggle(self.mons[self.model.focused_mon].rect, bar::height_px()),
            Action::None => {}
            Action::Fullscreen => self.toggle_fullscreen(),
            Action::ToggleLayout => {
                let ws = self.model.active();
                let w = self.model.active_mut();
                w.layout = w.layout.toggled();
                self.retile_ws(ws, Enter::None);
            }
            Action::Quit => unsafe {
                let _ = PostMessageW(Some(bar::hwnd()), WM_CLOSE, Default::default(), Default::default());
            },
        }
    }

    /// Devuelve todas las ventanas a su posición original (o a una visible si esa ya no lo es).
    fn restore_all(&mut self) {
        animator::shutdown();
        let screen = unsafe {
            Rect::new(
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let fallback = self.mons.first().map_or(screen, |m| m.work);
        for (&h, orig) in &self.originals {
            if !window::is_alive(h) {
                continue;
            }
            let visible =
                Rect::new(orig.x + 40, orig.y + 40, (orig.w - 80).max(1), (orig.h - 80).max(1)).intersects(&screen);
            let target = if visible {
                *orig
            } else {
                let (w, hh) = (orig.w.min(fallback.w), orig.h.min(fallback.h));
                Rect::new(fallback.x + (fallback.w - w) / 2, fallback.y + (fallback.h - hh) / 2, w, hh)
            };
            window::set_exact(h, target);
        }
    }
}

/// Alto que ocupa el dock en cada monitor (0 si está desactivado).
fn dock_reserve(cfg: &Config) -> i32 {
    if cfg.dock.enabled {
        dock::reserve_px(&cfg.dock)
    } else {
        0
    }
}

/// Con la barra de tareas oculta Windows sigue reservando su hueco en `rcWork`: el área de mosaico
/// pasa a ser todo el monitor menos nuestra barra superior. El dock reserva su franja inferior en el
/// de cada monitor gestionado (hay un dock en cada uno).
fn claim_taskbar_space(mons: &mut [MonitorInfo], hide_taskbar: bool, dock_h: i32) {
    if hide_taskbar {
        let bar_h = bar::height_px();
        for m in mons.iter_mut() {
            m.work = Rect::new(m.rect.x, m.rect.y + bar_h, m.rect.w, (m.rect.h - bar_h).max(1));
        }
    }
    for m in mons.iter_mut() {
        m.work.h = (m.work.h - dock_h).max(1);
    }
}

fn exec(cmd: &str) {
    let cmd = cmd.trim();
    let (file, params) = match cmd.split_once(' ') {
        Some((f, p)) => (f, p.trim()),
        None => (cmd, ""),
    };
    let file = HSTRING::from(file);
    let params = HSTRING::from(params);
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            if params.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_ptr()) },
            None,
            SW_SHOWNORMAL,
        );
    }
}
