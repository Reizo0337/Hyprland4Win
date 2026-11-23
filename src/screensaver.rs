//! Salvapantallas propio: una escena animada (la misma que el fondo animado, o una ambiental con los
//! colores del tema si no hay ninguna configurada) con los widgets del escritorio encima, en una ventana
//! real por monitor. Aparece por inactividad o con un atajo (`Super+Shift+L` por defecto).
//!
//! **No gestiona contraseñas ni nada de la autenticación**: corre en tu sesión normal, como cualquier otra
//! ventana de hypr-win, sin privilegios especiales. Al salir (cualquier tecla o mover el ratón) se oculta y,
//! si `lock_on_wake` está activado, pide a Windows que bloquee la sesión de verdad (`LockWorkStation`, la
//! misma API que usa `Super+L`): la pantalla de contraseña que aparece después la sigue dibujando Windows por
//! completo. Así la parte sensible (comprobar la contraseña) nunca pasa por este código.

use std::cell::RefCell;
use std::time::Instant;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, SetFocus, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::Screensaver as Cfg;
use crate::gfx::{Canvas, Theme};
use crate::layout::Rect;
use crate::sysinfo::Sampler;
use crate::widget_draw::{self as wd, Data, Kind as WKind, Now, Size as WSize, WFonts};
use crate::{livewall, media, samurai, wallpaper, weather};

const CLASS_NAME: &str = "HyprWinScreensaver";
/// Fotogramas por segundo de la escena mientras está activo.
const FPS: u32 = 20;
/// Cada cuánto se comprueba la inactividad del sistema (ms); no necesita ser muy fino.
const IDLE_POLL_MS: u32 = 5000;
/// Movimiento mínimo del ratón (px) para contar como "ha despertado": evita que el propio
/// posicionamiento de la ventana bajo el cursor dispare un despertar inmediato y espurio.
const WAKE_MOUSE_THRESHOLD: i32 = 4;

struct Win {
    hwnd: HWND,
    rect: Rect,
    canvas: Canvas,
    small: Canvas,
}

struct State {
    cfg: Cfg,
    theme: Theme,
    fonts: WFonts,
    data: Data,
    sampler: Sampler,
    mons: Vec<(u64, Rect)>,
    wins: Vec<Win>,
    active: bool,
    started: Instant,
    ticks: u32,
    /// Posición del ratón al activarse (para el umbral de despertar).
    wake_origin: Option<(i32, i32)>,
}

thread_local! {
    static S: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    S.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn noop() {}

/// Arranca el módulo (una vez, al iniciar hypr-win): registra la clase y programa la comprobación de
/// inactividad. No crea ninguna ventana todavía.
pub fn create(cfg: &Cfg, theme: &Theme, mons: &[(u64, Rect)]) -> Result<(), String> {
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
            return Err("RegisterClassExW (screensaver) falló".into());
        }
    }
    let fonts = WFonts::new(theme).ok_or("no se pudo crear la fuente del salvapantallas")?;
    S.with(|s| {
        *s.borrow_mut() = Some(State {
            cfg: cfg.clone(),
            theme: theme.clone(),
            fonts,
            data: Data::default(),
            sampler: Sampler::default(),
            mons: mons.to_vec(),
            wins: Vec::new(),
            active: false,
            started: Instant::now(),
            ticks: 0,
            wake_origin: None,
        })
    });
    unsafe {
        SetTimer(Some(crate::bar::hwnd()), crate::bar::TIMER_SCREENSAVER_IDLE, IDLE_POLL_MS, None);
    }
    Ok(())
}

/// Cambia la configuración (idle_minutes, lock_on_wake, widgets) sin reiniciar hypr-win.
pub fn set_config(cfg: &Cfg) {
    let _ = with(|s| s.cfg = cfg.clone());
}

/// Cambia el tema en vivo (colores, fuente, iconos).
pub fn set_theme(theme: &Theme) -> Result<(), String> {
    let fonts = WFonts::new(theme).ok_or("no se pudo crear la fuente del salvapantallas")?;
    with(|s| {
        s.theme = theme.clone();
        s.fonts = fonts;
    });
    Ok(())
}

/// Monitores conectados: se aplicará la próxima vez que se active.
pub fn set_monitors(mons: &[(u64, Rect)]) {
    let _ = with(|s| s.mons = mons.to_vec());
}

/// ¿Está mostrándose ahora?
pub fn active() -> bool {
    with(|s| s.active).unwrap_or(false)
}

/// Al salir de hypr-win: oculta el salvapantallas si estaba activo (sin bloquear Windows).
pub fn shutdown() {
    if active() {
        wake(false);
    }
}

/// Milisegundos de inactividad del sistema (teclado y ratón), de cualquier aplicación.
fn idle_ms() -> u32 {
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    let now = unsafe { GetTickCount() };
    if unsafe { GetLastInputInfo(&mut li) }.as_bool() {
        now.wrapping_sub(li.dwTime)
    } else {
        0
    }
}

/// Llamado cada `IDLE_POLL_MS`: si toca, activa el salvapantallas por inactividad.
pub fn on_idle_tick() {
    let due = with(|s| !s.active && s.cfg.idle_minutes > 0 && idle_ms() >= s.cfg.idle_minutes * 60_000).unwrap_or(false);
    if due {
        show();
    }
}

/// Muestra el salvapantallas ahora (atajo o inactividad). Si ya está activo, no hace nada.
pub fn show() {
    let Some(already) = with(|s| s.active) else { return };
    if already {
        return;
    }
    refresh_data();
    let origin = cursor_pos();
    let ok = with(|s| {
        s.active = true;
        s.started = Instant::now();
        s.ticks = 0;
        s.wake_origin = origin;
        s.rebuild();
    })
    .is_some();
    if !ok {
        return;
    }
    unsafe {
        SetTimer(Some(crate::bar::hwnd()), crate::bar::TIMER_SCREENSAVER_TICK, 1000 / FPS, None);
    }
    let _ = with(|s| s.draw_all());
    if let Some(hwnd) = with(|s| s.wins.first().map(|w| w.hwnd)).flatten() {
        unsafe {
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(hwnd));
        }
    }
}

/// Oculta el salvapantallas; si `lock` y `lock_on_wake` están activados, bloquea Windows de verdad.
fn wake(lock: bool) {
    let lock_on_wake = with(|s| s.cfg.lock_on_wake).unwrap_or(true);
    let _ = with(|s| {
        s.active = false;
        for w in s.wins.drain(..) {
            unsafe {
                let _ = DestroyWindow(w.hwnd);
            }
        }
    });
    unsafe {
        let _ = KillTimer(Some(crate::bar::hwnd()), crate::bar::TIMER_SCREENSAVER_TICK);
    }
    if lock && lock_on_wake {
        crate::packages::run_hidden("rundll32.exe", "user32.dll,LockWorkStation");
    }
}

fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

fn refresh_data() {
    let _ = with(|s| {
        s.data.stats = s.sampler.sample();
        weather::refresh(&s.data.city, noop);
        s.data.weather = weather::latest();
        media::start(noop);
        s.data.media = media::latest();
    });
}

/// Ciudad a mostrar (la de los widgets de escritorio, si hay alguna configurada).
pub fn set_city(city: &str) {
    let _ = with(|s| s.data.city = city.to_string());
}

/// Un fotograma (lo llama el temporizador de la barra mientras está activo).
pub fn tick() {
    let _ = with(|s| {
        if !s.active {
            return;
        }
        s.ticks += 1;
        if s.ticks.is_multiple_of(FPS) {
            // una vez por segundo: estado del sistema (rápido) y, cada 30 s, tiempo y música
            s.data.stats = s.sampler.sample();
            if s.ticks.is_multiple_of(FPS * 30) {
                weather::refresh(&s.data.city, noop);
                s.data.weather = weather::latest();
            }
            media::start(noop);
            s.data.media = media::latest();
        }
        s.draw_all();
    });
}

impl State {
    fn rebuild(&mut self) {
        for w in self.wins.drain(..) {
            unsafe {
                let _ = DestroyWindow(w.hwnd);
            }
        }
        for (i, &(_, r)) in self.mons.iter().enumerate() {
            let Some(hwnd) = create_window(r, i == 0) else { continue };
            let (sw, sh) = samurai::scene_size(r.w, r.h);
            let (Some(canvas), Some(small)) = (Canvas::new(r.w, r.h), Canvas::new(sw, sh)) else {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                continue;
            };
            self.wins.push(Win { hwnd, rect: r, canvas, small });
            unsafe {
                let _ = ShowWindow(hwnd, if i == 0 { SW_SHOW } else { SW_SHOWNOACTIVATE });
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    /// Qué escena de fondo pintar: la que ya esté configurada como fondo animado, o una Aurora ambiental
    /// con los colores del tema si no hay ninguna (o es un GIF, que aquí no se reproduce).
    fn background(&self) -> BgKind {
        match livewall::current() {
            Some(livewall::Source::Procedural(k)) => BgKind::Procedural(k),
            Some(livewall::Source::Samurai) => BgKind::Samurai,
            _ => BgKind::Procedural(wallpaper::Kind::Aurora),
        }
    }

    fn draw_all(&mut self) {
        let t = self.started.elapsed().as_secs_f32();
        let (bg, accent) = (self.theme.bg, self.theme.accent);
        let kind = self.background();
        let widget_strip = self.cfg.widgets.then(|| self.compose_widgets());
        for w in &mut self.wins {
            let (sw, sh) = (w.small.w as usize, w.small.h as usize);
            match kind {
                BgKind::Procedural(k) => w.small.fill_from(&wallpaper::render_at(k, sw, sh, bg, accent, t)),
                BgKind::Samurai => w.small.fill_from(&samurai::render(sw, sh, t, bg, accent)),
            }
            w.canvas.draw_scaled(&w.small, kind != BgKind::Samurai);
            if let Some(strip) = &widget_strip {
                let x = (w.rect.w as f32 - strip.w as f32) / 2.0;
                let y = w.rect.h as f32 * 0.62 - strip.h as f32 / 2.0;
                w.canvas.draw_at(strip, x, y);
            }
            w.canvas.present(w.hwnd, Some((w.rect.x, w.rect.y)));
        }
    }

    /// Compone reloj, tiempo, música y sistema en un único lienzo ancho, centrado sobre la escena.
    /// Sin miniatura de carátula (no hace falta mantener la caché de iconos aquí).
    fn compose_widgets(&self) -> Canvas {
        let now = local_now();
        let (theme, fonts, data) = (&self.theme, &self.fonts, &self.data);
        let make = |kind: WKind, size: WSize| -> Option<Canvas> {
            let (w, h) = wd::canvas_size(size);
            let mut c = Canvas::new(w, h)?;
            let _ = wd::render(&mut c, theme, fonts, kind, size, data, &now, None);
            Some(c)
        };
        let mut parts = Vec::new();
        if let Some(c) = make(WKind::Clock, WSize::Medium) {
            parts.push(c);
        }
        if data.weather.is_some() {
            if let Some(c) = make(WKind::Weather, WSize::Medium) {
                parts.push(c);
            }
        }
        if data.media.as_ref().is_some_and(|m| m.playing) {
            if let Some(c) = make(WKind::Media, WSize::Medium) {
                parts.push(c);
            }
        }
        if let Some(c) = make(WKind::System, WSize::Medium) {
            parts.push(c);
        }
        let gap = 24;
        let w = parts.iter().map(|c| c.w).sum::<i32>() + gap * (parts.len() as i32 - 1).max(0);
        let h = parts.iter().map(|c| c.h).max().unwrap_or(1);
        let mut strip = Canvas::new(w.max(1), h.max(1)).unwrap_or_else(|| Canvas::new(1, 1).expect("lienzo 1x1"));
        let mut x = 0.0;
        for c in &parts {
            strip.draw_at(c, x, 0.0);
            x += c.w as f32 + gap as f32;
        }
        strip
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BgKind {
    Procedural(wallpaper::Kind),
    Samurai,
}

fn local_now() -> Now {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    Now { year: t.wYear as i32, month: t.wMonth as u32, day: t.wDay as u32, weekday: (t.wDayOfWeek as u32 + 6) % 7, hour: t.wHour as u32, min: t.wMinute as u32, sec: t.wSecond as u32 }
}

fn create_window(r: Rect, focusable: bool) -> Option<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let class = HSTRING::from(CLASS_NAME);
        let ex = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | if focusable { Default::default() } else { WS_EX_NOACTIVATE };
        CreateWindowExW(ex, PCWSTR(class.as_ptr()), PCWSTR::null(), WS_POPUP, r.x, r.y, r.w, r.h, None, None, Some(inst.into()), None).ok()
    }
}

/// ¿Supera el movimiento del ratón el umbral de despertar desde `origin`?
fn moved_enough(origin: (i32, i32), now: (i32, i32)) -> bool {
    (now.0 - origin.0).abs() > WAKE_MOUSE_THRESHOLD || (now.1 - origin.1).abs() > WAKE_MOUSE_THRESHOLD
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MOUSEWHEEL => {
            wake(true);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let now = cursor_pos().unwrap_or((0, 0));
            let woke = with(|s| match s.wake_origin {
                Some(origin) if moved_enough(origin, now) => true,
                None => true,
                _ => false,
            })
            .unwrap_or(false);
            if woke {
                wake(true);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_wake_needs_real_movement_not_a_single_spurious_event() {
        assert!(!moved_enough((500, 500), (501, 500)));
        assert!(!moved_enough((500, 500), (500, 503)));
        assert!(moved_enough((500, 500), (505, 500)));
        assert!(moved_enough((500, 500), (500, 600)));
        assert!(moved_enough((500, 500), (0, 0)));
    }

    /// A mano: `cargo test dump_preview -- --ignored` escribe `%TEMP%\screensaver_preview.png`.
    #[test]
    #[ignore]
    fn dump_preview() {
        let theme = Theme {
            bg: 0x0d0b0e,
            fg: 0xd6e4ec,
            dim: 0x7e6a76,
            accent: 0x2e10c8,
            accent_text: 0xecf6ff,
            font: "Segoe UI Variable Text".into(),
            font_pt: 10.0,
            icons: String::new(),
            app_icons: String::new(),
        };
        let fonts = WFonts::new(&theme).expect("fuentes");
        let mut data = Data { city: "Zaragoza".into(), ..Default::default() };
        data.weather = Some(wd::Weather { city: "Zaragoza".into(), temp: 12, code: 2, days: vec![] });
        data.media = Some(wd::Media { title: "Canción de prueba".into(), artist: "Alguien".into(), playing: true, thumb: None });
        let state = State { cfg: Cfg::default(), theme, fonts, data, sampler: Sampler::default(), mons: Vec::new(), wins: Vec::new(), active: true, started: Instant::now(), ticks: 0, wake_origin: None };
        let (w, h) = (1920, 1080);
        let (sw, sh) = samurai::scene_size(w, h);
        let mut small = Canvas::new(sw, sh).unwrap();
        small.fill_from(&samurai::render(sw as usize, sh as usize, 6.0, state.theme.bg, state.theme.accent));
        let mut big = Canvas::new(w, h).unwrap();
        big.draw_scaled(&small, false);
        let strip = state.compose_widgets();
        big.draw_at(&strip, (w as f32 - strip.w as f32) / 2.0, h as f32 * 0.62 - strip.h as f32 / 2.0);
        assert!(big.save_png(&std::env::temp_dir().join("screensaver_preview.png").to_string_lossy()));
    }

    #[test]
    fn idle_tick_only_fires_when_enabled_and_over_the_threshold() {
        // comprobación pura equivalente a la de `on_idle_tick`, sin estado global ni Win32
        let due = |idle_minutes: u32, active: bool, idle_ms: u32| !active && idle_minutes > 0 && idle_ms >= idle_minutes * 60_000;
        assert!(!due(0, false, 999_999_999), "0 = desactivado, nunca por inactividad");
        assert!(!due(5, true, 999_999_999), "ya está activo: no se vuelve a disparar");
        assert!(!due(5, false, 5 * 60_000 - 1));
        assert!(due(5, false, 5 * 60_000));
        assert!(due(5, false, 10 * 60_000));
    }
}
