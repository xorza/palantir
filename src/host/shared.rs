//! App-global resource composition for hosts. [`HostShared`] retains recorder
//! resources and the frontend's gradient-atlas handle, then derives the
//! backend's capability bundle from those shared authorities.

use crate::common::clipboard::Clipboard;
use crate::renderer::backend::backend_resources::BackendResources;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;

#[derive(Debug)]
pub(crate) struct HostShared {
    pub(crate) resources: UiResources,
    pub(crate) gradient_atlas: SharedGradientAtlas,
}

impl HostShared {
    pub(super) fn with_clipboard(
        text: TextShaper,
        clipboard: Clipboard,
        texture_limit: TextureLimit,
    ) -> Self {
        Self {
            resources: UiResources::new(text, clipboard, texture_limit),
            gradient_atlas: SharedGradientAtlas::new(texture_limit),
        }
    }

    pub(super) fn backend_resources(&self) -> BackendResources {
        BackendResources {
            text: self.resources.text.clone(),
            images: self.resources.images.clone(),
            icons: self.resources.icons.clone(),
            gradient_atlas: self.gradient_atlas.clone(),
            gpu_pass_stats: self.resources.diagnostics.gpu_pass_stats.clone(),
        }
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::common::clipboard::Clipboard;
    use crate::host::shared::HostShared;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;

    impl HostShared {
        /// Resources over a memory clipboard, for tests that drive a
        /// `WindowDriver` without a host. Production hosts go through
        /// `HostCore::new`, which supplies the platform clipboard.
        pub(crate) fn new(text: TextShaper, texture_limit: TextureLimit) -> Self {
            Self::with_clipboard(text, Clipboard::default(), texture_limit)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use crate::diagnostics::DebugOverlayConfig;
    use crate::host::shared::HostShared;
    use crate::primitives::image::Image;
    use crate::renderer::texture_limit::TextureLimit;
    use crate::text::shaper::TextShaper;

    #[test]
    fn diagnostics_are_shared_across_capability_bundles() {
        let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
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
        let limit = TextureLimit::from_device(NonZeroU32::new(1).unwrap());
        let shared = HostShared::new(TextShaper::test_mono(), limit);
        let ui = shared.resources.clone();
        let backend = shared.backend_resources();

        assert!(ui.text.shares_cache_with(&backend.text));
        assert_eq!(
            ui.texture_limit, limit,
            "the recorder bundle carries the ceiling the host was built with",
        );
        let image = ui
            .images
            .register(Image::from_rgba8(1, 1, vec![1, 2, 3, 4]));
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
        let first = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
        let first_window = first.resources.clone();
        let second_window = first.resources.clone();
        let second = HostShared::new(TextShaper::test_mono(), TextureLimit::default()).resources;

        first_window.clipboard.set("shared").unwrap();

        assert_eq!(second_window.clipboard.get(), "shared");
        assert_eq!(second.clipboard.get(), "");
    }
}
