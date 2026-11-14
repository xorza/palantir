//! [`EffectModule`]: a loaded effect as a backend compiles it.

use crate::primitives::effect::effect_kind::KindId;

/// A validated effect module: the complete WGSL palantir compiles, its kind and how many images
/// each draw binds, which pick its pipeline layout. One stage draws every use: a backdrop effect's
/// also draws a [`Material`](crate::Material)'s box through palantir's material path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EffectModule {
    pub(crate) text: String,
    pub(crate) kind: KindId,
    pub(crate) image_slots: u8,
}
