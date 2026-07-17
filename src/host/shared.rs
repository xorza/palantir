//! App-global resources shared by the host, its windows, and the GPU backend.
//! [`HostShared`] is the retained authority; [`UiShared`] and
//! [`BackendShared`] are capability-specific clone bundles derived from it.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use crate::debug_overlay::DebugOverlayConfig;
use crate::renderer::assets::RenderAssets;
use crate::renderer::backend::BackendShared;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::text::TextShaper;
use crate::window::WindowToken;

#[derive(Debug, Default)]
pub(crate) struct HostShared {
    pub(crate) render: RenderShared,
    pub(crate) diagnostics: DiagnosticsShared,
    pub(crate) windows: WindowDirectory,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RenderShared {
    pub(crate) text: TextShaper,
    pub(crate) assets: RenderAssets,
}

#[derive(Clone, Debug)]
pub(crate) struct UiShared {
    pub(crate) render: RenderShared,
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
    overlay_dirty: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WindowDirectory {
    tokens: Rc<RefCell<Vec<WindowToken>>>,
}

impl HostShared {
    pub(crate) fn new(text: TextShaper) -> Self {
        Self {
            render: RenderShared {
                text,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub(crate) fn ui_shared(&self) -> UiShared {
        UiShared {
            render: self.render.clone(),
            diagnostics: self.diagnostics.clone(),
            windows: self.windows.clone(),
        }
    }

    pub(crate) fn backend_shared(&self) -> BackendShared {
        BackendShared {
            text: self.render.text.clone(),
            assets: self.render.assets.clone(),
            pass_stats: self.diagnostics.pass_stats.clone(),
        }
    }
}

impl DiagnosticsShared {
    pub(crate) fn debug_overlay(&self) -> DebugOverlayConfig {
        self.state.borrow().overlay
    }

    pub(crate) fn debug_overlay_mut(&self) -> RefMut<'_, DebugOverlayConfig> {
        let mut state = self.state.borrow_mut();
        state.overlay_dirty = true;
        RefMut::map(state, |state| &mut state.overlay)
    }

    pub(crate) fn take_overlay_dirty(&self) -> bool {
        std::mem::take(&mut self.state.borrow_mut().overlay_dirty)
    }
}

impl WindowDirectory {
    pub(crate) fn contains(&self, token: WindowToken) -> bool {
        self.tokens.borrow().contains(&token)
    }

    pub(crate) fn insert(&self, token: WindowToken) {
        let mut tokens = self.tokens.borrow_mut();
        assert!(
            !tokens.contains(&token),
            "window directory already contains {token:?}"
        );
        tokens.push(token);
    }

    pub(crate) fn remove(&self, token: WindowToken) {
        let mut tokens = self.tokens.borrow_mut();
        let index = tokens
            .iter()
            .position(|candidate| *candidate == token)
            .expect("removed window must exist in the window directory");
        tokens.swap_remove(index);
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::host::shared::HostShared;
    use crate::window::WindowToken;

    #[test]
    fn window_directory_mutations_are_shared_with_ui_capabilities() {
        let shared = HostShared::default();
        let ui = shared.ui_shared();

        shared.windows.insert(WindowToken(1));
        shared.windows.insert(WindowToken(2));
        assert!(ui.windows.contains(WindowToken(1)));
        assert!(ui.windows.contains(WindowToken(2)));

        shared.windows.remove(WindowToken(1));
        assert!(!ui.windows.contains(WindowToken(1)));
        assert!(ui.windows.contains(WindowToken(2)));
    }

    #[test]
    fn diagnostics_are_shared_across_capability_bundles() {
        let shared = HostShared::default();
        let ui = shared.ui_shared();
        assert!(!shared.diagnostics.take_overlay_dirty());

        ui.diagnostics.debug_overlay_mut().damage_rect = true;

        assert!(shared.diagnostics.take_overlay_dirty());
        assert!(!ui.diagnostics.take_overlay_dirty());
        assert!(shared.diagnostics.debug_overlay().damage_rect);
    }

    #[test]
    fn backend_and_ui_share_render_resources_and_gpu_stats() {
        let shared = HostShared::default();
        let ui = shared.ui_shared();
        let backend = shared.backend_shared();

        assert!(Rc::ptr_eq(&ui.render.text.inner, &backend.text.inner));
        backend.pass_stats.record_pass_ns(2_500_000);
        assert_eq!(ui.diagnostics.pass_stats.last_pass_ms(), Some(2.5));
    }
}
