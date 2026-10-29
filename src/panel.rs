//! Panel de control (Wi-Fi, sonido, batería) al estilo del Centro de control de macOS. Se abre al hacer
//! clic en los iconos de estado de la barra. Dibujo con GDI+ (`gfx`) en una ventana con alfa por píxel.

use std::cell::RefCell;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::apps;
use crate::audio::{self, Volume};
use crate::bar;
use crate::config::Bar;
use crate::gfx::{argb, px, Align, Canvas, FRect, Font, Theme};
use crate::glyphs;
use crate::layout::Rect;
use crate::status::{lit_from_signal, Status};
use crate::wifi::{self, Auth, Network};
use crate::window;

pub const CLASS_NAME: &str = "HyprWinPanel";
const TIMER_REFRESH: usize = 1;
const MAX_NETWORKS: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    WifiToggle,
    Network(usize),
    Mute,
    Slider,
    Settings(&'static str),
    PasswordField,
    PasswordConnect,
    PasswordCancel,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Main,
    Password { ssid: String, input: String },
}

pub struct Fonts {
    title: Font,
    row: Font,
    small: Font,
    small_bold: Font,
}

impl Fonts {
    pub fn new(th: &Theme) -> Option<Fonts> {
        let b = th.base_px();
        Some(Fonts {
            title: Font::new(&th.font, b * 1.1, false)?,
            row: Font::new(&th.font, b, false)?,
            small: Font::new(&th.font, b * 0.85, false)?,
            small_bold: Font::new(&th.font, b * 0.8, true)?,
        })
    }
}

pub struct View<'a> {
    pub status: &'a Status,
    pub networks: &'a [Network],
    pub device: &'a str,
    pub mode: &'a Mode,
    pub message: &'a str,
    pub scanning: bool,
}

/// Bloques verticales del panel: la altura total y el dibujo salen de la misma lista.
#[derive(Clone, Copy, Debug)]
enum Block {
    WifiHeader,
    Networks(usize),
    Message,
    Divider,
    SoundTitle,
    SoundRow,
    Battery,
    Buttons,
    PasswordTitle,
    PasswordField,
    PasswordButtons,
}

impl Block {
    /// Altura en px a 96 DPI.
    fn height(self) -> f32 {
        match self {
            Block::WifiHeader => 56.0,
            Block::Networks(n) => n as f32 * 40.0,
            Block::Message => 28.0,
            Block::Divider => 17.0,
            Block::SoundTitle => 28.0,
            Block::SoundRow => 44.0,
            Block::Battery => 44.0,
            Block::Buttons => 44.0,
            Block::PasswordTitle => 34.0,
            Block::PasswordField => 60.0,
            Block::PasswordButtons => 44.0,
        }
    }
}

fn blocks(v: &View) -> Vec<Block> {
    let mut b = Vec::new();
    match v.mode {
        Mode::Main => {
            b.push(Block::WifiHeader);
            if v.status.wifi.present && v.status.wifi.enabled {
                b.push(Block::Networks(v.networks.len().clamp(1, MAX_NETWORKS)));
            }
            if !v.message.is_empty() {
                b.push(Block::Message);
            }
            b.extend([Block::Divider, Block::SoundTitle, Block::SoundRow]);
            if v.status.battery.is_some() {
                b.extend([Block::Divider, Block::Battery]);
            }
            b.extend([Block::Divider, Block::Buttons]);
        }
        Mode::Password { .. } => {
            b.extend([Block::PasswordTitle, Block::PasswordField]);
            if !v.message.is_empty() {
                b.push(Block::Message);
            }
            b.push(Block::PasswordButtons);
        }
    }
    b
}

struct Geo {
    margin: f32,
    card_w: f32,
    pad: f32,
}

impl Geo {
    fn new() -> Geo {
        Geo { margin: px(30.0), card_w: px(340.0), pad: px(16.0) }
    }
}

/// Tamaño del lienzo (con margen para la sombra) para esta vista.
pub fn canvas_size(v: &View) -> (i32, i32) {
    let g = Geo::new();
    let h: f32 = blocks(v).iter().map(|b| b.height()).sum::<f32>() * px(1.0) + 2.0 * g.pad;
    ((g.card_w + 2.0 * g.margin).ceil() as i32, (h + 2.0 * g.margin).ceil() as i32)
}

fn gray() -> u32 {
    0x66_6363 // #636366 como COLORREF
}

fn switch(c: &mut Canvas, r: FRect, on: bool, th: &Theme) {
    c.fill_round(r, r.h / 2.0, argb(if on { th.accent } else { gray() }, 255));
    let k = r.h - px(4.0);
    let kx = if on { r.x + r.w - k - px(2.0) } else { r.x + px(2.0) };
    c.fill_ellipse(FRect::new(kx, r.y + px(2.0), k, k), argb(0xFFFFFF, 255));
}

/// Dibuja el panel y devuelve las zonas pulsables.
pub fn render(c: &mut Canvas, th: &Theme, f: &Fonts, v: &View) -> Vec<(FRect, Hit)> {
    c.clear();
    let g = Geo::new();
    let list = blocks(v);
    let height: f32 = list.iter().map(|b| b.height()).sum::<f32>() * px(1.0) + 2.0 * g.pad;
    let card = FRect::new(g.margin, g.margin, g.card_w, height);
    c.card(card, px(18.0), th.bg, 0xF0, th.fg);

    let mut hits = Vec::new();
    let (left, right) = (card.x + g.pad, card.x + card.w - g.pad);
    let mut y = card.y + g.pad;
    for block in list {
        let bh = block.height() * px(1.0);
        match block {
            Block::WifiHeader => {
                let st = &v.status.wifi;
                let tile = FRect::new(left, y + px(10.0), px(36.0), px(36.0));
                let on = st.present && st.enabled;
                c.fill_ellipse(tile, argb(if on { th.accent } else { gray() }, 255));
                glyphs::wifi(c, tile.x + px(8.0), tile.y + px(8.0), px(20.0), if st.ssid.is_some() { lit_from_signal(st.signal) } else { 0 }, !on, 0xFFFFFF);
                let tx = tile.x + tile.w + px(12.0);
                c.text(&f.title, "Wi-Fi", FRect::new(tx, y + px(6.0), px(160.0), px(24.0)), argb(th.fg, 255), Align::Left);
                let sub = if !st.present {
                    "Sin adaptador".to_string()
                } else if !st.enabled {
                    "Desactivado".to_string()
                } else {
                    st.ssid.clone().unwrap_or_else(|| "No conectado".into())
                };
                c.text(&f.small, &sub, FRect::new(tx, y + px(28.0), px(170.0), px(18.0)), argb(th.dim, 255), Align::Left);
                if st.present {
                    let sw = FRect::new(right - px(46.0), y + (bh - px(26.0)) / 2.0, px(46.0), px(26.0));
                    switch(c, sw, st.enabled, th);
                    hits.push((FRect::new(sw.x - px(8.0), y, sw.w + px(8.0), bh), Hit::WifiToggle));
                    hits.push((tile, Hit::WifiToggle));
                }
            }
            Block::Networks(n) => {
                if v.networks.is_empty() {
                    let msg = if v.scanning { "Buscando redes…" } else { "No hay redes disponibles" };
                    c.text(&f.row, msg, FRect::new(left + px(30.0), y, right - left - px(30.0), px(40.0)), argb(th.dim, 255), Align::Left);
                }
                for (i, net) in v.networks.iter().take(n).enumerate() {
                    let ry = y + i as f32 * px(40.0);
                    glyphs::wifi(c, left + px(2.0), ry + px(12.0), px(16.0), lit_from_signal(net.signal), false, th.fg);
                    let name_w = right - left - px(30.0) - px(26.0);
                    c.text(&f.row, &net.ssid, FRect::new(left + px(30.0), ry, name_w, px(40.0)), argb(th.fg, 255), Align::Left);
                    if net.connected {
                        glyphs::check(c, right - px(18.0), ry + px(12.0), px(16.0), th.accent);
                    } else if net.secured {
                        glyphs::lock(c, right - px(15.0), ry + px(12.0), px(14.0), th.dim);
                    }
                    hits.push((FRect::new(card.x + px(6.0), ry, card.w - px(12.0), px(40.0)), Hit::Network(i)));
                }
            }
            Block::Message => {
                c.text(&f.small, v.message, FRect::new(left, y, right - left, bh), argb(th.dim, 255), Align::Left);
            }
            Block::Divider => {
                c.fill_round(FRect::new(left, y + px(8.0), right - left, 1.0), 0.0, argb(th.fg, 28));
            }
            Block::SoundTitle => {
                c.text(&f.small_bold, "Sonido", FRect::new(left, y, px(70.0), bh), argb(th.dim, 255), Align::Left);
                c.text(&f.small, v.device, FRect::new(left + px(60.0), y, right - left - px(60.0), bh), argb(th.dim, 255), Align::Right);
            }
            Block::SoundRow => {
                let vol = v.status.volume.unwrap_or(Volume { level: 0.0, muted: true });
                let icon = FRect::new(left, y + (bh - px(20.0)) / 2.0, px(20.0), px(20.0));
                glyphs::speaker(c, icon.x, icon.y, icon.w, vol.level, vol.muted, th.fg);
                hits.push((icon.grow(px(6.0)), Hit::Mute));
                let (tx0, tx1) = (left + px(36.0), right - px(44.0));
                let ty = y + bh / 2.0;
                let track = FRect::new(tx0, ty - px(3.0), tx1 - tx0, px(6.0));
                c.fill_round(track, px(3.0), argb(th.fg, 50));
                let fw = track.w * vol.level.clamp(0.0, 1.0);
                if fw > 0.0 {
                    c.fill_round(FRect::new(track.x, track.y, fw.max(px(6.0)), track.h), px(3.0), argb(th.accent, 255));
                }
                let k = px(20.0);
                let knob = FRect::new(track.x + fw - k / 2.0, ty - k / 2.0, k, k);
                c.fill_ellipse(knob.grow(px(0.5)), argb(0x000000, 40));
                c.fill_ellipse(knob, argb(0xFFFFFF, 255));
                hits.push((FRect::new(track.x - px(10.0), y, track.w + px(20.0), bh), Hit::Slider));
                c.text(&f.small, &format!("{}%", (vol.level * 100.0).round() as i32), FRect::new(right - px(40.0), y, px(40.0), bh), argb(th.dim, 255), Align::Right);
            }
            Block::Battery => {
                if let Some(b) = v.status.battery {
                    glyphs::battery(c, FRect::new(left, y + (bh - px(14.0)) / 2.0, px(30.0), px(14.0)), b.percent, b.charging, th.fg);
                    c.text(&f.row, "Batería", FRect::new(left + px(42.0), y, px(110.0), bh), argb(th.fg, 255), Align::Left);
                    let state = if b.charging {
                        "Cargando"
                    } else if b.ac {
                        "Conectada"
                    } else {
                        "Descargando"
                    };
                    c.text(&f.small, &format!("{} % · {state}", b.percent), FRect::new(left + px(120.0), y, right - left - px(120.0), bh), argb(th.dim, 255), Align::Right);
                }
            }
            Block::Buttons => {
                let gap = px(8.0);
                let w = (right - left - 2.0 * gap) / 3.0;
                for (i, (label, uri)) in [("Red", "ms-settings:network-wifi"), ("Sonido", "ms-settings:sound"), ("Bluetooth", "ms-settings:bluetooth")]
                    .into_iter()
                    .enumerate()
                {
                    let r = FRect::new(left + i as f32 * (w + gap), y + px(5.0), w, px(34.0));
                    c.fill_round(r, px(10.0), argb(th.fg, 28));
                    c.text(&f.small, label, r, argb(th.fg, 255), Align::Center);
                    hits.push((r, Hit::Settings(uri)));
                }
            }
            Block::PasswordTitle => {
                if let Mode::Password { ssid, .. } = v.mode {
                    c.text(&f.title, &format!("Contraseña de «{ssid}»"), FRect::new(left, y, right - left, px(30.0)), argb(th.fg, 255), Align::Left);
                }
            }
            Block::PasswordField => {
                if let Mode::Password { input, .. } = v.mode {
                    let r = FRect::new(left, y, right - left, px(44.0));
                    c.fill_round(r, px(10.0), argb(th.fg, 25));
                    c.stroke_round(r.grow(-0.75), px(10.0), 1.5, argb(th.accent, 255));
                    let dots: String = "•".repeat(input.chars().count());
                    let inner = FRect::new(r.x + px(12.0), r.y, r.w - px(24.0), r.h);
                    if input.is_empty() {
                        c.text(&f.row, "Contraseña", inner, argb(th.dim, 255), Align::Left);
                    } else {
                        c.text(&f.row, &dots, inner, argb(th.fg, 255), Align::Left);
                    }
                    let cx = inner.x + c.measure(&f.row, &dots).min(inner.w) + px(1.0);
                    c.fill_round(FRect::new(cx, r.y + px(11.0), px(2.0), px(22.0)), px(1.0), argb(th.accent, 255));
                    hits.push((r, Hit::PasswordField));
                }
            }
            Block::PasswordButtons => {
                let gap = px(8.0);
                let w = (right - left - gap) / 2.0;
                let cancel = FRect::new(left, y + px(5.0), w, px(34.0));
                let connect = FRect::new(left + w + gap, y + px(5.0), w, px(34.0));
                c.fill_round(cancel, px(10.0), argb(th.fg, 28));
                c.text(&f.small, "Cancelar", cancel, argb(th.fg, 255), Align::Center);
                let ready = matches!(v.mode, Mode::Password { input, .. } if input.chars().count() >= 8);
                c.fill_round(connect, px(10.0), argb(th.accent, if ready { 255 } else { 110 }));
                c.text(&f.small, "Conectar", connect, argb(th.accent_text, 255), Align::Center);
                hits.push((cancel, Hit::PasswordCancel));
                hits.push((connect, Hit::PasswordConnect));
            }
        }
        y += bh;
    }
    hits
}

struct Panel {
    hwnd: HWND,
    theme: Theme,
    fonts: Fonts,
    status: Status,
    networks: Vec<Network>,
    device: String,
    mode: Mode,
    message: String,
    scanning: bool,
    hits: Vec<(FRect, Hit)>,
    canvas: Option<Canvas>,
    /// Esquina superior derecha de la tarjeta (px de pantalla).
    anchor: (i32, i32),
    dragging: bool,
}

thread_local! {
    static P: RefCell<Option<Panel>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Panel) -> R) -> Option<R> {
    P.with(|p| p.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

pub fn create(cfg: &Bar) -> Result<(), String> {
    let theme = Theme::from_cfg(cfg)?;
    let fonts = Fonts::new(&theme).ok_or("no se pudo crear la fuente del panel")?;
    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = HSTRING::from(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("RegisterClassExW (panel) falló".into());
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
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
        P.with(|s| {
            *s.borrow_mut() = Some(Panel {
                hwnd,
                theme,
                fonts,
                status: Status::default(),
                networks: Vec::new(),
                device: String::new(),
                mode: Mode::Main,
                message: String::new(),
                scanning: false,
                hits: Vec::new(),
                canvas: None,
                anchor: (0, 0),
                dragging: false,
            })
        });
    }
    Ok(())
}

pub fn is_open() -> bool {
    with(|p| unsafe { IsWindowVisible(p.hwnd).as_bool() }).unwrap_or(false)
}

/// Abre el panel bajo la barra del monitor `mon` (esquina superior derecha), o lo cierra.
pub fn toggle(mon: Rect, bar_height: i32) {
    if is_open() {
        close();
        return;
    }
    wifi::scan();
    let Some(hwnd) = with(|p| {
        p.mode = Mode::Main;
        p.message.clear();
        p.scanning = true;
        p.anchor = (mon.right() - px(10.0) as i32, mon.y + bar_height + px(6.0) as i32);
        p.device = audio::device_name().unwrap_or_default();
        p.reload();
        p.redraw();
        p.hwnd
    }) else {
        return;
    };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        SetTimer(Some(hwnd), TIMER_REFRESH, 2500, None);
    }
    window::focus(window::id(hwnd));
}

pub fn close() {
    let Some(hwnd) = with(|p| p.hwnd) else { return };
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return;
        }
        let _ = KillTimer(Some(hwnd), TIMER_REFRESH);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    bar::refresh_status();
}

impl Panel {
    /// Vuelve a leer el estado y la lista de redes.
    fn reload(&mut self) {
        bar::refresh_status();
        self.status = bar::status();
        self.networks = wifi::networks();
    }

    fn redraw(&mut self) {
        let view = View {
            status: &self.status,
            networks: &self.networks,
            device: &self.device,
            mode: &self.mode,
            message: &self.message,
            scanning: self.scanning,
        };
        let (w, h) = canvas_size(&view);
        if self.canvas.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.canvas = Canvas::new(w, h);
        }
        let Some(canvas) = self.canvas.as_mut() else { return };
        self.hits = render(canvas, &self.theme, &self.fonts, &view);
        let g = Geo::new();
        // la tarjeta termina en el ancla (derecha) y empieza bajo la barra
        let x = self.anchor.0 - (g.card_w + g.margin) as i32;
        let y = self.anchor.1 - g.margin as i32;
        canvas.present(self.hwnd, Some((x, y)));
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h).map(|(_, h)| h.clone())
    }

    fn slider_rect(&self) -> Option<FRect> {
        self.hits.iter().find(|(_, h)| *h == Hit::Slider).map(|(r, _)| *r)
    }

    fn set_slider(&mut self, x: f32) {
        let Some(r) = self.slider_rect() else { return };
        let (x0, w) = (r.x + px(10.0), r.w - px(20.0));
        let level = ((x - x0) / w).clamp(0.0, 1.0);
        audio::set_level(level);
        if self.status.volume.is_some_and(|v| v.muted) && level > 0.0 {
            audio::set_muted(false);
        }
        self.status.volume = Some(Volume { level, muted: false });
    }
}

fn act(hit: Hit) {
    let _ = with(|p| match hit {
        Hit::WifiToggle => {
            let on = !p.status.wifi.enabled;
            p.message = match wifi::set_radio(on) {
                Ok(()) => String::new(),
                Err(e) => e,
            };
            p.scanning = on;
            std::thread::sleep(std::time::Duration::from_millis(300));
            p.reload();
        }
        Hit::Network(i) => {
            let Some(net) = p.networks.get(i).cloned() else { return };
            if net.connected {
                wifi::disconnect();
                p.message = "Desconectando…".into();
            } else if net.known || net.auth == Auth::Open {
                p.message = match wifi::connect(&net, None) {
                    Ok(()) => format!("Conectando a {}…", net.ssid),
                    Err(e) => e,
                };
            } else if matches!(net.auth, Auth::Psk | Auth::Sae) {
                p.mode = Mode::Password { ssid: net.ssid, input: String::new() };
                p.message.clear();
            } else {
                p.message = "Esta red (empresa/WEP) se configura en Windows".into();
            }
        }
        Hit::Mute => {
            let muted = p.status.volume.is_some_and(|v| v.muted);
            audio::set_muted(!muted);
            if let Some(v) = p.status.volume.as_mut() {
                v.muted = !muted;
            }
        }
        Hit::Slider | Hit::PasswordField => {}
        Hit::Settings(uri) => {
            close();
            apps::open_uri(uri);
        }
        Hit::PasswordCancel => {
            p.mode = Mode::Main;
            p.message.clear();
        }
        Hit::PasswordConnect => submit_password(p),
    });
    let _ = with(|p| p.redraw());
}

fn submit_password(p: &mut Panel) {
    let Mode::Password { ssid, input } = &p.mode else { return };
    if input.chars().count() < 8 {
        p.message = "La contraseña debe tener al menos 8 caracteres".into();
        return;
    }
    let (ssid, input) = (ssid.clone(), input.clone());
    let Some(net) = p.networks.iter().find(|n| n.ssid == ssid).cloned() else {
        p.mode = Mode::Main;
        p.message = "La red ya no está disponible".into();
        return;
    };
    match wifi::connect(&net, Some(&input)) {
        Ok(()) => {
            p.message = format!("Conectando a {ssid}…");
            p.mode = Mode::Main;
        }
        Err(e) => p.message = e,
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let xy = |lp: LPARAM| ((lp.0 & 0xffff) as i16 as f32, ((lp.0 >> 16) & 0xffff) as i16 as f32);
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_ACTIVATE => {
            if wp.0 & 0xffff == 0 {
                close();
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = with(|p| {
                p.scanning = false;
                p.reload();
                p.redraw();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lp);
            let hit = with(|p| p.hit_at(x, y)).flatten();
            match hit {
                Some(Hit::Slider) => {
                    let _ = with(|p| {
                        p.dragging = true;
                        p.set_slider(x);
                        p.redraw();
                    });
                    SetCapture(hwnd);
                }
                Some(h) => act(h),
                None => {}
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, _) = xy(lp);
            let _ = with(|p| {
                if p.dragging {
                    p.set_slider(x);
                    p.redraw();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if with(|p| std::mem::take(&mut p.dragging)).unwrap_or(false) {
                let _ = ReleaseCapture();
                bar::refresh_status();
            }
            LRESULT(0)
        }
        WM_CHAR => {
            let c = wp.0 as u32;
            let mut submit = false;
            let _ = with(|p| {
                if let Mode::Password { input, .. } = &mut p.mode {
                    match c {
                        0x08 => {
                            input.pop();
                        }
                        0x0D => submit = true,
                        c if c >= 0x20 => {
                            if let Some(ch) = char::from_u32(c) {
                                input.push(ch);
                            }
                        }
                        _ => {}
                    }
                    p.redraw();
                }
            });
            if submit {
                act(Hit::PasswordConnect);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wp.0 as u32 == 0x1B {
                let back = with(|p| std::mem::replace(&mut p.mode, Mode::Main) != Mode::Main).unwrap_or(false);
                if back {
                    let _ = with(|p| p.redraw());
                } else {
                    close();
                }
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
