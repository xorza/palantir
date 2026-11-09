//! [`ReducedBackdrop`]: what a use of a backdrop paints while transparency is reduced.

/// What a [`Material`](crate::Material) or a backdrop effect's shape
/// ([`EffectPaint::reduced_transparency`](crate::widget::EffectPaint::reduced_transparency))
/// paints while transparency is reduced
/// ([`Ui::is_transparency_reduced`](crate::Ui::is_transparency_reduced)). A choice of each use,
/// as the role decides it: a float's material reads the ground, and a scrim keeps its dim, as iOS
/// keeps a dim translucent under Reduce Transparency. Neither captures or blurs.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ReducedBackdrop {
    /// The effect over the window's ground, its clear color, in place of what is painted under
    /// it: a material is then opaque, as it shows over a bare window, the effect's grain and
    /// glint included.
    #[default]
    Ground,
    /// No effect: a material leaves its fill, translucent, over what is painted under it, and a
    /// backdrop effect's shape paints nothing.
    Nothing,
}

impl ReducedBackdrop {
    /// Whether this is [`Self::Ground`], the default a file leaves out. Takes `&self` because
    /// serde names it in a `skip_serializing_if`.
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `skip_serializing_if` hands the field by reference"
    )]
    pub(crate) const fn is_ground(&self) -> bool {
        matches!(self, Self::Ground)
    }
}
