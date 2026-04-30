use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const ZERO: Self = Self { w: 0.0, h: 0.0 };
    pub const INF: Self = Self { w: f32::INFINITY, h: f32::INFINITY };

    pub const fn new(w: f32, h: f32) -> Self { Self { w, h } }

    pub fn min(self, other: Self) -> Self {
        Self { w: self.w.min(other.w), h: self.h.min(other.h) }
    }
    pub fn max(self, other: Self) -> Self {
        Self { w: self.w.max(other.w), h: self.h.max(other.h) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub min: Vec2,
    pub size: Size,
}

impl Rect {
    pub const ZERO: Self = Self { min: Vec2::ZERO, size: Size::ZERO };

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { min: Vec2::new(x, y), size: Size::new(w, h) }
    }

    pub fn max(&self) -> Vec2 { self.min + Vec2::new(self.size.w, self.size.h) }
    pub fn width(&self) -> f32 { self.size.w }
    pub fn height(&self) -> f32 { self.size.h }

    pub fn contains(&self, p: Vec2) -> bool {
        let mx = self.max();
        p.x >= self.min.x && p.y >= self.min.y && p.x < mx.x && p.y < mx.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    pub r: f32, pub g: f32, pub b: f32, pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
    pub const WHITE:       Self = Self { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK:       Self = Self { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self { Self { r, g, b, a } }
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self { Self { r, g, b, a: 1.0 } }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub width: f32,
    pub color: Color,
}

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
pub struct Spacing {
    pub left: f32, pub top: f32, pub right: f32, pub bottom: f32,
}

impl Spacing {
    pub const ZERO: Self = Self { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
    pub const fn all(v: f32) -> Self { Self { left: v, top: v, right: v, bottom: v } }
    pub fn horiz(&self) -> f32 { self.left + self.right }
    pub fn vert(&self) -> f32 { self.top + self.bottom }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Style {
    pub width:   Sizing,
    pub height:  Sizing,
    pub padding: Spacing,
    pub margin:  Spacing,
}
