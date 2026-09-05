//! Registered image lifetimes shared by the host, its recorders, and the
//! backend.
//!
//! [`ImageHandle`](image_handle::ImageHandle) is the RAII owner of one
//! registered image, and the registry is what every handle reaches. The
//! texels go straight to the [`ImageStore`] the backend attaches when the
//! host builds it over the same resources — the trait carries the
//! immediacy contract — so the registry keeps no CPU copy and waits for
//! no frame. A registry no backend was ever built over is a deviceless
//! recorder: it keeps ids, sizes and generations and discards the texels.
//!
//! The pure data types live elsewhere —
//! [`Image`] / [`ImageFit`](crate::primitives::image::ImageFit) in
//! `primitives`, [`TextureId`] and the counter it is minted from in
//! `primitives::texture_id`, and the device ceiling a source is
//! measured against in
//! [`TextureLimit`](crate::renderer::texture_limit::TextureLimit) — so this
//! module owns only the lifecycle. `UiResources` mints the id and applies
//! the ceiling before a handle is built, so nothing here can fail.

pub(crate) mod image_handle;
pub(crate) mod image_store;

use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::image_registry::image_store::ImageStore;
use std::cell::OnceCell;
use std::rc::Rc;

/// Cheap to clone: every clone reaches the one store.
#[derive(Clone, Debug, Default)]
pub(crate) struct ImageRegistry {
    /// Set once, by the backend that draws these images, and never for a
    /// standalone CPU recorder — the same convention
    /// [`TextureLimit`](crate::renderer::texture_limit::TextureLimit) uses
    /// for its ceiling. A clone copies the cell as it stands, which is why
    /// the host attaches before it mints a recorder.
    store: OnceCell<Rc<dyn ImageStore>>,
}

impl ImageRegistry {
    /// Give the registry the store its texels go to. The one backend a
    /// host builds calls this at its construction, through the host's own
    /// registry and before the host mints a recorder, so every clone a
    /// handle or a window later takes carries the store.
    ///
    /// # Panics
    ///
    /// Panics on a second call: one host, one backend, one store.
    pub(crate) fn attach<S: ImageStore + 'static>(&self, store: Rc<S>) {
        let store: Rc<dyn ImageStore> = store;
        assert!(
            self.store.set(store).is_ok(),
            "one backend attaches the image store once",
        );
    }

    fn write(&self, id: TextureId, image: &Image) {
        if let Some(store) = self.store.get() {
            store.write(id, image);
        }
    }

    fn free(&self, id: TextureId) {
        if let Some(store) = self.store.get() {
            store.free(id);
        }
    }
}
