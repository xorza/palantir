//! [`EffectStore`]: the seam between a loaded effect and the pipelines that draw it.

use std::fmt::Debug;

use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::identity::effect_id::EffectId;

/// A validated effect module: the complete WGSL palantir compiles, its kind and how many images
/// each draw binds, which pick its pipeline layout. One stage draws every use: a backdrop effect's
/// also draws a [`Material`](crate::Material)'s box through palantir's material path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EffectModule {
    pub(crate) text: String,
    pub(crate) kind: KindId,
    pub(crate) image_slots: u8,
}

/// Where a loaded effect's pipelines are built. The backend implements it over wgpu and keeps
/// each effect's module, to build it where a use wants it, and again for a new swapchain format or
/// a new device; a deviceless recorder has no store, and builds nothing. `&self`, as the registry
/// and the backend share one store through an `Rc`.
pub(crate) trait EffectStore: Debug {
    /// Hold `module`, which the registry validated, as `id`'s, in place of the one before: a
    /// wanted effect builds the new one before its next frame, any other nothing yet.
    fn register(&self, id: EffectId, module: &EffectModule);

    /// Want `id`'s pipeline of a box, which a press and a material draw, built before the next
    /// frame draws, for every format and every device after; another form builds at the first
    /// frame that draws it.
    fn want(&self, id: EffectId);
}
