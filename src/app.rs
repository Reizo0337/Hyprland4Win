//! Pegamento entre el modelo de workspaces, los layouts y Win32.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::bar;
use crate::config::Config;
use crate::keybinds::{Action, Binding, Dir};
use crate::layout::{self, Gaps, Rect};
use crate::window;
use crate::workspace::{neighbor, Model};

pub struct App {
    cfg: Config,
    binds: Vec<Binding>,
    model: Model,
    /// Posición original de cada ventana gestionada, para restaurarla al salir.
    originals: HashMap<isize, Rect>,
    work: Rect,
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

pub fn on_bar_click(index: usize) {
    with(|a| a.switch_workspace(index));
}

pub fn on_work_area_changed() {
    with(|a| {
        let new = work_area();
        if new != a.work {
            a.work = new;
            a.retile();
        }
    });
}

pub fn work_area() -> Rect {
    let mut r = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut _ as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
    Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}

impl App {
    pub fn new(cfg: Config, binds: Vec<Binding>) -> Self {
        let model = Model::new(cfg.general.workspaces, cfg.layout_kind());
        Self { cfg, binds, model, originals: HashMap::new(), work: work_area() }
    }

    fn gaps(&self) -> Gaps {
        Gaps { inner: self.cfg.general.gaps_in, outer: self.cfg.general.gaps_out }
    }

    fn rects(&self, ws: usize) -> Vec<Rect> {
        let w = &self.model.ws[ws];
        layout::arrange(w.layout, self.work, w.windows.len(), self.gaps(), self.cfg.general.master_ratio)
    }

    fn adopt_existing(&mut self) {
        for h in window::enumerate() {
            self.track(h);
        }
        let fg = window::foreground();
        if self.model.active_ws().windows.contains(&fg) {
            self.model.active_mut().focused = Some(fg);
        }
        self.retile();
    }

    fn track(&mut self, h: isize) -> bool {
        if self.model.find(h).is_some() || !window::is_manageable(window::to_hwnd(h)) {
            return false;
        }
        if let Some(r) = window::normal_rect(window::to_hwnd(h)) {
            self.originals.insert(h, r);
        }
        self.model.add(h, self.model.active);
        true
    }

    fn update_bar(&self) {
        let occupied = self.model.ws.iter().map(|w| !w.windows.is_empty()).collect();
        let title = self
            .model
            .active_ws()
            .focus_target()
            .map(|h| window::title(window::to_hwnd(h)))
            .unwrap_or_default();
        bar::set_state(self.model.active, occupied, title);
    }

    /// Recalcula y aplica el layout del workspace activo.
    fn retile(&mut self) {
        let ws = self.model.active;
        let w = &self.model.ws[ws];
        let items: Vec<(isize, Rect)> = match w.fullscreen {
            Some(f) => vec![(f, self.work)],
            None => w.windows.iter().copied().zip(self.rects(ws)).collect(),
        };
        window::place(&items);
        if let Some(f) = w.fullscreen {
            window::raise(f);
        }
        self.update_bar();
    }

    fn focus_active(&self) {
        if let Some(h) = self.model.active_ws().focus_target() {
            window::focus(h);
        }
    }

    fn on_event(&mut self, event: u32, h: isize) {
        match event {
            EVENT_OBJECT_CREATE | EVENT_OBJECT_SHOW | EVENT_SYSTEM_MINIMIZEEND => {
                if self.track(h) {
                    self.retile();
                }
            }
            EVENT_OBJECT_DESTROY => self.untrack(h),
            EVENT_OBJECT_HIDE | EVENT_SYSTEM_MINIMIZESTART => {
                if event == EVENT_SYSTEM_MINIMIZESTART || !window::is_visible(h) {
                    self.untrack(h);
                }
            }
            EVENT_SYSTEM_FOREGROUND => {
                if self.track(h) {
                    self.retile();
                }
                match self.model.find(h) {
                    Some(ws) if ws != self.model.active => {
                        // alt-tab hacia una ventana de otro workspace: seguimos al foco
                        self.model.ws[ws].focused = Some(h);
                        self.switch_workspace(ws);
                    }
                    Some(_) => {
                        self.model.active_mut().focused = Some(h);
                        self.update_bar();
                    }
                    None => {}
                }
            }
            EVENT_SYSTEM_MOVESIZEEND => {
                if self.model.active_ws().windows.contains(&h) {
                    self.retile();
                }
            }
            EVENT_OBJECT_NAMECHANGE => {
                if self.track(h) {
                    self.retile();
                } else {
                    self.update_bar();
                }
            }
            _ => {}
        }
    }

    fn untrack(&mut self, h: isize) {
        let was_focused = self.model.active_ws().focused == Some(h);
        let Some(ws) = self.model.remove(h) else { return };
        self.originals.remove(&h);
        if ws == self.model.active {
            self.retile();
            if was_focused && window::foreground() == h || !window::is_alive(h) && was_focused {
                self.focus_active();
            }
        }
        self.update_bar();
    }

    fn switch_workspace(&mut self, n: usize) {
        if n >= self.model.ws.len() || n == self.model.active {
            return;
        }
        window::hide(&self.model.active_ws().windows);
        self.model.active = n;
        self.retile();
        self.focus_active();
        self.update_bar();
    }

    fn move_to_workspace(&mut self, n: usize) {
        if n >= self.model.ws.len() || n == self.model.active {
            return;
        }
        let Some(h) = self.model.active_ws().focus_target() else { return };
        self.model.remove(h);
        self.model.add(h, n);
        window::hide(&[h]);
        self.retile();
        self.focus_active();
    }

    fn focus_dir(&mut self, dir: Dir) {
        let ws = self.model.active;
        self.model.active_mut().fullscreen = None;
        let w = self.model.active_ws();
        let Some(from) = w.focus_target().and_then(|f| w.windows.iter().position(|&x| x == f)) else { return };
        if let Some(to) = neighbor(&self.rects(ws), from, dir) {
            let h = self.model.ws[ws].windows[to];
            self.model.active_mut().focused = Some(h);
            self.retile();
            window::focus(h);
        }
    }

    fn move_dir(&mut self, dir: Dir) {
        let ws = self.model.active;
        self.model.active_mut().fullscreen = None;
        let w = self.model.active_ws();
        let Some(from) = w.focus_target().and_then(|f| w.windows.iter().position(|&x| x == f)) else { return };
        if let Some(to) = neighbor(&self.rects(ws), from, dir) {
            self.model.active_mut().windows.swap(from, to);
            self.retile();
        }
    }

    fn toggle_fullscreen(&mut self) {
        let w = self.model.active_mut();
        let Some(f) = w.focus_target() else { return };
        w.fullscreen = if w.fullscreen == Some(f) { None } else { Some(f) };
        self.retile();
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
            Action::Workspace(n) => self.switch_workspace(n),
            Action::MoveToWorkspace(n) => self.move_to_workspace(n),
            Action::Fullscreen => self.toggle_fullscreen(),
            Action::ToggleLayout => {
                let w = self.model.active_mut();
                w.layout = w.layout.toggled();
                self.retile();
            }
            Action::Quit => unsafe {
                let _ = PostMessageW(Some(bar::hwnd()), WM_CLOSE, Default::default(), Default::default());
            },
        }
    }

    /// Devuelve todas las ventanas a su posición original (o a una visible si esa ya no lo es).
    fn restore_all(&mut self) {
        let screen = unsafe { Rect::new(0, 0, GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        for (&h, orig) in &self.originals {
            if !window::is_alive(h) {
                continue;
            }
            let visible = Rect::new(orig.x + 40, orig.y + 40, (orig.w - 80).max(1), (orig.h - 80).max(1)).intersects(&screen);
            let target = if visible {
                *orig
            } else {
                let (w, hh) = (orig.w.min(self.work.w), orig.h.min(self.work.h));
                Rect::new(self.work.x + (self.work.w - w) / 2, self.work.y + (self.work.h - hh) / 2, w, hh)
            };
            window::set_exact(h, target);
        }
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

pub fn build(cfg: Config) -> Result<App, String> {
    let binds = cfg.bindings()?;
    Ok(App::new(cfg, binds))
}

pub fn binding_count() -> usize {
    with(|a| a.binds.len()).unwrap_or(0)
}

pub fn bindings() -> Vec<Binding> {
    with(|a| a.binds.clone()).unwrap_or_default()
}
