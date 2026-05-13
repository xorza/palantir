use crate::primitives::approx::noop_f32;
use crate::primitives::color::Color;
use glam::Vec2;
use palantir_anim_derive::Animatable;

/// Single drop-or-inset shadow attached to a [`Background`]. Lowered
/// at `open_node` into a [`ShapeRecord::Shadow`] sitting at the head
/// of the owning node's shape span so existing damage / paint-rect /
/// overhang plumbing covers it without a parallel code path.
///
/// `offset` shifts in logical px (CSS `box-shadow` x/y). `blur` is
/// the Gaussian σ in logical px (CSS `blur-radius / 2`); 0 collapses
/// to a sharp SDF. `spread` inflates (drop) or deflates (inset) the
/// source rect. `inset = true` paints inside the chrome boundary;
/// `false` paints outside it.
///
/// Multi-shadow stacks are intentionally not modelled here — drop a
/// `Shape::Shadow` directly when you need more than one. Folding a
/// `SmallVec<[Shadow; N]>` onto `Background` would cost the
/// `Copy`/`Hash`/`SparseColumn` properties for a niche case.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize, Animatable,
)]
pub struct Shadow {
    pub color: Color,
    pub offset: Vec2,
    pub blur: f32,
    pub spread: f32,
    #[animate(snap)]
    pub inset: bool,
}

impl Shadow {
    /// Canonical "no shadow" sentinel. Equivalent to
    /// `Shadow::default()` but `const`, so callers can use it in
    /// `const` contexts (theme tables, look defaults). Reports
    /// `is_noop()` — emits nothing.
    pub const NONE: Self = Self {
        color: Color::TRANSPARENT,
        offset: Vec2::ZERO,
        blur: 0.0,
        spread: 0.0,
        inset: false,
    };

    pub fn is_noop(&self) -> bool {
        self.color.is_noop()
            || (noop_f32(self.blur)
                && noop_f32(self.spread)
                && noop_f32(self.offset.x)
                && noop_f32(self.offset.y)
                && self.color.a <= 0.0)
    }
}

impl std::hash::Hash for Shadow {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.color.hash(state);
        state.write_u32(self.offset.x.to_bits());
        state.write_u32(self.offset.y.to_bits());
        state.write_u32(self.blur.to_bits());
        state.write_u32(self.spread.to_bits());
        state.write_u8(self.inset as u8);
    }
}
