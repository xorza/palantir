/// WPF-style sizing. Maps to: Fixed = exact px, Hug = Auto (use desired), Fill = Star (take remainder).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Sizing {
    Fixed(f32),
    #[default]
    Hug,
    Fill,
}

impl From<f32> for Sizing {
    fn from(v: f32) -> Self {
        Sizing::Fixed(v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sizes {
    pub w: Sizing,
    pub h: Sizing,
}

impl Sizes {
    pub const HUG: Self = Self {
        w: Sizing::Hug,
        h: Sizing::Hug,
    };
    pub const FILL: Self = Self {
        w: Sizing::Fill,
        h: Sizing::Fill,
    };
    pub const fn new(w: Sizing, h: Sizing) -> Self {
        Self { w, h }
    }
}

impl From<Sizing> for Sizes {
    fn from(s: Sizing) -> Self {
        Self { w: s, h: s }
    }
}

impl From<f32> for Sizes {
    fn from(v: f32) -> Self {
        Self {
            w: Sizing::Fixed(v),
            h: Sizing::Fixed(v),
        }
    }
}

impl<W: Into<Sizing>, H: Into<Sizing>> From<(W, H)> for Sizes {
    fn from((w, h): (W, H)) -> Self {
        Self {
            w: w.into(),
            h: h.into(),
        }
    }
}
