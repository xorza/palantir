//! [`TemplateCover`]: what of its shape template a closed draw covers.

/// What of the shape template (`shape_template.wgsl`) a draw covers, pinned as a constant in an
/// effect's pipeline beside its form: the template whole, or its rings and bands alone around a
/// shape it leaves undrawn, a halo's. A constant, as a form is: a halo's pipeline folds its outer
/// reach and its outside coverage in, and the rest read no flag. Refused: the halo as a bit of the
/// instance's flags, which every effect fragment then read, a varying more: +4.2 % of the
/// effect bench's glows on an Apple GPU (`atomic/effect_pass/gpu/rect`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TemplateCover {
    /// Every triangle: the shape and its edge.
    Whole = 0,
    /// The rings and bands alone, around a shape it leaves undrawn: a halo.
    Rim = 1,
}

impl TemplateCover {
    /// Every cover, in discriminant order.
    pub(crate) const ALL: [Self; 2] = [Self::Whole, Self::Rim];
}
