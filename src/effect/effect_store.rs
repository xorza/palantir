//! [`EffectStore`]: the seam between a loaded effect and the pipelines that draw it.

use std::fmt::Debug;

use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::identity::effect_id::EffectId;

/// A validated effect module: the complete WGSL palantir compiles, its kind and how many images
/// each draw binds, which pick its pipeline layout, and the stages it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EffectModule {
    pub(crate) text: String,
    pub(crate) kind: KindId,
    pub(crate) image_slots: u8,
    pub(crate) entries: ModuleEntries,
}

/// The stages a module holds: a shape's alone, or also a material's, for a backdrop effect over
/// [`MaterialInput`](crate::MaterialInput), which a [`Material`](crate::Material) names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModuleEntries {
    Shape,
    ShapeAndMaterial,
}

/// Where a loaded effect's pipelines are built. The backend implements it over wgpu and keeps
/// each effect's module, to build it where a use wants it, and again for a new swapchain format or
/// a new device; a deviceless recorder has no store, and builds nothing. `&self`, as the registry
/// and the backend share one store through an `Rc`.
pub(crate) trait EffectStore: Debug {
    /// Hold `module`, which the registry validated, as `id`'s, in place of the one before: a
    /// wanted effect builds the new one before its next frame, any other nothing yet.
    fn register(&self, id: EffectId, module: &EffectModule);

    /// Want `id` built before the next frame draws, for every format and every device after.
    fn want(&self, id: EffectId);
}
