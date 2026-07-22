//! App-global resource composition for hosts. [`HostShared`] retains recorder
//! resources and the frontend's gradient-atlas handle, then derives the
//! backend's capability bundle from those shared authorities.

use crate::common::clipboard::Clipboard;
use crate::renderer::backend::BackendResources;
use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
use crate::text::TextShaper;
use crate::ui::resources::UiResources;
use std::num::NonZeroU32;

#[derive(Debug)]
pub(crate) struct HostShared {
    pub(crate) resources: UiResources,
    pub(crate) gradient_atlas: SharedGradientAtlas,
}

impl HostShared {
    pub(crate) fn new(text: TextShaper, max_texture_dimension_2d: Option<NonZeroU32>) -> Self {
        Self::with_clipboard(text, Clipboard::default(), max_texture_dimension_2d)
    }

    pub(crate) fn with_clipboard(
        text: TextShaper,
        clipboard: Clipboard,
        max_texture_dimension_2d: Option<NonZeroU32>,
    ) -> Self {
        Self {
            resources: UiResources::new(text, clipboard, max_texture_dimension_2d),
            gradient_atlas: SharedGradientAtlas::default(),
        }
    }

    pub(crate) fn backend_resources(&self) -> BackendResources {
        BackendResources {
            text: self.resources.text.clone(),
            images: self.resources.images.clone(),
            gradient_atlas: self.gradient_atlas.clone(),
            gpu_pass_stats: self.resources.diagnostics.gpu_pass_stats.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::rc::Rc;

    use crate::diagnostics::DebugOverlayConfig;
    use crate::host::shared::HostShared;
    use crate::primitives::image::Image;
    use crate::text::TextShaper;

    #[test]
    fn diagnostics_are_shared_across_capability_bundles() {
        let shared = HostShared::new(TextShaper::default(), None);
        let ui = shared.resources.clone();
        assert_eq!(
            *shared.resources.diagnostics.overlay.borrow(),
            DebugOverlayConfig::default()
        );

        ui.diagnostics.overlay.borrow_mut().damage_rect = true;

        assert!(shared.resources.diagnostics.overlay.borrow().damage_rect);
        assert!(ui.diagnostics.overlay.borrow().damage_rect);
    }

    #[test]
    fn backend_and_ui_share_text_images_and_gpu_stats() {
        let shared = HostShared::new(TextShaper::default(), Some(NonZeroU32::new(1).unwrap()));
        let ui = shared.resources.clone();
        let backend = shared.backend_resources();

        assert!(Rc::ptr_eq(&ui.text.inner, &backend.text.inner));
        let rejected = ui
            .images
            .register(Image::from_rgba8(2, 1, vec![0; 8]))
            .unwrap_err();
        assert_eq!(rejected.max_dimension, 1);
        let image = ui
            .images
            .register(Image::from_rgba8(1, 1, vec![1, 2, 3, 4]))
            .unwrap();
        let mut uploaded = None;
        backend.images.drain_pending(|id, data| {
            uploaded = Some(id);
            assert_eq!(data.pixels, vec![1, 2, 3, 4]);
        });
        assert_eq!(uploaded, Some(image.id()));
        backend.gpu_pass_stats.record_pass_ns(2_500_000);
        assert_eq!(ui.diagnostics.gpu_pass_stats.last_pass_ms(), Some(2.5));
    }

    #[test]
    fn clipboard_is_shared_within_one_host_and_isolated_between_hosts() {
        let first = HostShared::new(TextShaper::default(), None);
        let first_window = first.resources.clone();
        let second_window = first.resources.clone();
        let second = HostShared::new(TextShaper::default(), None).resources;

        first_window.clipboard.set("shared").unwrap();

        assert_eq!(second_window.clipboard.get(), "shared");
        assert_eq!(second.clipboard.get(), "");
    }
}
