use super::num::Num;

#[repr(C)]
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Default,
    bytemuck::Pod,
    bytemuck::Zeroable,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct Spacing {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl std::hash::Hash for Spacing {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl Spacing {
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };
    pub const fn all(v: f32) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }
    pub const fn xy(x: f32, y: f32) -> Self {
        Self {
            left: x,
            top: y,
            right: x,
            bottom: y,
        }
    }
    pub const fn horiz(&self) -> f32 {
        self.left + self.right
    }
    pub const fn vert(&self) -> f32 {
        self.top + self.bottom
    }
}

impl<T: Num> From<T> for Spacing {
    fn from(v: T) -> Self {
        Self::all(v.as_f32())
    }
}

/// `(horizontal, vertical)` — both sides on each axis.
impl<X: Num, Y: Num> From<(X, Y)> for Spacing {
    fn from((x, y): (X, Y)) -> Self {
        Self::xy(x.as_f32(), y.as_f32())
    }
}

/// `(left, top, right, bottom)` — matches struct field order.
impl<L: Num, T: Num, R: Num, B: Num> From<(L, T, R, B)> for Spacing {
    fn from((l, t, r, b): (L, T, R, B)) -> Self {
        Self {
            left: l.as_f32(),
            top: t.as_f32(),
            right: r.as_f32(),
            bottom: b.as_f32(),
        }
    }
}
