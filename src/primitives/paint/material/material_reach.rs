//! [`MaterialReach`]: how far past its box a material's effect reads.

use crate::primitives::paint::material::lens::Lens;

/// How far past its box a [`Material`](crate::Material)'s effect reads, which its effect's source
/// derives from the material's parameters
/// ([`EffectSource::reach`](crate::EffectSource::reach)): a read past it reads its edge, and the
/// capture grows by it, so damage under it repaints. The effect's, as how far a ray bends is its
/// shader's, not each material's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaterialReach {
    /// A fixed reach in logical px, as
    /// [`EffectPaint::max_sample_offset`](crate::widget::EffectPaint::max_sample_offset) names
    /// one.
    Px(f32),
    /// As far as `Lens` bends a ray at the box's size, as the stock glass reads.
    Lens(Lens),
}

impl MaterialReach {
    /// Under the box alone.
    pub const NONE: Self = Self::Px(0.0);
}
