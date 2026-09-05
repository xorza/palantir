//! Registered image lifetimes shared by the host, its recorders, and the
//! backend.
//!
//! [`ImageHandle`](image_handle::ImageHandle) is the RAII owner of one
//! registered image, and the registry is what every handle reaches. The
//! texels go straight to an [`ImageStore`] the backend implements — the
//! trait carries the immediacy contract — so the registry keeps no CPU
//! copy and waits for no frame. A registry built without a store is a
//! deviceless recorder: it keeps ids, sizes and generations and discards
//! the texels.
//!
//! The pure data types live elsewhere —
//! [`Image`] / [`ImageFit`](crate::primitives::image::ImageFit) in
//! `primitives`, [`TextureId`] in `primitives::texture_id` and its source
//! in `renderer::texture_id_source`, and the device ceiling a source is
//! measured against in
//! [`TextureLimit`](crate::renderer::texture_limit::TextureLimit) — so this
//! module owns only the lifecycle. `UiResources` mints the id and applies
//! the ceiling before a handle is built, so nothing here can fail.

pub(crate) mod image_handle;
pub(crate) mod image_store;

use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::image_registry::image_store::ImageStore;
use std::rc::Rc;

/// Cheap to clone: every clone reaches the one store.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImageRegistry {
    /// `None` is a standalone CPU recorder — no device, so no texture to
    /// fill — the same convention
    /// [`TextureLimit`](crate::renderer::texture_limit::TextureLimit) uses
    /// for its ceiling.
    store: Option<Rc<dyn ImageStore>>,
}

impl ImageRegistry {
    pub(crate) fn new<S: ImageStore + 'static>(store: Rc<S>) -> Self {
        Self { store: Some(store) }
    }

    fn create(&self, id: TextureId, image: &Image) {
        if let Some(store) = &self.store {
            store.create(id, image);
        }
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
