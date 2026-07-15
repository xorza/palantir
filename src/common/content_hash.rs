/// Authoring fingerprint shared by tree rollups, shape/chrome records,
/// layout caches, text shaping, cascade, and damage.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ContentHash(pub(crate) u64);
