//! [`EffectStore`]: the seam between a loaded effect and the pipelines that draw it.

use std::fmt::Debug;

use crate::effect::effect_kind::KindId;
use crate::primitives::identity::effect_id::EffectId;

/// A validated effect module: the complete WGSL palantir compiles, its kind and how many images
/// each draw binds, which pick its pipeline layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EffectModule {
    pub(crate) text: String,
    pub(crate) kind: KindId,
    pub(crate) image_slots: u8,
}

/// Where a loaded effect's pipelines are built. The backend implements it over wgpu and keeps
/// each effect's text, to build it again for a new swapchain format or a new device; a deviceless
/// recorder has no store, and builds nothing. `&self`, as the registry and the backend share one
/// store through an `Rc`.
pub(crate) trait EffectStore: Debug {
    /// Build `id`'s pipelines from `module`, which the registry validated, for every format the
    /// device draws to now, with their rounded-clip twins, replacing the ones built before:
    /// at load, so no draw waits for a compile.
    fn load(&self, id: EffectId, module: &EffectModule);
}
