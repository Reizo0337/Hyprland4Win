//! Layouts puros: reciben un rectángulo y un número de ventanas, devuelven rectángulos.
//! No tocan Win32, así que se pueden probar sin escritorio.

pub mod dwindle;
pub mod master;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    /// Reduce el rectángulo `m` px por cada lado (nunca por debajo de 1x1).
    pub fn shrink(&self, m: i32) -> Rect {
        let w = (self.w - 2 * m).max(1);
        let h = (self.h - 2 * m).max(1);
        Rect::new(self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h)
    }

    /// Escala el rectángulo alrededor de su centro (para el efecto pop-in).
    pub fn scaled(&self, f: f32) -> Rect {
        let w = ((self.w as f32 * f) as i32).max(1);
        let h = ((self.h as f32 * f) as i32).max(1);
        Rect::new(self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h)
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Gaps {
    pub inner: i32,
    pub outer: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    Dwindle,
    Master,
}

impl LayoutKind {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dwindle" => Ok(Self::Dwindle),
            "master" => Ok(Self::Master),
            other => Err(format!("layout desconocido: '{other}' (usa dwindle o master)")),
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::Dwindle => Self::Master,
            Self::Master => Self::Dwindle,
        }
    }
}

pub trait Layout {
    /// Devuelve `count` rectángulos dentro de `area`, en el orden de las ventanas.
    fn arrange(&self, area: Rect, count: usize, gaps: Gaps) -> Vec<Rect>;
}

pub fn arrange(kind: LayoutKind, area: Rect, count: usize, gaps: Gaps, master_ratio: f32) -> Vec<Rect> {
    match kind {
        LayoutKind::Dwindle => dwindle::Dwindle.arrange(area, count, gaps),
        LayoutKind::Master => master::Master { ratio: master_ratio }.arrange(area, count, gaps),
    }
}
