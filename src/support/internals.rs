//! Bench/test-only helpers that reach into `Ui`'s cross-frame caches.
//! Gated `#[cfg(test)]` at the lib.rs declaration so production builds
//! never see this module.
//!
//! `cargo bench` enables `cfg(test)` for the lib crate, so criterion
//! benches in `benches/` can call these alongside in-tree tests.
//!
//! These exist *only* to A/B cache-enabled vs forced-miss frames in the
//! cache benches and to assert cache population in tests. Production
//! code should never need them.

use crate::{Ui, WgpuBackend};

/// Drop every cross-frame measure-cache entry, forcing the next frame
/// to re-measure every leaf from scratch. See `benches/measure_cache.rs`.
pub fn clear_measure_cache(ui: &mut Ui) {
    let cache = &mut ui.layout.cache;
    cache.nodes.clear();
    cache.hugs.clear();
    cache.text_shapes_arena.clear();
    cache.snapshots.clear();
}

/// Drop every cross-frame encode-cache entry, forcing the next frame's
/// encoder to re-encode every subtree from scratch. See
/// `benches/encode_cache.rs`.
pub fn clear_encode_cache(ui: &mut Ui) {
    ui.frontend.encoder.cache.clear();
}

/// Run only `Cascades::run` against the just-finished frame's forest +
/// layout results. Lets the cascade bench isolate cascade cost without
/// re-running record / measure / arrange / encode / compose. The
/// caller must have called `Ui::end_frame` at least once after the
/// most recent recording so `ui.layout.results` is populated.
pub fn run_cascades(ui: &mut Ui) {
    let _ = ui.cascades.run(&ui.forest, &ui.layout.results);
}

/// Render-debug knob: when `on`, every frame loads with `LoadOp::Clear`
/// (the submit-time clear color) even on `DamagePaint::Partial`. The
/// scissor still applies, so only the dirty region paints — surrounding
/// pixels flash the clear color. Used by the damage-visualization
/// fixtures in `tests/visual/` to see exactly which pixels were
/// repainted this frame.
pub fn set_clear_on_damage(backend: &mut WgpuBackend, on: bool) {
    backend.debug_clear_on_damage = on;
}
