//! Fondo animado: una ventana por monitor, detrás de todas las demás y transparente al ratón, que dibuja
//! una animación procedural con los colores del tema (degradado, resplandor, aurora) o un GIF animado.
//!
//! Con APIs documentadas no se puede colocar entre el fondo y los iconos del escritorio, así que mientras
//! está activo tapa los iconos. Se probó el truco de `Progman`/`WorkerW` (mensaje `0x052C`, ventanas hijas
//! del escritorio, en capas o no, y debajo de `Progman`): en Windows 11 26200 ninguna variante llega a
//! verse, por lo que no se usa.

use std::cell::RefCell;
use std::time::Instant;

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::gfx::{AnimatedImage, Canvas, FRect};
use crate::layout::Rect;
use crate::wallpaper::{self, Kind};

const CLASS_NAME: &str = "HyprWinLiveWallpaper";
/// Los fondos procedurales se calculan a esta fracción del tamaño del monitor y se amplían (son suaves).
const PROC_DIVISOR: i32 = 6;

/// Qué se está reproduciendo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Procedural(Kind),
    /// Escena nocturna con samurái (`samurai.rs`).
    Samurai,
    Gif(String),
}

impl Source {
    /// Valor de `[wallpaper] animated`: `gradient`, `glow`, `aurora` o la ruta de un `.gif`. Vacío = ninguno.
    pub fn parse(s: &str) -> Option<Source> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if s.eq_ignore_ascii_case("samurai") {
            return Some(Source::Samurai);
        }
        match Kind::parse(s) {
            Some(k) if wallpaper::is_animatable(k) => Some(Source::Procedural(k)),
            Some(_) => None,
            None => Some(Source::Gif(s.to_string())),
        }
    }

    pub fn key(&self) -> String {
        match self {
            Source::Procedural(k) => k.key().to_string(),
            Source::Samurai => "samurai".to_string(),
            Source::Gif(p) => p.clone(),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Source::Procedural(k) => format!("{} en movimiento", k.label()),
            Source::Samurai => "Samurái · noche".to_string(),
            Source::Gif(p) => std::path::Path::new(p).file_stem().map_or_else(|| p.clone(), |s| s.to_string_lossy().into_owned()),
        }
    }
}

/// Valida `[wallpaper] animated` y `animated_fps` (para `Config::validate`).
pub fn validate(animated: &str, fps: u32) -> Result<(), String> {
    if !(5..=60).contains(&fps) {
        return Err("wallpaper.animated_fps debe estar entre 5 y 60".into());
    }
    if let Some(Source::Gif(p)) = Source::parse(animated) {
        if !wallpaper::is_animated_file(std::path::Path::new(&p)) {
            return Err("wallpaper.animated debe ser gradient, glow, aurora o la ruta de un .gif".into());
        }
    }
    Ok(())
}

/// Instante de la animación (segundos) -> índice de fotograma y tiempo hasta el siguiente, dados los retardos.
pub fn frame_at(delays_ms: &[u32], elapsed_ms: u64) -> usize {
    let total: u64 = delays_ms.iter().map(|&d| d as u64).sum();
    if total == 0 || delays_ms.is_empty() {
        return 0;
    }
    let mut t = elapsed_ms % total;
    for (i, &d) in delays_ms.iter().enumerate() {
        if t < d as u64 {
            return i;
        }
        t -= d as u64;
    }
    delays_ms.len() - 1
}

/// Tamaño reducido en el que se calcula un fondo procedural.
pub fn proc_size(w: i32, h: i32) -> (i32, i32) {
    ((w / PROC_DIVISOR).max(16), (h / PROC_DIVISOR).max(9))
}

struct Win {
    hwnd: HWND,
    rect: Rect,
    canvas: Canvas,
    /// Solo en los fondos procedurales: el lienzo pequeño donde se calcula cada fotograma.
    small: Option<Canvas>,
}

impl Win {
    fn show(&self) {
        self.canvas.present(self.hwnd, Some((self.rect.x, self.rect.y)));
    }
}

struct State {
    source: Source,
    fps: u32,
    bg: u32,
    accent: u32,
    mons: Vec<(u64, Rect)>,
    wins: Vec<Win>,
    /// El GIF se abre una sola vez y se comparte entre monitores; el segundo valor es el último fotograma dibujado.
    gif: Option<(AnimatedImage, Option<usize>)>,
    started: Instant,
}

thread_local! {
    static S: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    S.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

/// Intervalo del temporizador (ms) para ese número de fotogramas por segundo.
pub fn interval_ms(fps: u32) -> u32 {
    1000 / fps.clamp(5, 60)
}

/// ¿Hay un fondo animado en marcha?
pub fn active() -> bool {
    with(|_| ()).is_some()
}

pub fn current() -> Option<Source> {
    with(|s| s.source.clone())
}

/// Arranca (o cambia) el fondo animado. `bg` y `accent` son COLORREF del tema.
pub fn start(source: Source, fps: u32, mons: &[(u64, Rect)], bg: u32, accent: u32) -> Result<(), String> {
    if let Source::Gif(p) = &source {
        if AnimatedImage::open(p).is_none() {
            return Err(format!("no se pudo abrir el GIF: {p}"));
        }
    }
    register_class()?;
    stop();
    S.with(|s| {
        *s.borrow_mut() = Some(State { source, fps, bg, accent, mons: mons.to_vec(), wins: Vec::new(), gif: None, started: Instant::now() })
    });
    let _ = with(|s| {
        s.rebuild();
        s.draw_all();
    });
    schedule();
    restack();
    Ok(())
}

/// Detiene la animación y cierra sus ventanas (el fondo normal de Windows vuelve a verse).
pub fn stop() {
    let old = S.with(|s| s.borrow_mut().take());
    if let Some(st) = old {
        for w in &st.wins {
            unsafe {
                let _ = DestroyWindow(w.hwnd);
            }
        }
    }
    unsafe {
        let _ = KillTimer(Some(crate::bar::hwnd()), crate::bar::TIMER_LIVEWALL);
    }
}

/// La escena samurái se mueve despacio: más de 20 fotogramas por segundo solo gastarían CPU.
const SAMURAI_MAX_FPS: u32 = 20;

fn schedule() {
    if let Some(fps) = with(|s| if s.source == Source::Samurai { s.fps.min(SAMURAI_MAX_FPS) } else { s.fps }) {
        unsafe {
            SetTimer(Some(crate::bar::hwnd()), crate::bar::TIMER_LIVEWALL, interval_ms(fps), None);
        }
    }
}

/// Cambia los fotogramas por segundo.
pub fn set_fps(fps: u32) {
    if with(|s| s.fps = fps.clamp(5, 60)).is_some() {
        schedule();
    }
}

/// Los colores del tema cambiaron (solo afecta a los fondos procedurales).
pub fn set_colors(bg: u32, accent: u32) {
    let _ = with(|s| {
        s.bg = bg;
        s.accent = accent;
    });
}

/// Monitores conectados (id, rectángulo): vuelve a crear las ventanas si cambiaron.
pub fn set_monitors(mons: &[(u64, Rect)]) {
    let _ = with(|s| {
        if s.mons != mons {
            s.mons = mons.to_vec();
            s.rebuild();
            s.draw_all();
        }
    });
    restack();
}

/// Un fotograma (lo llama el temporizador de la barra).
pub fn tick() {
    let _ = with(|s| s.draw_all());
}

/// Deja las ventanas del fondo detrás de todas las demás (los widgets lo llaman tras bajarse).
pub fn restack() {
    let _ = with(|s| {
        for w in s.wins.iter() {
            unsafe {
                let _ = SetWindowPos(w.hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    });
}

impl State {
    fn rebuild(&mut self) {
        for w in self.wins.drain(..) {
            unsafe {
                let _ = DestroyWindow(w.hwnd);
            }
        }
        self.gif = match &self.source {
            Source::Gif(path) => AnimatedImage::open(path).map(|g| (g, None)),
            Source::Procedural(_) | Source::Samurai => None,
        };
        if matches!(self.source, Source::Gif(_)) && self.gif.is_none() {
            return;
        }
        for &(_, r) in &self.mons {
            let Some(hwnd) = create_window(r) else { continue };
            let Some(canvas) = Canvas::new(r.w, r.h) else {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                continue;
            };
            let small = match self.source {
                Source::Procedural(_) | Source::Samurai => {
                    let (sw, sh) = if self.source == Source::Samurai { crate::samurai::scene_size(r.w, r.h) } else { proc_size(r.w, r.h) };
                    match Canvas::new(sw, sh) {
                        Some(c) => Some(c),
                        None => continue,
                    }
                }
                Source::Gif(_) => None,
            };
            self.wins.push(Win { hwnd, rect: r, canvas, small });
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
        }
    }

    fn draw_all(&mut self) {
        let elapsed = self.started.elapsed();
        let (t, ms) = (elapsed.as_secs_f32(), elapsed.as_millis() as u64);
        let (bg, accent) = (self.bg, self.accent);
        // GIF: mismo fotograma para todos los monitores; si no cambió no hay nada que redibujar
        let mut gif_changed = false;
        if let Some((img, shown)) = &mut self.gif {
            let i = frame_at(&img.delays_ms, ms);
            if *shown != Some(i) {
                *shown = Some(i);
                img.select(i);
                gif_changed = true;
            }
        }
        for w in &mut self.wins {
            if let Some(small) = w.small.as_mut() {
                let (sw, sh) = (small.w as usize, small.h as usize);
                match &self.source {
                    Source::Procedural(kind) => {
                        small.fill_from(&wallpaper::render_at(*kind, sw, sh, bg, accent, t));
                        w.canvas.draw_scaled(small, true);
                    }
                    _ => {
                        small.fill_from(&crate::samurai::render(sw, sh, t, bg, accent));
                        w.canvas.draw_scaled(small, false);
                    }
                }
            } else if let (true, Some((img, _))) = (gif_changed, &self.gif) {
                w.canvas.fill_round(FRect::new(0.0, 0.0, w.rect.w as f32, w.rect.h as f32), 0.0, 0xFF00_0000);
                w.canvas.draw_cover(img);
            } else {
                continue;
            }
            w.show();
        }
    }
}

fn register_class() -> Result<(), String> {
    unsafe {
        let inst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = HSTRING::from(CLASS_NAME);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // si ya estaba registrada (segundo arranque) RegisterClassExW falla y no pasa nada
        RegisterClassExW(&wc);
    }
    Ok(())
}

fn create_window(r: Rect) -> Option<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let class = HSTRING::from(CLASS_NAME);
        let ex = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        CreateWindowExW(ex, PCWSTR(class.as_ptr()), PCWSTR::null(), WS_POPUP, r.x, r.y, r.w, r.h, None, None, Some(inst.into()), None).ok()
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_parse_and_round_trip() {
        assert_eq!(Source::parse(""), None);
        assert_eq!(Source::parse("  Aurora "), Some(Source::Procedural(Kind::Aurora)));
        assert_eq!(Source::parse("solid"), None); // el color liso no se mueve
        assert_eq!(Source::parse("Samurai"), Some(Source::Samurai));
        assert_eq!(Source::parse("samurai").unwrap().key(), "samurai");
        let gif = Source::parse(r"C:\Fondos\lluvia.gif").unwrap();
        assert_eq!(gif, Source::Gif(r"C:\Fondos\lluvia.gif".into()));
        assert_eq!(Source::parse(&gif.key()), Some(gif.clone()));
        assert_eq!(gif.label(), "lluvia");
    }

    #[test]
    fn validation() {
        assert!(validate("", 30).is_ok());
        assert!(validate("glow", 5).is_ok() && validate("glow", 60).is_ok());
        assert!(validate("glow", 4).is_err() && validate("glow", 61).is_err());
        assert!(validate(r"C:\x\video.mp4", 30).is_err());
        assert!(validate(r"C:\x\a.GIF", 30).is_ok());
    }

    #[test]
    fn frame_selection_loops() {
        let d = [100, 200, 100];
        assert_eq!(frame_at(&d, 0), 0);
        assert_eq!(frame_at(&d, 99), 0);
        assert_eq!(frame_at(&d, 100), 1);
        assert_eq!(frame_at(&d, 299), 1);
        assert_eq!(frame_at(&d, 300), 2);
        assert_eq!(frame_at(&d, 400), 0); // da la vuelta
        assert_eq!(frame_at(&d, 4_000_100), 1);
        assert_eq!(frame_at(&[], 5), 0);
    }

    #[test]
    fn intervals_and_sizes() {
        assert_eq!(interval_ms(30), 33);
        assert_eq!(interval_ms(0), 200); // se limita a 5 fps
        assert_eq!(interval_ms(500), 16);
        assert_eq!(proc_size(1920, 1080), (320, 180));
        assert_eq!(proc_size(10, 10), (16, 9));
    }

    #[test]
    fn animated_frames_start_at_the_static_wallpaper_and_then_move() {
        let (bg, ac) = (0x201e1e, 0xff840a);
        for k in [Kind::Gradient, Kind::Glow, Kind::Aurora] {
            let still = wallpaper::render(k, 48, 27, bg, ac);
            assert_eq!(wallpaper::render_at(k, 48, 27, bg, ac, 0.0), still, "{k:?} en t=0");
            assert_ne!(wallpaper::render_at(k, 48, 27, bg, ac, 4.0), still, "{k:?} debería moverse");
        }
    }
}
