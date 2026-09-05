//! Texture ids shared by image registration and `GpuView` targets.

use crate::primitives::texture_id::TextureId;
use std::cell::Cell;
use std::rc::Rc;

/// The image draw path resolves registered images and `GpuView` targets by
/// the same id, so their separate stores must never contain colliding ids.
/// `UiResources` owns one source shared across its clones and used by both
/// registration paths. Zero is reserved for the render path's "no texture".
///
/// Ids are scoped to one host, so this needs neither a process-wide counter
/// nor atomics: all recorders sharing a host run on its UI thread.
#[derive(Clone, Debug, Default)]
pub(crate) struct TextureIdSource(Rc<Cell<u64>>);

impl TextureIdSource {
    pub(crate) fn reserve(&self) -> TextureId {
        let id = self.0.get() + 1;
        self.0.set(id);
        TextureId(id)
    }
}
