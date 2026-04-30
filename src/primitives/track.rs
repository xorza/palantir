use crate::primitives::Sizing;

/// One row or column definition for a `Grid`. Wraps a `Sizing` (Pixel / Auto /
/// Star) with optional `[min, max]` clamps. Defaults: `min = 0.0`,
/// `max = INFINITY` (no clamp).
///
/// `From<Sizing>` lets bare sizing values land in `.cols([Sizing::FILL, …])`
/// without the wrapper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub size: Sizing,
    pub min: f32,
    pub max: f32,
}

impl Track {
    pub const fn new(size: Sizing) -> Self {
        Self {
            size,
            min: 0.0,
            max: f32::INFINITY,
        }
    }

    pub const fn fixed(v: f32) -> Self {
        Self::new(Sizing::Fixed(v))
    }
    pub const fn hug() -> Self {
        Self::new(Sizing::Hug)
    }
    pub const fn fill() -> Self {
        Self::new(Sizing::FILL)
    }
    pub const fn fill_weight(w: f32) -> Self {
        Self::new(Sizing::Fill(w))
    }

    pub fn min(mut self, m: f32) -> Self {
        self.min = m;
        self
    }
    pub fn max(mut self, m: f32) -> Self {
        self.max = m;
        self
    }
}

impl From<Sizing> for Track {
    fn from(s: Sizing) -> Self {
        Self::new(s)
    }
}
