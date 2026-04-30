/// WPF-style sizing. Maps to: Fixed = exact px, Hug = Auto (use desired), Fill = Star (take remainder).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sizing {
    Fixed(f32),
    Hug,
    Fill,
}

impl Default for Sizing {
    fn default() -> Self { Self::Hug }
}

impl From<f32> for Sizing {
    fn from(v: f32) -> Self { Sizing::Fixed(v) }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sizes {
    pub w: Sizing,
    pub h: Sizing,
}

impl Sizes {
    pub const HUG: Self = Self { w: Sizing::Hug, h: Sizing::Hug };
    pub const FILL: Self = Self { w: Sizing::Fill, h: Sizing::Fill };
    pub const fn new(w: Sizing, h: Sizing) -> Self { Self { w, h } }
}
