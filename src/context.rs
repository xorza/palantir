//! [`HostContext`] — the app-global shared state cloned into every
//! window's [`Ui`](crate::ui::Ui): the GPU-agnostic render resources (text
//! shaper, per-frame arena, CPU-side render caches, GPU-stats handle) plus
//! the host state (live-window set + debug overlay). One per app, owned by
//! the windowing host ([`WinitHost`](crate::WinitHost) /
//! [`OffscreenHost`](crate::offscreen_host::OffscreenHost)).
//!
//! It's a passive bag, not a factory: the host builds one, hands it to
//! `WgpuBackend::new` (which clones the render handles it needs) and to
//! `Ui::new` / `Frontend::new`. Every field is a cheap Rc/Arc-backed
//! handle, so all clones point at one shared set — including the host
//! state, which rides a single `Rc<RefCell<>>`. Single-threaded by
//! construction (the event loop and every window's frame run on one
//! thread), so it's `Rc<RefCell<>>`, not `Arc<Mutex<>>`; borrows are taken
//! and dropped within one method call, never held across a frame.

use std::cell::{RefCell, RefMut};
use std::rc::Rc;

use crate::debug_overlay::DebugOverlayConfig;
use crate::forest::frame_arena::FrameArena;
use crate::renderer::backend::gpu_pass_stats::GpuPassStats;
use crate::renderer::caches::RenderCaches;
use crate::renderer::texture_id::TextureIdSource;
use crate::text::TextShaper;
use crate::window::WindowToken;

/// Shared, app-global state cloned into every window's `Ui` + `Frontend`
/// and (the render handles) into the one shared backend. Cloning is cheap —
/// every field is an Rc/Arc-backed handle pointing at one set.
#[derive(Clone)]
pub(crate) struct HostContext {
    /// Shared `TextureId` source. `caches.images` (CPU images) and each
    /// window's `GpuViewRegistry` (render targets) both mint from this one
    /// counter, so their ids never collide in the one backend texture cache.
    pub(crate) texture_ids: TextureIdSource,
    pub(crate) shaper: TextShaper,
    pub(crate) frame_arena: FrameArena,
    pub(crate) caches: RenderCaches,
    pub(crate) pass_stats: GpuPassStats,
    /// App-global host state (live-window set + debug overlay) behind one
    /// shared cell, so a toggle or window open/close from any window is
    /// seen by all. Private: the backend pulls only the render handles
    /// above, never this.
    host: Rc<RefCell<HostState>>,
}

impl Default for HostContext {
    fn default() -> Self {
        // `caches.images` must mint from the same source the per-window
        // GpuViewRegistries use, so build the source first and wire it in
        // (the derived `Default` would give each its own counter).
        let texture_ids = TextureIdSource::default();
        Self {
            caches: RenderCaches::new(texture_ids.clone()),
            texture_ids,
            shaper: Default::default(),
            frame_arena: Default::default(),
            pass_stats: Default::default(),
            host: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct HostState {
    /// Tokens of every currently-live window. The host rewrites it on
    /// open/close; [`HostContext::window_open`] reads it. Retained Vec
    /// — capacity reused, so the steady state is alloc-free.
    open_windows: Vec<WindowToken>,
    /// App-global debug overlay. The host reads it to drive the backend's
    /// overlay passes; user code toggles it via
    /// [`HostContext::debug_overlay_mut`].
    debug_overlay: DebugOverlayConfig,
    /// Set whenever `debug_overlay` is handed out mutably; the host takes
    /// it each tick to know a window toggled the overlay and idle windows
    /// need repainting. Lives with the data, so the host needs no separate
    /// "last seen" snapshot.
    overlay_dirty: bool,
}

impl HostContext {
    /// Build the context around `shaper` (the caller supplies it so headless
    /// harnesses can share a process/thread-local shaper). Everything else
    /// is fresh.
    pub(crate) fn new(shaper: TextShaper) -> Self {
        Self {
            shaper,
            ..Default::default()
        }
    }

    pub(crate) fn window_open(&self, token: WindowToken) -> bool {
        self.host.borrow().open_windows.contains(&token)
    }

    /// Replace the live-window set (host-side, on open/close). Reuses the
    /// Vec's capacity.
    pub(crate) fn set_open_windows(&self, tokens: impl IntoIterator<Item = WindowToken>) {
        let mut host = self.host.borrow_mut();
        host.open_windows.clear();
        host.open_windows.extend(tokens);
    }

    pub(crate) fn debug_overlay(&self) -> DebugOverlayConfig {
        self.host.borrow().debug_overlay
    }

    /// Mutable borrow of the app-global overlay; the guard derefs to
    /// `&mut DebugOverlayConfig`. Writes land globally at once, and the
    /// borrow flags the overlay dirty so the host repaints idle windows
    /// (otherwise damage-`Skip`, they'd never show the change).
    pub(crate) fn debug_overlay_mut(&self) -> RefMut<'_, DebugOverlayConfig> {
        let mut host = self.host.borrow_mut();
        host.overlay_dirty = true;
        RefMut::map(host, |h| &mut h.debug_overlay)
    }

    /// Take the "overlay toggled since last check" flag. The host calls
    /// this each tick; `true` ⇒ repaint every window. Flagged on any
    /// mutable borrow rather than on an actual value change, but the only
    /// caller is the toggle path, so in practice it fires exactly on a
    /// real change.
    pub(crate) fn take_overlay_dirty(&self) -> bool {
        std::mem::take(&mut self.host.borrow_mut().overlay_dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_windows_round_trip_is_shared_across_clones() {
        let a = HostContext::default();
        let b = a.clone();
        assert!(!a.window_open(WindowToken(1)));
        a.set_open_windows([WindowToken(1), WindowToken(2)]);
        // The clone shares one host cell, so it sees the write.
        assert!(b.window_open(WindowToken(1)));
        assert!(b.window_open(WindowToken(2)));
        assert!(!b.window_open(WindowToken(3)));
        // A rewrite replaces the set rather than appending.
        a.set_open_windows([WindowToken(3)]);
        assert!(!b.window_open(WindowToken(1)));
        assert!(b.window_open(WindowToken(3)));
    }

    #[test]
    fn overlay_mut_flags_dirty_then_take_clears_it() {
        let ctx = HostContext::default();
        assert!(!ctx.take_overlay_dirty(), "fresh state is clean");
        // A mutable borrow flags dirty — even though the value matched the
        // default, the borrow alone trips it (the documented behavior).
        ctx.debug_overlay_mut().damage_rect = true;
        assert!(ctx.take_overlay_dirty(), "mutable borrow flags dirty");
        assert!(!ctx.take_overlay_dirty(), "take clears the flag");
        // The write is observable through the read path.
        assert!(ctx.debug_overlay().damage_rect);
    }

    #[test]
    fn overlay_writes_and_dirty_flag_are_shared_across_clones() {
        let a = HostContext::default();
        let b = a.clone();
        a.debug_overlay_mut().frame_stats = true;
        // Either handle observes + clears the one shared dirty flag.
        assert!(b.take_overlay_dirty());
        assert!(!a.take_overlay_dirty());
        assert!(b.debug_overlay().frame_stats);
    }
}
