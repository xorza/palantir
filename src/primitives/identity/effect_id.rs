//! [`EffectId`]: a loaded effect's identity in its host's registry.

/// A loaded effect's identity: its place in the host's [`EffectRegistry`], given at its first load
/// and kept for the app's life, as an effect never unloads. A shape record carries it in place of
/// the 63-byte name, and the backend keys its pipelines by it.
///
/// [`EffectRegistry`]: crate::effect::effect_registry::EffectRegistry
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EffectId(pub(crate) u32);

impl EffectId {
    pub(crate) const fn idx(self) -> usize {
        self.0 as usize
    }
}
