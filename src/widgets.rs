//! Widgets de escritorio: tarjetas translúcidas (ventanas propias con alfa por píxel) que viven detrás de las
//! ventanas, se arrastran con el ratón y se pueden traer al frente con un atajo. La disposición se guarda en
//! `%APPDATA%\hypr-win\widgets.toml`. El dibujo está en `widget_draw`; los datos, en `sysinfo`, `weather`,
//! `media` y el archivo de la nota.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::Bar;
use crate::gfx::{px, Canvas, FRect, Theme};
use crate::icons::{self, Icon};
use crate::layout::Rect;
use crate::sysinfo::Sampler;
use crate::widget_draw::{self as wd, Data, Hit, Kind, Now, Size, WFonts};
use crate::workspace::monitor_at;
use crate::{media, weather};

pub const CLASS_NAME: &str = "HyprWinWidget";
/// Cuadrícula de ajuste al soltar un widget (px a 96 DPI).
const SNAP: f32 = 8.0;

/// Un widget colocado: tipo, tamaño y posición como fracción del monitor (esquina superior izquierda de la tarjeta).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub id: u32,
    pub kind: Kind,
    pub size: Size,
    /// Monitor (índice) de archivos antiguos; hoy manda `mon_id`.
    #[serde(default)]
    pub monitor: usize,
    /// Monitor al que pertenece (ver `window::MonitorInfo::id`); 0 = aún sin asignar.
    #[serde(default)]
    pub mon_id: u64,
    pub x: f32,
    pub y: f32,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub next_id: u32,
    /// Ciudad del widget del tiempo.
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub widgets: Vec<Placed>,
}

impl Default for Layout {
    fn default() -> Self {
        Self { visible: true, next_id: 1, city: String::new(), widgets: Vec::new() }
    }
}

// ---------- disposición (pura) ----------

/// Posición inicial en cascada para el `n`-ésimo widget.
pub fn default_position(n: usize) -> (f32, f32) {
    (0.05 + (n % 4) as f32 * 0.19, 0.12 + ((n / 4) % 3) as f32 * 0.3)
}

/// Fracción de monitor que corresponde a la esquina (px) de una tarjeta, ajustada a la cuadrícula y dentro del monitor.
pub fn to_fraction(card_x: i32, card_y: i32, card_w: i32, card_h: i32, mon: Rect, snap: i32) -> (f32, f32) {
    let snapped = |v: i32| ((v as f32 / snap as f32).round() as i32) * snap;
    let x = snapped(card_x - mon.x).clamp(0, (mon.w - card_w).max(0));
    let y = snapped(card_y - mon.y).clamp(0, (mon.h - card_h).max(0));
    (x as f32 / mon.w as f32, y as f32 / mon.h as f32)
}

/// Esquina (px) de la tarjeta para una posición fraccionaria, sin salirse del monitor.
pub fn from_fraction(x: f32, y: f32, card_w: i32, card_h: i32, mon: Rect) -> (i32, i32) {
    let px_ = mon.x + (x * mon.w as f32).round() as i32;
    let py_ = mon.y + (y * mon.h as f32).round() as i32;
    (px_.clamp(mon.x, (mon.x + mon.w - card_w).max(mon.x)), py_.clamp(mon.y, (mon.y + mon.h - card_h).max(mon.y)))
}

pub fn layout_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join(r"hypr-win\widgets.toml"))
}

fn note_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join(r"hypr-win\notes.txt"))
}

pub fn load_layout() -> Layout {
    layout_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| toml::from_str::<Layout>(&t).ok())
        .unwrap_or_default()
}

fn save_layout(l: &Layout) {
    let Some(p) = layout_path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = toml::to_string_pretty(l) {
        let _ = std::fs::write(p, text);
    }
}

fn local_now() -> Now {
    let t = unsafe { GetLocalTime() };
    Now {
        year: t.wYear as i32,
        month: t.wMonth as u32,
        day: t.wDay as u32,
        weekday: (t.wDayOfWeek as u32 + 6) % 7,
        hour: t.wHour as u32,
        min: t.wMinute as u32,
        sec: t.wSecond as u32,
    }
}

/// Firma del contenido que se dibuja: si no cambia entre dos ciclos, no se vuelve a pintar.
fn signature(kind: Kind, size: Size, data: &Data, now: &Now) -> u64 {
    let mut h = DefaultHasher::new();
    (kind, size).hash(&mut h);
    match kind {
        Kind::Clock => (now.hour, now.min, now.sec).hash(&mut h),
        Kind::Calendar => (now.year, now.month, now.day).hash(&mut h),
        Kind::System => {
            let s = &data.stats;
            (s.cpu as i32, s.mem_pct as i32, s.disk_free_gb as i32, (s.net_down / 1024.0) as u64, (s.net_up / 1024.0) as u64).hash(&mut h)
        }
        Kind::Weather => format!("{:?}{}", data.weather, data.city).hash(&mut h),
        Kind::Media => format!("{:?}", data.media).hash(&mut h),
        Kind::Notes => data.note.hash(&mut h),
        Kind::Shortcuts => data.shortcuts.hash(&mut h),
    }
    h.finish()
}

// ---------- estado y ventanas ----------

struct Win {
    placed: Placed,
    hwnd: HWND,
    canvas: Option<Canvas>,
    sig: u64,
    hits: Vec<(FRect, Hit)>,
    /// Esquina de la ventana en pantalla (con el margen de la sombra).
    origin: (i32, i32),
    /// Desplazamiento del cursor respecto a la ventana mientras se arrastra.
    drag: Option<(i32, i32)>,
}

struct State {
    theme: Theme,
    fonts: WFonts,
    layout: Layout,
    wins: Vec<Win>,
    data: Data,
    sampler: Sampler,
    /// Monitores conectados (id, rectángulo).
    mons: Vec<(u64, Rect)>,
    peek: bool,
    ticks: u32,
    thumb: Option<(PathBuf, Icon)>,
}

thread_local! {
    static S: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    S.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn noop() {}

/// Crea el sistema de widgets: carga la disposición y abre una ventana por cada widget guardado.
pub fn create(cfg: &Bar, mons: &[(u64, Rect)]) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = WFonts::new(&theme).ok_or("no se pudo crear la fuente de los widgets")?;
    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = HSTRING::from(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            hCursor: LoadCursorW(None, IDC_SIZEALL).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("RegisterClassExW (widgets) falló".into());
        }
    }
    let layout = load_layout();
    let mut data = Data { city: layout.city.clone(), ..Default::default() };
    data.note = note_path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    S.with(|s| {
        *s.borrow_mut() = Some(State {
            theme,
            fonts,
            layout,
            wins: Vec::new(),
            data,
            sampler: Sampler::default(),
            mons: mons.to_vec(),
            peek: false,
            ticks: 0,
            thumb: None,
        })
    });
    let _ = with(|s| {
        s.adopt_legacy();
        s.rebuild_windows();
        s.refresh_shortcuts();
        s.tick();
    });
    Ok(())
}

impl State {
    fn shown(&self) -> bool {
        self.layout.visible || self.peek
    }

    /// Rectángulo del monitor con ese id, si está conectado.
    fn mon_rect(&self, id: u64) -> Option<Rect> {
        self.mons.iter().find(|m| m.0 == id).map(|m| m.1)
    }

    /// Los widgets de archivos antiguos (solo con índice de monitor) pasan a identificar su monitor por id.
    fn adopt_legacy(&mut self) {
        let mut changed = false;
        for i in 0..self.layout.widgets.len() {
            if self.layout.widgets[i].mon_id == 0 {
                let idx = self.layout.widgets[i].monitor;
                if let Some(m) = self.mons.get(idx).or(self.mons.first()) {
                    self.layout.widgets[i].mon_id = m.0;
                    changed = true;
                }
            }
        }
        if changed {
            self.save();
        }
    }

    /// ¿Está conectado el monitor de este widget? Si no, el widget se queda donde está, oculto.
    fn connected(&self, p: &Placed) -> bool {
        self.mon_rect(p.mon_id).is_some()
    }

    fn card_px(size: Size) -> (i32, i32) {
        let (w, h) = size.dims();
        (px(w).round() as i32, px(h).round() as i32)
    }

    fn origin_for(&self, p: &Placed) -> Option<(i32, i32)> {
        let (cw, ch) = Self::card_px(p.size);
        let (x, y) = from_fraction(p.x, p.y, cw, ch, self.mon_rect(p.mon_id)?);
        let m = wd::margin() as i32;
        Some((x - m, y - m))
    }

    fn has(&self, kind: Kind) -> bool {
        self.wins.iter().any(|w| w.placed.kind == kind)
    }

    /// Crea las ventanas que falten y destruye las de widgets que ya no están.
    fn rebuild_windows(&mut self) {
        let wanted: Vec<Placed> = self.layout.widgets.clone();
        self.wins.retain(|w| {
            let keep = wanted.iter().any(|p| p.id == w.placed.id);
            if !keep {
                unsafe {
                    let _ = DestroyWindow(w.hwnd);
                }
            }
            keep
        });
        for p in wanted {
            if let Some(w) = self.wins.iter_mut().find(|w| w.placed.id == p.id) {
                w.placed = p;
                continue;
            }
            let Some(hwnd) = create_window() else { continue };
            let origin = self.origin_for(&p).unwrap_or_default();
            self.wins.push(Win { placed: p, hwnd, canvas: None, sig: 0, hits: Vec::new(), origin, drag: None });
        }
        self.apply_visibility();
    }

    fn apply_visibility(&mut self) {
        let show = self.shown();
        for w in &self.wins {
            let show = show && self.connected(&w.placed);
            unsafe {
                if show {
                    let _ = ShowWindow(w.hwnd, SW_SHOWNOACTIVATE);
                } else {
                    let _ = ShowWindow(w.hwnd, SW_HIDE);
                }
            }
        }
        self.restack();
    }

    /// Detrás de todas las ventanas (o por encima de todo mientras se "echa un vistazo").
    fn restack(&self) {
        let z = if self.peek { HWND_TOPMOST } else { HWND_BOTTOM };
        for w in &self.wins {
            unsafe {
                let _ = SetWindowPos(w.hwnd, Some(z), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    fn refresh_shortcuts(&mut self) {
        if !self.has(Kind::Shortcuts) {
            return;
        }
        self.data.shortcuts = crate::app::bindings_raw()
            .into_iter()
            .map(|(raw, b)| (raw.split(',').next().unwrap_or("").trim().to_string(), crate::keybinds::describe(&b.action)))
            .collect();
    }

    /// Un ciclo (cada segundo): lee datos, y vuelve a dibujar solo los widgets cuyo contenido cambió.
    fn tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        if self.wins.is_empty() || !self.shown() {
            return;
        }
        if self.wins.iter().all(|w| !self.connected(&w.placed)) {
            return;
        }
        if self.has(Kind::System) && self.ticks.is_multiple_of(2) {
            self.data.stats = self.sampler.sample();
        }
        if self.has(Kind::Weather) {
            weather::refresh(&self.layout.city, noop);
            self.data.weather = weather::latest();
        }
        if self.has(Kind::Media) {
            media::start(noop);
            self.data.media = media::latest();
            self.update_thumb();
        }
        if self.ticks % 10 == 1 {
            self.refresh_shortcuts();
        }
        let now = local_now();
        for wi in 0..self.wins.len() {
            if !self.connected(&self.wins[wi].placed) {
                continue;
            }
            let (kind, size) = (self.wins[wi].placed.kind, self.wins[wi].placed.size);
            let sig = signature(kind, size, &self.data, &now);
            if sig != self.wins[wi].sig || self.wins[wi].canvas.is_none() {
                self.wins[wi].sig = sig;
                self.paint(wi, &now);
            }
        }
        if self.ticks.is_multiple_of(5) && !self.peek {
            self.restack();
        }
    }

    fn update_thumb(&mut self) {
        let want = self.data.media.as_ref().and_then(|m| m.thumb.clone());
        if want.as_ref() == self.thumb.as_ref().map(|t| &t.0) {
            return;
        }
        self.thumb = want.and_then(|p| icons::from_image_file(&p.to_string_lossy()).map(|i| (p, i)));
    }

    fn paint(&mut self, wi: usize, now: &Now) {
        let State { theme, fonts, data, wins, thumb, .. } = self;
        let w = &mut wins[wi];
        let (cw, ch) = wd::canvas_size(w.placed.size);
        if w.canvas.as_ref().is_none_or(|c| c.w != cw || c.h != ch) {
            w.canvas = Canvas::new(cw, ch);
        }
        let Some(canvas) = w.canvas.as_mut() else { return };
        let icon = if w.placed.kind == Kind::Media { thumb.as_ref().map(|t| &t.1) } else { None };
        w.hits = wd::render(canvas, theme, fonts, w.placed.kind, w.placed.size, data, now, icon);
        canvas.present(w.hwnd, Some(w.origin));
    }

    fn repaint_all(&mut self) {
        let now = local_now();
        for wi in 0..self.wins.len() {
            self.wins[wi].sig = 0;
            if self.shown() && self.connected(&self.wins[wi].placed) {
                self.paint(wi, &now);
            }
        }
    }

    fn win_index(&self, hwnd: HWND) -> Option<usize> {
        self.wins.iter().position(|w| w.hwnd == hwnd)
    }

    fn save(&self) {
        save_layout(&self.layout);
    }
}

fn create_window() -> Option<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let class = HSTRING::from(CLASS_NAME);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(inst.into()),
            None,
        )
        .ok()
    }
}

// ---------- API ----------

/// Widgets colocados ahora.
pub fn list() -> Vec<Placed> {
    with(|s| s.layout.widgets.clone()).unwrap_or_default()
}

/// Añade un widget (en cascada) y devuelve su identificador.
pub fn add(kind: Kind, size: Size) -> Result<u32, String> {
    if !kind.sizes().contains(&size) {
        return Err(format!("{} no existe en tamaño {}", kind.label(), size.label()));
    }
    with(|s| {
        let id = s.layout.next_id.max(1);
        s.layout.next_id = id + 1;
        let (x, y) = default_position(s.layout.widgets.len());
        s.layout.widgets.push(Placed { id, kind, size, monitor: 0, mon_id: s.mons.first().map_or(0, |m| m.0), x, y });
        s.layout.visible = true;
        s.save();
        s.rebuild_windows();
        s.tick();
        id
    })
    .ok_or_else(|| "los widgets no están disponibles".to_string())
}

pub fn remove(id: u32) {
    let _ = with(|s| {
        s.layout.widgets.retain(|w| w.id != id);
        s.save();
        s.rebuild_windows();
    });
}

pub fn visible() -> bool {
    with(|s| s.layout.visible).unwrap_or(false)
}

pub fn set_visible(v: bool) {
    let _ = with(|s| {
        s.layout.visible = v;
        s.save();
        s.apply_visibility();
        if v {
            s.repaint_all();
        }
    });
}

/// Trae los widgets al frente (o los vuelve a dejar detrás), también si estaban ocultos.
pub fn toggle_peek() {
    let _ = with(|s| {
        s.peek = !s.peek;
        s.apply_visibility();
        if s.peek {
            s.repaint_all();
        }
    });
}

pub fn city() -> String {
    with(|s| s.layout.city.clone()).unwrap_or_default()
}

/// Cambia la ciudad del widget del tiempo y vuelve a pedirlo.
pub fn set_city(city: &str) {
    let _ = with(|s| {
        s.layout.city = city.trim().to_string();
        s.data.city = s.layout.city.clone();
        s.data.weather = None;
        s.save();
        s.tick();
    });
}

pub fn note() -> String {
    with(|s| s.data.note.clone()).unwrap_or_default()
}

pub fn set_note(text: &str) {
    if let Some(p) = note_path() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, text);
    }
    let _ = with(|s| {
        s.data.note = text.to_string();
        s.tick();
    });
}

/// Vuelve a poner todos los widgets en su posición inicial.
pub fn reset_positions() {
    let _ = with(|s| {
        for (i, w) in s.layout.widgets.iter_mut().enumerate() {
            let (x, y) = default_position(i);
            (w.x, w.y) = (x, y); // cada uno se queda en su monitor
        }
        s.save();
        let placed = s.layout.widgets.clone();
        for p in placed {
            let o = s.origin_for(&p);
            if let Some(w) = s.wins.iter_mut().find(|w| w.placed.id == p.id) {
                w.placed = p;
                if let Some(o) = o {
                    w.origin = o;
                }
            }
        }
        s.repaint_all();
    });
}

pub fn set_theme(cfg: &Bar) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = WFonts::new(&theme).ok_or("no se pudo crear la fuente de los widgets")?;
    let _ = with(|s| {
        s.theme = theme;
        s.fonts = fonts;
        s.repaint_all();
    });
    Ok(())
}

/// Monitores conectados (cambio de resolución, de disposición, o se conecta/desconecta uno). Cada widget
/// sigue en su monitor: si no está conectado se oculta sin moverlo, y reaparece donde estaba al volver.
pub fn set_monitors(mons: &[(u64, Rect)]) {
    let _ = with(|s| {
        s.mons = mons.to_vec();
        s.adopt_legacy();
        for wi in 0..s.wins.len() {
            let id = s.wins[wi].placed.id;
            let p = s.layout.widgets.iter().find(|p| p.id == id).cloned().unwrap_or_else(|| s.wins[wi].placed.clone());
            if let Some(o) = s.origin_for(&p) {
                s.wins[wi].origin = o;
            }
            s.wins[wi].placed = p;
        }
        s.apply_visibility();
        s.repaint_all();
    });
}

/// Ciclo de un segundo (lo llama el temporizador de la barra).
pub fn tick() {
    let _ = with(|s| s.tick());
}

// ---------- ratón: arrastrar y controles ----------

fn cursor() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => {
            let (x, y) = ((lp.0 & 0xffff) as i16 as f32, ((lp.0 >> 16) & 0xffff) as i16 as f32);
            let hit = with(|s| {
                let wi = s.win_index(hwnd)?;
                let w = &mut s.wins[wi];
                if let Some((_, h)) = w.hits.iter().find(|(r, _)| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h) {
                    return Some(*h);
                }
                let c = cursor();
                w.drag = Some((c.x - w.origin.0, c.y - w.origin.1));
                None
            })
            .flatten();
            match hit {
                Some(Hit::MediaPrev) => media::previous(),
                Some(Hit::MediaPlay) => media::toggle(),
                Some(Hit::MediaNext) => media::next(),
                None => {
                    SetCapture(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let _ = with(|s| {
                let Some(wi) = s.win_index(hwnd) else { return };
                let c = cursor();
                let w = &mut s.wins[wi];
                if let Some((dx, dy)) = w.drag {
                    w.origin = (c.x - dx, c.y - dy);
                    let _ = SetWindowPos(hwnd, None, w.origin.0, w.origin.1, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = with(|s| {
                let Some(wi) = s.win_index(hwnd) else { return };
                if s.wins[wi].drag.take().is_none() {
                    return;
                }
                let _ = ReleaseCapture();
                let (cw, ch) = State::card_px(s.wins[wi].placed.size);
                let m = wd::margin() as i32;
                let (cx, cy) = (s.wins[wi].origin.0 + m, s.wins[wi].origin.1 + m);
                let rects: Vec<Rect> = s.mons.iter().map(|m| m.1).collect();
                if rects.is_empty() {
                    return;
                }
                let mon = monitor_at(&rects, cx + cw / 2, cy + ch / 2);
                let (mon_id, mon_rect) = s.mons[mon];
                let (fx, fy) = to_fraction(cx, cy, cw, ch, mon_rect, px(SNAP).round() as i32);
                let id = s.wins[wi].placed.id;
                if let Some(p) = s.layout.widgets.iter_mut().find(|p| p.id == id) {
                    (p.monitor, p.mon_id, p.x, p.y) = (mon, mon_id, fx, fy);
                }
                let placed = s.layout.widgets.iter().find(|p| p.id == id).cloned();
                if let Some(p) = placed {
                    if let Some(o) = s.origin_for(&p) {
                        s.wins[wi].origin = o;
                    }
                    s.wins[wi].placed = p;
                    let o = s.wins[wi].origin;
                    let _ = SetWindowPos(hwnd, None, o.0, o.1, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
                s.save();
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MON: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

    #[test]
    fn fractions_round_trip_and_snap_to_the_grid() {
        let (fx, fy) = to_fraction(203, 157, 340, 160, MON, 8);
        let (x, y) = from_fraction(fx, fy, 340, 160, MON);
        assert_eq!((x % 8, y % 8), (0, 0), "no se ajustó a la cuadrícula: {x},{y}");
        assert!((x - 203).abs() <= 4 && (y - 157).abs() <= 4);
    }

    #[test]
    fn widgets_stay_inside_the_monitor() {
        // arrastrado fuera por la derecha y por abajo
        let (fx, fy) = to_fraction(5000, 5000, 340, 160, MON, 8);
        let (x, y) = from_fraction(fx, fy, 340, 160, MON);
        assert!(x + 340 <= MON.w && y + 160 <= MON.h, "{x},{y}");
        // y por la izquierda/arriba
        let (fx, fy) = to_fraction(-300, -300, 340, 160, MON, 8);
        assert_eq!(from_fraction(fx, fy, 340, 160, MON), (0, 0));
        // en un monitor desplazado
        let right = Rect::new(1920, 0, 1920, 1080);
        let (x, _) = from_fraction(0.5, 0.2, 340, 160, right);
        assert!((1920..=1920 + 1920 - 340).contains(&x));
    }

    #[test]
    fn default_positions_cascade_without_overlapping_exactly() {
        let pos: Vec<(f32, f32)> = (0..8).map(default_position).collect();
        for (i, a) in pos.iter().enumerate() {
            assert!(a.0 > 0.0 && a.0 < 0.9 && a.1 > 0.0 && a.1 < 0.9);
            assert!(pos.iter().skip(i + 1).all(|b| a != b));
        }
    }

    #[test]
    fn layout_round_trips_through_toml() {
        let mut l = Layout { city: "Madrid".into(), next_id: 3, ..Default::default() };
        l.widgets.push(Placed { id: 1, kind: Kind::Clock, size: Size::Small, monitor: 0, mon_id: 11, x: 0.1, y: 0.2 });
        l.widgets.push(Placed { id: 2, kind: Kind::Weather, size: Size::Medium, monitor: 1, mon_id: 22, x: 0.5, y: 0.25 });
        let text = toml::to_string_pretty(&l).unwrap();
        assert!(text.contains("kind = \"weather\"") && text.contains("size = \"medium\""), "{text}");
        let back: Layout = toml::from_str(&text).unwrap();
        assert_eq!(back.widgets, l.widgets);
        assert_eq!((back.city.as_str(), back.next_id, back.visible), ("Madrid", 3, true));
        // un archivo vacío da la disposición por defecto
        let empty: Layout = toml::from_str("").unwrap();
        assert!(empty.visible && empty.widgets.is_empty());
    }

    #[test]
    fn signature_changes_only_when_the_content_does() {
        let data = Data::default();
        let t = Now { year: 2026, month: 10, day: 29, weekday: 3, hour: 10, min: 0, sec: 0 };
        let a = signature(Kind::Clock, Size::Small, &data, &t);
        assert_eq!(a, signature(Kind::Clock, Size::Small, &data, &t));
        assert_ne!(a, signature(Kind::Clock, Size::Small, &data, &Now { sec: 1, ..t }));
        // el calendario no depende de los segundos
        let c = signature(Kind::Calendar, Size::Small, &data, &t);
        assert_eq!(c, signature(Kind::Calendar, Size::Small, &data, &Now { sec: 30, min: 5, ..t }));
        assert_ne!(c, signature(Kind::Calendar, Size::Small, &data, &Now { day: 30, ..t }));
    }
}
