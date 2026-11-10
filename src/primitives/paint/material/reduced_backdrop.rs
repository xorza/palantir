//! [`ReducedBackdrop`]: what a use of a backdrop paints while transparency is reduced, and the
//! [`BackdropMode`] it resolves to.

use crate::primitives::reduction::Reduction;

/// What a [`Material`](crate::Material) or an [`EffectPaint`](crate::widget::EffectPaint) of a
/// backdrop effect ([`EffectPaint::reduced_transparency`](crate::widget::EffectPaint::reduced_transparency))
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
    /// No effect: a material leaves its fill, translucent, over what is painted under it, and an
    /// `EffectPaint` of a backdrop effect paints nothing.
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

    /// What a use that paints this while transparency is reduced reads under `transparency`:
    /// what is painted under it while transparency runs whole, else the ground or, for
    /// [`Self::Nothing`], `None`, as it paints nothing. A material and a backdrop
    /// `EffectPaint` both resolve here.
    pub(crate) const fn mode(self, transparency: Reduction) -> Option<BackdropMode> {
        match (transparency, self) {
            (Reduction::Full, _) => Some(BackdropMode::Capture),
            (Reduction::Reduced, Self::Ground) => Some(BackdropMode::Ground),
            (Reduction::Reduced, Self::Nothing) => None,
        }
    }
}

/// What a use of a backdrop effect reads under it, as its [`ReducedBackdrop`] and the
/// transparency setting decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BackdropMode {
    /// What is painted under it, captured and blurred: a backdrop.
    Capture,
    /// The window's ground, its clear color, while transparency is reduced: no capture, no blur
    /// and no backdrop.
    Ground,
}

#[cfg(test)]
mod tests {
    use crate::primitives::paint::material::reduced_backdrop::{BackdropMode, ReducedBackdrop};
    use crate::primitives::reduction::Reduction;

    /// Whole transparency captures whatever the use chose; reduced, `Ground` reads the ground and
    /// `Nothing` paints nothing.
    #[test]
    fn a_use_reads_the_mode_its_choice_and_the_setting_give() {
        for (choice, transparency, mode) in [
            (
                ReducedBackdrop::Ground,
                Reduction::Full,
                Some(BackdropMode::Capture),
            ),
            (
                ReducedBackdrop::Nothing,
                Reduction::Full,
                Some(BackdropMode::Capture),
            ),
            (
                ReducedBackdrop::Ground,
                Reduction::Reduced,
                Some(BackdropMode::Ground),
            ),
            (ReducedBackdrop::Nothing, Reduction::Reduced, None),
        ] {
            assert_eq!(
                choice.mode(transparency),
                mode,
                "{choice:?} {transparency:?}"
            );
        }
    }
}
