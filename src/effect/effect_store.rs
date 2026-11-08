//! [`EffectStore`]: the seam between a loaded effect and the pipelines that draw it.

use std::fmt::Debug;

use crate::primitives::identity::effect_id::EffectId;

/// Where a loaded effect's pipelines are built. The backend implements it over wgpu and keeps
/// each effect's text, to build it again for a new swapchain format or a new device; a deviceless
/// recorder has no store, and builds nothing. `&self`, as the registry and the backend share one
/// store through an `Rc`.
pub(crate) trait EffectStore: Debug {
    /// Build `id`'s pipelines from `wgsl`, a module the registry validated, for every format the
    /// device draws to now, with their rounded-clip twins, replacing the ones built before:
    /// at load, so no draw waits for a compile.
    fn load(&self, id: EffectId, wgsl: &str);
}
