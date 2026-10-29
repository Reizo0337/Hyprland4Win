//! Dock al estilo de macOS: pastilla translúcida centrada abajo con los iconos reales de las apps
//! fijadas y de las que están abiertas, magnificación al pasar el ratón, punto bajo las apps abiertas,
//! rebote al lanzar y etiqueta con el nombre. Dibujo con GDI+ en una ventana con alfa por píxel.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::appid::AppIdentity;
use crate::apps::{self, Entry};
use crate::config::{Bar, Dock as DockCfg};
use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::icons::{self, Icon};
use crate::launcher::tile_color;
use crate::layout::Rect;
use crate::search;

pub const CLASS_NAME: &str = "HyprWinDock";
const TIMER_ANIM: usize = 1;
/// Cuántos iconos a cada lado del cursor se magnifican.
const SPREAD: f32 = 2.6;
const BOUNCE_SECS: f32 = 1.1;
/// WM_MOUSELEAVE (viene de TrackMouseEvent).
const WM_MOUSELEAVE: u32 = 0x02A3;

// ---------- lógica pura ----------

/// Posición y escala de un elemento del dock; `left` es relativo al centro del dock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub left: f32,
    pub width: f32,
    pub scale: f32,
    pub separator: bool,
}

/// Calcula la escala de cada icono según la distancia al cursor (curva de campana, como el dock de
/// macOS) y los coloca uno tras otro centrados. `cursor` es relativo al centro del dock; `amount`
/// (0-1) es cuánto está activa la magnificación.
pub fn compute_layout(separators: &[bool], size: f32, gap: f32, mag: f32, cursor: Option<f32>, amount: f32) -> Vec<Slot> {
    let base_w = |sep: bool| if sep { gap * 1.5 } else { size + gap };
    let total_base: f32 = separators.iter().map(|&s| base_w(s)).sum();
    let mut x = -total_base / 2.0;
    let mut slots = Vec::with_capacity(separators.len());
    let mut unscaled = Vec::with_capacity(separators.len());
    for &sep in separators {
        unscaled.push(x + base_w(sep) / 2.0);
        x += base_w(sep);
    }
    let mut left = 0.0;
    for (i, &sep) in separators.iter().enumerate() {
        let scale = match (cursor, sep) {
            (Some(cx), false) => {
                let d = ((cx - unscaled[i]).abs() / (SPREAD * (size + gap))).min(1.0);
                1.0 + (mag - 1.0) * amount * 0.5 * (1.0 + (std::f32::consts::PI * d).cos())
            }
            _ => 1.0,
        };
        let width = if sep { base_w(true) } else { size * scale + gap };
        slots.push(Slot { left, width, scale, separator: sep });
        left += width;
    }
    let shift = -left / 2.0;
    for s in &mut slots {
        s.left += shift;
    }
    slots
}

/// Desplazamiento vertical (px, hacia arriba) del rebote de lanzamiento tras `t` segundos.
pub fn bounce_offset(t: f32, size: f32) -> f32 {
    if !(0.0..BOUNCE_SECS).contains(&t) {
        return 0.0;
    }
    let k = t / BOUNCE_SECS;
    size * 0.5 * (std::f32::consts::PI * k * 3.0).sin().abs() * (1.0 - k)
}

/// Índice del slot (no separador) bajo la coordenada `x` relativa al centro.
pub fn slot_at(slots: &[Slot], x: f32) -> Option<usize> {
    slots.iter().position(|s| !s.separator && x >= s.left && x < s.left + s.width)
}

/// Medidas del dock en px reales.
pub struct Metrics {
    pub size: f32,
    pub gap: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub margin: f32,
    pub mag: f32,
}

impl Metrics {
    pub fn new(cfg: &DockCfg) -> Metrics {
        Metrics {
            size: px(cfg.icon_size as f32),
            gap: px(10.0),
            pad_x: px(12.0),
            pad_y: px(8.0),
            margin: px(8.0),
            mag: cfg.magnification,
        }
    }

    pub fn pill_h(&self) -> f32 {
        self.size + 2.0 * self.pad_y
    }

    /// Alto reservado en el área de trabajo (el dock sin magnificar y su margen).
    pub fn reserve(&self) -> f32 {
        self.margin + self.pill_h() + px(6.0)
    }

    /// Tamaño del lienzo para `icons` iconos y `seps` separadores (deja sitio a la magnificación).
    pub fn canvas_size(&self, icons: usize, seps: usize) -> (i32, i32) {
        let base = icons as f32 * (self.size + self.gap) + seps as f32 * self.gap * 1.5;
        let extra = (self.mag - 1.0) * self.size * SPREAD * 1.4;
        let w = base + extra + 2.0 * self.pad_x + px(80.0);
        let h = self.margin + self.pill_h() + (self.mag - 1.0) * self.size + self.size * 0.55 + px(44.0);
        (w.ceil() as i32, h.ceil() as i32)
    }
}

pub fn reserve_px(cfg: &DockCfg) -> i32 {
    Metrics::new(cfg).reserve().ceil() as i32
}

pub struct ItemView<'a> {
    pub name: &'a str,
    pub icon: Option<&'a Icon>,
    pub running: bool,
    /// Desplazamiento del rebote (px hacia arriba).
    pub bounce: f32,
    pub separator: bool,
}

pub struct Fonts {
    label: Font,
    letter: Font,
}

impl Fonts {
    pub fn new(th: &Theme) -> Option<Fonts> {
        let b = th.base_px();
        Some(Fonts { label: Font::new(&th.font, b * 0.95, false)?, letter: Font::new(&th.font, b * 1.8, true)? })
    }
}

/// Dibuja el dock. `cursor` es la x del ratón en el lienzo. Devuelve la lista de zonas (x, ancho, índice).
pub fn render(c: &mut Canvas, th: &Theme, f: &Fonts, m: &Metrics, items: &[ItemView], cursor: Option<f32>, amount: f32) -> Vec<(f32, f32, usize)> {
    c.clear();
    let seps: Vec<bool> = items.iter().map(|i| i.separator).collect();
    let center = c.w as f32 / 2.0;
    let slots = compute_layout(&seps, m.size, m.gap, m.mag, cursor.map(|x| x - center), amount);
    let total = slots.last().map_or(0.0, |s| s.left + s.width) - slots.first().map_or(0.0, |s| s.left);

    let pill_h = m.pill_h();
    let pill = FRect::new(center - total / 2.0 - m.pad_x, c.h as f32 - m.margin - pill_h, total + 2.0 * m.pad_x, pill_h);
    c.card(pill, px(22.0), th.bg, 0xB4, th.fg);

    let baseline = pill.y + pill_h - m.pad_y;
    let hovered = cursor.filter(|_| amount > 0.3).and_then(|x| slot_at(&slots, x - center));
    let mut zones = Vec::new();
    for (i, (slot, item)) in slots.iter().zip(items).enumerate() {
        let sx = center + slot.left;
        if slot.separator {
            c.fill_round(FRect::new(sx + slot.width / 2.0 - 0.5, pill.y + px(10.0), 1.0, pill_h - px(20.0)), 0.0, argb(th.fg, 50));
            continue;
        }
        let side = m.size * slot.scale;
        let rect = FRect::new(sx + m.gap / 2.0, baseline - side - item.bounce, side, side);
        match item.icon {
            Some(icon) => c.draw_icon(icon, rect),
            None => {
                c.fill_round(rect, side * 0.22, argb(tile_color(item.name), 255));
                let letter: String =
                    item.name.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
                c.text(&f.letter, &letter, rect, argb(0xFFFFFF, 255), Align::Center);
            }
        }
        if item.running {
            let d = px(4.5);
            c.fill_ellipse(FRect::new(sx + slot.width / 2.0 - d / 2.0, pill.y + pill_h - px(6.0) - d / 2.0, d, d), argb(th.fg, 200));
        }
        zones.push((sx, slot.width, i));
        if hovered == Some(i) {
            let w = c.measure(&f.label, item.name) + px(22.0);
            let tip = FRect::new(rect.x + side / 2.0 - w / 2.0, rect.y - px(38.0), w, px(28.0));
            c.card(tip, px(8.0), th.bg, 0xE8, th.fg);
            c.text(&f.label, item.name, tip, argb(th.fg, 255), Align::Center);
        }
    }
    zones
}

// ---------- estado y ventana ----------

/// Ventanas abiertas agrupadas por aplicación (las entrega la app).
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub ident: AppIdentity,
    pub hwnds: Vec<isize>,
}

struct Item {
    name: String,
    /// Identificador en `shell:AppsFolder`, si se conoce.
    id: Option<String>,
    exe: Option<String>,
    icon_key: String,
    hwnds: Vec<isize>,
    separator: bool,
    bounce: Option<Instant>,
}

/// Una ventana del dock (hay una por monitor); el contenido es compartido.
struct Win {
    hwnd: HWND,
    mon: Rect,
    canvas: Option<Canvas>,
    zones: Vec<(f32, f32, usize)>,
    cursor: Option<f32>,
    amount: f32,
    target: f32,
    tracking: bool,
    timer_on: bool,
}

struct Dock {
    theme: Theme,
    fonts: Fonts,
    metrics: Metrics,
    pinned_queries: Vec<String>,
    show_running: bool,
    items: Vec<Item>,
    icons: HashMap<String, Option<Icon>>,
    running: Vec<Running>,
    pinned_ready: bool,
    wins: Vec<Win>,
}

thread_local! {
    static D: RefCell<Option<Dock>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Dock) -> R) -> Option<R> {
    D.with(|d| d.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Crea un dock en cada monitor de `mons` (mismo contenido; magnificación independiente).
pub fn create(bar_cfg: &Bar, cfg: &DockCfg, mons: &[Rect]) -> Result<(), String> {
    let theme = Theme::from_cfg(bar_cfg)?;
    let fonts = Fonts::new(&theme).ok_or("no se pudo crear la fuente del dock")?;
    let mut wins = Vec::new();
    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = HSTRING::from(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("RegisterClassExW (dock) falló".into());
        }
        for &mon in mons {
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
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
            .map_err(|e| e.to_string())?;
            wins.push(Win {
                hwnd,
                mon,
                canvas: None,
                zones: Vec::new(),
                cursor: None,
                amount: 0.0,
                target: 0.0,
                tracking: false,
                timer_on: false,
            });
        }
    }
    D.with(|s| {
        *s.borrow_mut() = Some(Dock {
            theme,
            fonts,
            metrics: Metrics::new(cfg),
            pinned_queries: cfg.pinned.clone(),
            show_running: cfg.show_running,
            items: Vec::new(),
            icons: HashMap::new(),
            running: Vec::new(),
            pinned_ready: false,
            wins,
        })
    });
    let _ = with(|d| {
        d.rebuild();
        d.redraw();
        for w in &d.wins {
            unsafe {
                let _ = ShowWindow(w.hwnd, SW_SHOWNOACTIVATE);
            }
        }
    });
    Ok(())
}

/// Actualiza las apps abiertas; solo redibuja si cambió algo.
pub fn update(running: Vec<Running>) {
    let _ = with(|d| {
        if d.running != running {
            d.running = running;
            d.rebuild();
            d.redraw();
        }
    });
}

/// Reintenta resolver las apps fijadas cuando el índice de aplicaciones ya está listo.
pub fn refresh() {
    let _ = with(|d| {
        if !d.pinned_ready && !apps::entries().is_empty() {
            d.rebuild();
            d.redraw();
        }
    });
}

/// Reposiciona los docks (cambio de resolución o de disposición de los monitores).
pub fn set_monitors(mons: &[Rect]) {
    let _ = with(|d| {
        let mut changed = false;
        for (w, &m) in d.wins.iter_mut().zip(mons) {
            if w.mon != m {
                w.mon = m;
                changed = true;
            }
        }
        if changed {
            d.redraw();
        }
    });
}

impl Dock {
    fn pinned_entries(&self) -> Vec<Entry> {
        let all = apps::entries();
        let names: Vec<String> = all.iter().map(|e| e.name.clone()).collect();
        let mut out: Vec<Entry> = Vec::new();
        for q in &self.pinned_queries {
            if let Some(&i) = search::rank(q, &names, 1).first() {
                if !out.iter().any(|e| e.id == all[i].id) {
                    out.push(all[i].clone());
                }
            }
        }
        out
    }

    /// Reconstruye los elementos: fijadas (con sus ventanas) + separador + abiertas sin fijar.
    fn rebuild(&mut self) {
        let all = apps::entries();
        self.pinned_ready = !all.is_empty();
        let pinned = self.pinned_entries();
        let mut groups: Vec<(Running, bool)> = self.running.iter().cloned().map(|r| (r, false)).collect();
        let mut items: Vec<Item> = Vec::new();
        let old: HashMap<String, Option<Instant>> =
            self.items.iter().filter(|i| !i.separator).map(|i| (i.icon_key.clone(), i.bounce)).collect();

        for p in &pinned {
            let mut hwnds = Vec::new();
            let mut exe = None;
            for (g, used) in groups.iter_mut() {
                if g.ident.matches_entry(&p.id) {
                    hwnds.extend(&g.hwnds);
                    exe.get_or_insert(g.ident.exe.clone());
                    *used = true;
                }
            }
            items.push(Item {
                name: p.name.clone(),
                id: Some(p.id.clone()),
                exe,
                icon_key: p.id.clone(),
                hwnds,
                separator: false,
                bounce: old.get(&p.id).copied().flatten(),
            });
        }
        if self.show_running {
            let extra: Vec<&Running> = groups.iter().filter(|(_, used)| !used).map(|(g, _)| g).collect();
            if !extra.is_empty() && !items.is_empty() {
                items.push(Item { name: String::new(), id: None, exe: None, icon_key: String::new(), hwnds: vec![], separator: true, bounce: None });
            }
            for g in extra {
                let entry = all.iter().find(|e| g.ident.matches_entry(&e.id));
                let id = entry.map(|e| e.id.clone()).or_else(|| g.ident.aumid.clone());
                let key = id.clone().unwrap_or_else(|| g.ident.exe.clone());
                items.push(Item {
                    name: entry.map(|e| e.name.clone()).unwrap_or_else(|| g.ident.fallback_name()),
                    id,
                    exe: Some(g.ident.exe.clone()),
                    icon_key: key.clone(),
                    hwnds: g.hwnds.clone(),
                    separator: false,
                    bounce: old.get(&key).copied().flatten(),
                });
            }
        }
        // iconos que aún no están en la caché (se piden una sola vez)
        for it in items.iter().filter(|i| !i.separator) {
            if !self.icons.contains_key(&it.icon_key) {
                let icon = it
                    .id
                    .as_deref()
                    .and_then(icons::from_apps_folder)
                    .or_else(|| it.exe.as_deref().and_then(icons::from_path));
                self.icons.insert(it.icon_key.clone(), icon);
            }
        }
        self.items = items;
    }

    fn bouncing(&self) -> bool {
        self.items.iter().any(|i| i.bounce.is_some_and(|t| t.elapsed().as_secs_f32() < BOUNCE_SECS))
    }

    fn animating(&self, wi: usize) -> bool {
        let w = &self.wins[wi];
        (w.amount - w.target).abs() > 0.01 || self.bouncing()
    }

    fn redraw(&mut self) {
        for wi in 0..self.wins.len() {
            self.redraw_win(wi);
        }
    }

    fn redraw_win(&mut self, wi: usize) {
        let Dock { items, icons, theme, fonts, metrics, wins, .. } = self;
        let win = &mut wins[wi];
        let icons_n = items.iter().filter(|i| !i.separator).count();
        let (w, h) = metrics.canvas_size(icons_n, items.len() - icons_n);
        let w = w.min(win.mon.w);
        if win.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            win.canvas = Canvas::new(w, h);
        }
        let Some(canvas) = win.canvas.as_mut() else { return };
        let views: Vec<ItemView> = items
            .iter()
            .map(|it| ItemView {
                name: &it.name,
                icon: icons.get(&it.icon_key).and_then(|i| i.as_ref()),
                running: !it.hwnds.is_empty(),
                bounce: it.bounce.map_or(0.0, |t| bounce_offset(t.elapsed().as_secs_f32(), metrics.size)),
                separator: it.separator,
            })
            .collect();
        win.zones = render(canvas, theme, fonts, metrics, &views, win.cursor, win.amount);
        let x = win.mon.x + (win.mon.w - w) / 2;
        let y = win.mon.y + win.mon.h - h;
        canvas.present(win.hwnd, Some((x, y)));
    }

    fn ensure_timer(&mut self, wi: usize) {
        let w = &mut self.wins[wi];
        if !w.timer_on {
            w.timer_on = true;
            unsafe {
                SetTimer(Some(w.hwnd), TIMER_ANIM, 16, None);
            }
        }
    }

    fn tick(&mut self, wi: usize) {
        let w = &mut self.wins[wi];
        w.amount += (w.target - w.amount) * 0.3;
        if (w.amount - w.target).abs() <= 0.01 {
            w.amount = w.target;
        }
        self.redraw_win(wi);
        if !self.animating(wi) {
            let w = &mut self.wins[wi];
            w.timer_on = false;
            unsafe {
                let _ = KillTimer(Some(w.hwnd), TIMER_ANIM);
            }
        }
    }

    fn win_index(&self, hwnd: HWND) -> Option<usize> {
        self.wins.iter().position(|w| w.hwnd == hwnd)
    }

    fn item_at(&self, wi: usize, x: f32) -> Option<usize> {
        self.wins[wi].zones.iter().find(|(sx, w, _)| x >= *sx && x < sx + w).map(|(_, _, i)| *i)
    }
}

enum Click {
    Focus(Vec<isize>),
    Launch(Entry),
    None,
}

fn on_click(hwnd: HWND, x: f32) {
    let click = with(|d| {
        let Some(wi) = d.win_index(hwnd) else { return Click::None };
        let Some(i) = d.item_at(wi, x) else { return Click::None };
        let it = &mut d.items[i];
        if !it.hwnds.is_empty() {
            return Click::Focus(it.hwnds.clone());
        }
        match it.id.clone() {
            Some(id) => {
                it.bounce = Some(Instant::now());
                let entry = Entry { name: it.name.clone(), id };
                for wi in 0..d.wins.len() {
                    d.ensure_timer(wi); // el rebote se ve en todos los docks
                }
                Click::Launch(entry)
            }
            None => Click::None,
        }
    })
    .unwrap_or(Click::None);
    match click {
        Click::Focus(h) => crate::app::dock_activate(h),
        Click::Launch(e) => apps::launch(&e),
        Click::None => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let x = (lp.0 & 0xffff) as i16 as f32;
            let _ = with(|d| {
                let Some(wi) = d.win_index(hwnd) else { return };
                let w = &mut d.wins[wi];
                w.cursor = Some(x);
                w.target = 1.0;
                if !w.tracking {
                    w.tracking = true;
                    let mut t = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut t);
                }
                d.ensure_timer(wi);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let _ = with(|d| {
                let Some(wi) = d.win_index(hwnd) else { return };
                let w = &mut d.wins[wi];
                w.tracking = false;
                w.target = 0.0;
                d.ensure_timer(wi);
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            on_click(hwnd, (lp.0 & 0xffff) as i16 as f32);
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = with(|d| {
                if let Some(wi) = d.win_index(hwnd) {
                    d.tick(wi);
                }
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: f32 = 48.0;
    const G: f32 = 10.0;

    #[test]
    fn no_cursor_means_uniform_scale_and_centered() {
        let slots = compute_layout(&[false; 4], S, G, 1.7, None, 1.0);
        assert!(slots.iter().all(|s| s.scale == 1.0));
        let total: f32 = slots.iter().map(|s| s.width).sum();
        assert!((slots[0].left + total / 2.0).abs() < 1e-3);
    }

    #[test]
    fn magnification_peaks_at_cursor_and_decays() {
        let seps = [false; 7];
        let base = compute_layout(&seps, S, G, 1.7, None, 1.0);
        let mid = base[3].left + base[3].width / 2.0;
        let slots = compute_layout(&seps, S, G, 1.7, Some(mid), 1.0);
        assert!((slots[3].scale - 1.7).abs() < 1e-3);
        assert!(slots[2].scale < slots[3].scale && slots[1].scale < slots[2].scale);
        assert!((slots[2].scale - slots[4].scale).abs() < 1e-2); // simétrico
        assert!((slots[0].scale - 1.0).abs() < 1e-3 || slots[0].scale < 1.1);
    }

    #[test]
    fn amount_zero_disables_magnification() {
        let slots = compute_layout(&[false; 5], S, G, 2.0, Some(0.0), 0.0);
        assert!(slots.iter().all(|s| (s.scale - 1.0).abs() < 1e-6));
    }

    #[test]
    fn separators_do_not_scale_and_hit_test_skips_them() {
        let slots = compute_layout(&[false, true, false], S, G, 1.7, Some(0.0), 1.0);
        assert!(slots[1].separator && slots[1].scale == 1.0);
        let sep_mid = slots[1].left + slots[1].width / 2.0;
        assert_eq!(slot_at(&slots, sep_mid), None);
        assert_eq!(slot_at(&slots, slots[0].left + 1.0), Some(0));
    }

    #[test]
    fn bounce_decays_and_ends() {
        assert_eq!(bounce_offset(-0.1, S), 0.0);
        assert_eq!(bounce_offset(BOUNCE_SECS + 0.1, S), 0.0);
        let early = (0..40).map(|i| bounce_offset(i as f32 * 0.01, S)).fold(0.0f32, f32::max);
        let late = (0..40).map(|i| bounce_offset(0.7 + i as f32 * 0.01, S)).fold(0.0f32, f32::max);
        assert!(early > late && early <= S * 0.5);
    }
}
