//! App-global resources shared by the host, its windows, and the GPU backend.
//! [`HostShared`] is the retained authority; [`UiShared`] and
//! [`BackendShared`] are capability-specific clone bundles derived from it.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use crate::common::clipboard::Clipboard;
use crate::debug_overlay::DebugOverlayConfig;
use crate::renderer::assets::RenderAssets;
use crate::renderer::backend::BackendShared;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::text::TextShaper;
use crate::window::WindowToken;

#[derive(Debug, Default)]
pub(crate) struct HostShared {
    pub(crate) text: TextShaper,
    pub(crate) assets: RenderAssets,
    pub(crate) clipboard: Clipboard,
    pub(crate) diagnostics: DiagnosticsShared,
    pub(crate) windows: WindowDirectory,
}

#[derive(Clone, Debug)]
pub(crate) struct UiShared {
    pub(crate) text: TextShaper,
    pub(crate) assets: RenderAssets,
    pub(crate) clipboard: Clipboard,
    pub(crate) diagnostics: DiagnosticsShared,
    pub(crate) windows: WindowDirectory,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct DiagnosticsShared {
    pub(crate) pass_stats: GpuPassStats,
    state: Rc<RefCell<DiagnosticsState>>,
}

#[derive(Debug, Default)]
struct DiagnosticsState {
    overlay: DebugOverlayConfig,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WindowDirectory {
    tokens: Rc<RefCell<Vec<WindowToken>>>,
}

impl HostShared {
    pub(crate) fn new(text: TextShaper) -> Self {
        Self {
            text,
            ..Default::default()
        }
    }

    pub(crate) fn ui_shared(&self) -> UiShared {
        UiShared {
            text: self.text.clone(),
            assets: self.assets.clone(),
            clipboard: self.clipboard.clone(),
            diagnostics: self.diagnostics.clone(),
            windows: self.windows.clone(),
        }
    }

    pub(crate) fn backend_shared(&self) -> BackendShared {
        BackendShared {
            text: self.text.clone(),
            assets: self.assets.clone(),
            pass_stats: self.diagnostics.pass_stats.clone(),
        }
    }
}

impl DiagnosticsShared {
    pub(crate) fn debug_overlay(&self) -> DebugOverlayConfig {
        self.state.borrow().overlay
    }

    pub(crate) fn debug_overlay_mut(&self) -> RefMut<'_, DebugOverlayConfig> {
        RefMut::map(self.state.borrow_mut(), |state| &mut state.overlay)
    }
}

impl WindowDirectory {
    pub(crate) fn contains(&self, token: WindowToken) -> bool {
        self.tokens.borrow().contains(&token)
    }

    pub(crate) fn set_live(&self, token: WindowToken, live: bool) {
        let mut tokens = self.tokens.borrow_mut();
        let index = tokens.iter().position(|candidate| *candidate == token);
        if live {
            assert!(
                index.is_none(),
                "window directory already contains {token:?}"
            );
            tokens.push(token);
        } else {
            tokens.swap_remove(index.expect("removed window must exist in the window directory"));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::debug_overlay::DebugOverlayConfig;
    use crate::host::shared::HostShared;
    use crate::renderer::texture_id::TextureId;
    use crate::window::WindowToken;

    #[test]
    fn window_directory_mutations_are_shared_with_ui_capabilities() {
        let shared = HostShared::default();
        let ui = shared.ui_shared();

        shared.windows.set_live(WindowToken(1), true);
        shared.windows.set_live(WindowToken(2), true);
        assert!(ui.windows.contains(WindowToken(1)));
        assert!(ui.windows.contains(WindowToken(2)));

        shared.windows.set_live(WindowToken(1), false);
        assert!(!ui.windows.contains(WindowToken(1)));
        assert!(ui.windows.contains(WindowToken(2)));
    }

    #[test]
    fn diagnostics_are_shared_across_capability_bundles() {
        let shared = HostShared::default();
        let ui = shared.ui_shared();
        assert_eq!(
            shared.diagnostics.debug_overlay(),
            DebugOverlayConfig::default()
        );

        ui.diagnostics.debug_overlay_mut().damage_rect = true;

        assert!(shared.diagnostics.debug_overlay().damage_rect);
        assert!(ui.diagnostics.debug_overlay().damage_rect);
    }

    #[test]
    fn backend_and_ui_share_text_assets_and_gpu_stats() {
        let shared = HostShared::default();
        let ui = shared.ui_shared();
        let backend = shared.backend_shared();

        assert!(Rc::ptr_eq(&ui.text.inner, &backend.text.inner));
        assert_eq!(ui.assets.texture_ids.reserve(), TextureId(1));
        assert_eq!(backend.assets.texture_ids.reserve(), TextureId(2));
        backend.pass_stats.record_pass_ns(2_500_000);
        assert_eq!(ui.diagnostics.pass_stats.last_pass_ms(), Some(2.5));
    }

    #[test]
    fn clipboard_is_shared_within_one_host_and_isolated_between_hosts() {
        let first = HostShared::default();
        let first_window = first.ui_shared();
        let second_window = first.ui_shared();
        let second = HostShared::default().ui_shared();

        first_window.clipboard.set("shared").unwrap();

        assert_eq!(second_window.clipboard.get(), "shared");
        assert_eq!(second.clipboard.get(), "");
    }
}
