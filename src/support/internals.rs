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

use crate::Ui;
use crate::animation::animatable::Animatable;
use crate::text::TextShaper;
use crate::tree::widget_id::WidgetId;

/// Drop every cross-frame measure-cache entry, forcing the next frame
/// to re-measure every leaf from scratch. See `benches/measure_cache.rs`.
pub fn clear_measure_cache(ui: &mut Ui) {
    let cache = &mut ui.layout.cache;
    cache.nodes.clear();
    cache.hugs.clear();
    cache.text_shapes_arena.clear();
    cache.snapshots.clear();
}

/// Run only `Cascades::run` against the just-finished frame's forest +
/// layout results. Lets the cascade bench isolate cascade cost without
/// re-running record / measure / arrange / encode / compose. The
/// caller must have called `Ui::end_frame` at least once after the
/// most recent recording so `ui.layout.result` is populated.
pub fn run_cascades(ui: &mut Ui) {
    let _ = ui.cascades.run(&ui.forest, &ui.layout.result);
}

/// Number of animation rows currently allocated for type `T`, or `0`
/// if no typed map for `T` has ever been touched. Used by tests to
/// assert "no rows" / "row exists" without allocating a typed map as
/// a side effect.
pub fn anim_row_count<T: Animatable>(ui: &Ui) -> usize {
    ui.anim.try_typed::<T>().map_or(0, |t| t.rows.len())
}

/// Total `measure` calls dispatched through `shaper` (cache misses
/// only). Cache hits don't increment. Read by tests pinning
/// reshape-skip behaviour and bench A/B fixtures.
pub fn text_shaper_measure_calls(shaper: &TextShaper) -> u64 {
    shaper.inner.borrow().measure_calls
}

/// `true` if a reuse entry exists for `(wid, ordinal)` in `shaper`'s
/// per-widget measure cache. Used by sweep fixtures to confirm entries
/// land on first render and get evicted when the widget disappears.
pub fn text_shaper_has_reuse_entry(shaper: &TextShaper, wid: WidgetId, ordinal: u16) -> bool {
    shaper.inner.borrow().reuse.contains_key(&(wid, ordinal))
}

/// Number of damage rects produced by the most recent `end_frame`.
/// `0` for `Skip`/`Full` paths (they don't enumerate rects);
/// `1..=DAMAGE_RECT_CAP` for `Partial`. Read by benches to verify
/// scenario shape (e.g. "two-corner change actually produced 2
/// rects").
pub fn damage_rect_count(ui: &Ui) -> usize {
    ui.damage.region.iter_rects().count()
}

/// Variant of `damage_rect_count` that classifies the frame's
/// final paint decision. Lets benches partition timings by Skip /
/// Partial(N) / Full path without the caller reaching into private
/// types.
pub fn damage_paint_kind(ui: &Ui) -> &'static str {
    use crate::ui::damage::DamagePaint;
    let surface = ui.display.logical_rect();
    match ui.damage.filter(surface) {
        DamagePaint::Skip => "skip",
        DamagePaint::Full => "full",
        DamagePaint::Partial(_) => "partial",
    }
}

/// Build a `DamageRegion` from a slice of rects, for microbenches.
/// Mirrors what `Damage::compute` does internally without needing a
/// full `Ui` setup. Returns the rect count after the merge policy
/// runs — benches use that to verify "8 disjoint inputs produced 8
/// retained rects" (no min-growth fired) vs "9 inputs produced 8
/// retained" (min-growth did fire).
pub fn damage_region_after_adds(rects: &[crate::primitives::rect::Rect]) -> usize {
    let mut region = crate::ui::damage::region::DamageRegion::default();
    for r in rects {
        region.add(*r);
    }
    region.iter_rects().count()
}

/// Simulate a successful `WgpuBackend::submit` for benches that
/// drive `Ui::run_frame` without a real GPU. Without it,
/// `Ui::begin_frame`'s auto-rewind would fire every iteration —
/// every bench frame would escalate to `Full` and the Skip /
/// Partial scenarios would be unmeasurable.
pub fn mark_frame_submitted(out: &crate::renderer::frontend::FrameOutput<'_>) {
    out.frame_state.mark_submitted();
}

/// Force a frame's damage decision, bypassing `Damage::compute`'s
/// merge policy and coverage threshold. Used by the GPU merge bench
/// (`benches/damage_merge_gpu.rs`) to A/B "submit the same scene
/// with N separate damage rects vs one merged bbox" without
/// touching production damage policy.
///
/// `rects.is_empty()` ⇒ `DamagePaint::Full` (single full-viewport
/// pass). Otherwise builds `DamagePaint::Partial(region)` by
/// `add`ing each rect in order — note that `add` still runs the
/// merge cascade, so passing two overlapping rects collapses to
/// one. Pass disjoint rects to actually exercise the multi-pass
/// path.
pub fn force_frame_damage_to_rects(
    out: &mut crate::renderer::frontend::FrameOutput<'_>,
    rects: &[crate::primitives::rect::Rect],
) {
    use crate::ui::damage::DamagePaint;
    use crate::ui::damage::region::DamageRegion;
    if rects.is_empty() {
        out.damage = DamagePaint::Full;
        return;
    }
    let mut region = DamageRegion::default();
    for r in rects {
        region.add(*r);
    }
    out.damage = DamagePaint::Partial(region);
}
