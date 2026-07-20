//! App-global resource composition for hosts. [`HostShared`] retains the
//! authority and creates capability-specific bundles for each recorder and the
//! two renderer stages.

use crate::common::clipboard::Clipboard;
use crate::renderer::backend::BackendResources;
use crate::renderer::frontend::FrontendResources;
use crate::text::TextShaper;
use crate::ui::resources::UiResources;

#[derive(Debug)]
pub(crate) struct HostShared {
    pub(crate) resources: UiResources,
    pub(crate) frontend: FrontendResources,
}

impl HostShared {
    pub(crate) fn new(text: TextShaper) -> Self {
        Self::with_clipboard(text, Clipboard::default())
    }

    pub(crate) fn with_clipboard(text: TextShaper, clipboard: Clipboard) -> Self {
        Self {
            resources: UiResources::new(text, clipboard),
            frontend: FrontendResources::default(),
        }
    }

    pub(crate) fn backend_resources(&self) -> BackendResources {
        BackendResources {
            text: self.resources.text.clone(),
            images: self.resources.images.clone(),
            gradient_atlas: self.frontend.gradient_atlas.clone(),
            gpu_pass_stats: self.resources.diagnostics.gpu_pass_stats.clone(),
        }
    }
}

impl Default for HostShared {
    fn default() -> Self {
        Self::new(TextShaper::default())
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::diagnostics::DebugOverlayConfig;
    use crate::host::shared::HostShared;
    use crate::primitives::image::Image;

    #[test]
    fn diagnostics_are_shared_across_capability_bundles() {
        let shared = HostShared::default();
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
        let shared = HostShared::default();
        let ui = shared.resources.clone();
        let backend = shared.backend_resources();

        assert!(Rc::ptr_eq(&ui.text.inner, &backend.text.inner));
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
        let first = HostShared::default();
        let first_window = first.resources.clone();
        let second_window = first.resources.clone();
        let second = HostShared::default().resources;

        first_window.clipboard.set("shared").unwrap();

        assert_eq!(second_window.clipboard.get(), "shared");
        assert_eq!(second.clipboard.get(), "");
    }
}
