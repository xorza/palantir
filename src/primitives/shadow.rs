use crate::primitives::color::Color;
use glam::Vec2;
use palantir_anim_derive::Animatable;

/// Single drop-or-inset shadow. Used in two places: embedded in a
/// `Shape::Shadow` (paints via the shape buffer, multi-shadow stacks
/// allowed by record order) and as `Background::shadow` (paints via
/// the encoder's chrome branch, before the rect fill, single-shadow
/// only). Both routes share the `shadow_paint_rect_local` overhang
/// formula and the `draw_shadow` cmd path.
///
/// `Shadow::NONE` (also `Default`) is the "no shadow" sentinel —
/// matches the `Stroke::ZERO` convention so consumers can store a
/// plain `Shadow` field instead of `Option<Shadow>` and animate
/// componentwise through it.
///
/// `offset` shifts in logical px (CSS `box-shadow` x/y). `blur` is
/// the Gaussian σ in logical px (CSS `blur-radius / 2`); 0 collapses
/// to a sharp SDF. `spread` inflates (drop) or deflates (inset) the
/// source rect. `inset = true` paints inside the chrome boundary;
/// `false` paints outside it.
///
/// Multi-shadow stacks are intentionally not modelled here — drop a
/// `Shape::Shadow` directly when you need more than one.
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
