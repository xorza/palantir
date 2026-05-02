#[cfg(test)]
mod tests;

/// Axis-aligned rectangle in physical pixels (`u32`). Used for scissors,
/// glyph clip bounds, viewport extents — anywhere the renderer hands
/// integer pixel rects to the GPU. Logical-px rects use [`super::Rect`].
///
/// Stored as origin + size so it round-trips with wgpu's
/// `set_scissor_rect(x, y, w, h)` without arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct URect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl URect {
    pub const ZERO: Self = Self {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    };

    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn max_x(self) -> u32 {
        self.x + self.w
    }
    pub const fn max_y(self) -> u32 {
        self.y + self.h
    }
    pub const fn area(self) -> u32 {
        self.w * self.h
    }

    /// True if either dimension is zero — the rect paints/clips nothing.
    pub const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Strict axis-aligned intersection. Returns `None` when the inputs
    /// don't overlap (touching edges don't count). Used by the
    /// damage-rendering backend to combine the per-frame damage scissor
    /// with each group's existing clip scissor.
    pub const fn intersect(self, other: Self) -> Option<Self> {
        let x0 = if self.x > other.x { self.x } else { other.x };
        let y0 = if self.y > other.y { self.y } else { other.y };
        let a_max_x = self.x + self.w;
        let b_max_x = other.x + other.w;
        let x1 = if a_max_x < b_max_x { a_max_x } else { b_max_x };
        let a_max_y = self.y + self.h;
        let b_max_y = other.y + other.h;
        let y1 = if a_max_y < b_max_y { a_max_y } else { b_max_y };
        if x1 > x0 && y1 > y0 {
            Some(Self {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            })
        } else {
            None
        }
    }

    /// Saturating intersection: clamps `me` to fit inside `parent`,
    /// returning a (possibly zero-sized) rect. Used by the composer's
    /// clip stack where parent-child overlap is the common case and a
    /// zero-sized result is treated as "skip this group."
    pub const fn clamp_to(self, parent: Self) -> Self {
        let x0 = if self.x > parent.x { self.x } else { parent.x };
        let y0 = if self.y > parent.y { self.y } else { parent.y };
        let a_max_x = self.x + self.w;
        let b_max_x = parent.x + parent.w;
        let x1 = if a_max_x < b_max_x { a_max_x } else { b_max_x };
        let a_max_y = self.y + self.h;
        let b_max_y = parent.y + parent.h;
        let y1 = if a_max_y < b_max_y { a_max_y } else { b_max_y };
        Self {
            x: x0,
            y: y0,
            w: x1.saturating_sub(x0),
            h: y1.saturating_sub(y0),
        }
    }
}
