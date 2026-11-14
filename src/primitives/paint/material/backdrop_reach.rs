//! [`BackdropReach`]: how far past its shape a backdrop effect reads.

use crate::primitives::paint::material::lens::Lens;

/// How far past its shape a backdrop effect reads, which the effect's source derives from its
/// parameters ([`EffectSource::reach`](crate::EffectSource::reach)), a
/// [`Material`](crate::Material)'s and an [`EffectPaint`](crate::widget::EffectPaint)'s alike: a
/// read past it reads its edge, and the capture grows by it, so damage under it repaints. The
/// effect's, as how far a ray bends is its shader's, not each paint's. Refused: a reach each paint
/// names by hand beside the source's, which two paints of one effect could set apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BackdropReach {
    /// A fixed reach in logical px.
    Px(f32),
    /// As far as `Lens` bends a ray at the shape's size, as the stock glass reads.
    Lens(Lens),
}

impl BackdropReach {
    /// Under the shape alone.
    pub const NONE: Self = Self::Px(0.0);
}
