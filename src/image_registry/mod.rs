//! Registered image lifetimes shared by the host, its recorders and the backend.
//!
//! [`ImageHandle`](image_handle::ImageHandle) is the RAII owner of one registered image; the registry
//! is what every handle reaches. Texels go straight to the [`ImageStore`] the host builds the
//! registry over, the one its backend draws from (the trait carries the immediacy contract), so the
//! registry keeps no CPU copy and waits for no frame. A registry with no store is a deviceless
//! recorder: it keeps ids, sizes and generations and discards the texels.
//!
//! The pure data types live elsewhere —
//! [`Image`] / [`ImageFit`](crate::primitives::paint::image::ImageFit) in
//! `primitives`, [`TextureId`] and the counter it is minted from in
//! `primitives::identity::texture_id`, and the device ceiling a source is
//! measured against in
//! [`TextureLimit`](crate::renderer::texture_limit::TextureLimit) — so this
//! module owns only the lifecycle. `UiResources` mints the id and applies
//! the ceiling before a handle is built, so nothing here can fail.

pub(crate) mod image_handle;
pub(crate) mod image_store;

use crate::image_registry::image_store::ImageStore;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use std::rc::Rc;

/// Cheap to clone: every clone reaches the one store. `Default` is a deviceless recorder's.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImageRegistry {
    /// Fixed at construction, as [`TextureLimit`](crate::renderer::texture_limit::TextureLimit) is
    /// for its ceiling: the host builds the store before any recorder exists.
    store: Option<Rc<dyn ImageStore>>,
}

impl ImageRegistry {
    /// A registry whose texels go to `store`, the one the host's backend draws from.
    pub(crate) fn new<S: ImageStore + 'static>(store: Rc<S>) -> Self {
        Self { store: Some(store) }
    }

    fn write(&self, id: TextureId, image: &Image) {
        if let Some(store) = &self.store {
            store.write(id, image);
        }
    }

    fn free(&self, id: TextureId) {
        if let Some(store) = &self.store {
            store.free(id);
        }
    }
}

#[cfg(test)]
mod tests;
