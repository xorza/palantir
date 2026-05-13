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
use crate::layout::scroll::ScrollLayoutState;
use crate::primitives::widget_id::WidgetId;
use crate::text::TextShaper;

/// Drop every cross-frame measure-cache entry, forcing the next frame
/// to re-measure every leaf from scratch. See `benches/measure_cache.rs`.
pub fn clear_measure_cache(ui: &mut Ui) {
    let cache = &mut ui.layout_engine.cache;
    cache.nodes.clear();
    cache.hugs.clear();
    cache.text_shapes_arena.clear();
    cache.snapshots.clear();
}

/// Run only `CascadesEngine::run` against the just-finished frame's forest +
/// layout results. Lets the cascade bench isolate cascade cost without
/// re-running record / measure / arrange / encode / compose. The
/// caller must have called `Ui::post_record` at least once after the
/// most recent recording so `ui.layout` is populated.
pub fn run_cascades(ui: &mut Ui) {
    ui.cascades_engine.run(&ui.forest, &mut ui.layout);
}

/// Number of animation rows currently allocated for type `T`, or `0`
/// if no typed map for `T` has ever been touched. Used by tests to
/// assert "no rows" / "row exists" without allocating a typed map as
/// a side effect. Takes `&mut Ui` because the type-erased downcast
/// goes through `as_any_mut`.
pub fn anim_row_count<T: Animatable>(ui: &mut Ui) -> usize {
    ui.anim.try_typed_mut::<T>().map_or(0, |t| t.rows.len())
}

/// Borrow (or insert default) the scroll-state row for the layout
/// node at `id`. Tests pass `outer_id.with("__viewport")` — the
/// `LayoutMode::Scroll` node's actual `WidgetId`. Production widgets
/// reach `ui.layout_engine.scroll_states` directly; this helper exists
/// purely to keep test inspection sites short.
#[allow(dead_code)]
pub(crate) fn scroll_state(ui: &mut Ui, id: WidgetId) -> &mut ScrollLayoutState {
    ui.layout_engine.scroll_states.entry(id).or_default()
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

/// Number of damage rects produced by the most recent `post_record`.
/// `0` for `Skip`/`Full` paths (they don't enumerate rects);
/// `1..=DAMAGE_RECT_CAP` for `Partial`. Read by benches to verify
/// scenario shape (e.g. "two-corner change actually produced 2
/// rects").
pub fn damage_rect_count(ui: &Ui) -> usize {
    ui.damage_engine.region.iter_rects().count()
}

/// Variant of `damage_rect_count` that classifies the frame's
/// final paint decision. Lets benches partition timings by Skip /
/// Partial(N) / Full path without the caller reaching into private
/// types.
pub fn damage_paint_kind(ui: &Ui) -> &'static str {
    use crate::ui::damage::Damage;
    let surface = ui.display.logical_rect();
    match ui.damage_engine.filter(surface) {
        None => "skip",
        Some(Damage::Full) => "full",
        Some(Damage::Partial(_)) => "partial",
    }
}

/// Build a `DamageRegion` from a slice of rects, for microbenches.
/// Mirrors what `DamageEngine::compute` does internally without needing a
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

/// Simulate a successful `Renderer::render` for benches that drive
/// `Ui::frame` without a real GPU. Without it, the next frame's
/// `should_invalidate_prev` would fire every iteration — every bench
/// frame would escalate to `Full` and the Skip / Partial scenarios
/// would be unmeasurable.
pub fn mark_frame_submitted(ui: &Ui) {
    ui.frame_state.mark_submitted();
}

/// Force a `FrameReport`'s damage to a specific value, bypassing
/// `DamageEngine::compute`'s merge policy and coverage threshold.
/// Used by the GPU merge bench (`benches/damage_merge_gpu.rs`) to
/// A/B "submit the same scene with N separate damage rects vs one
/// merged bbox" without touching production damage policy. Mutate
/// the report between `Host::run_frame` and `Host::render(&report)`.
///
/// `rects.is_empty()` ⇒ `Damage::Full` (single full-viewport
/// pass). Otherwise builds `Damage::Partial(region)` by
/// `add`ing each rect in order — note that `add` still runs the
/// merge cascade, so passing two overlapping rects collapses to
/// one. Pass disjoint rects to actually exercise the multi-pass
/// path.
pub fn force_report_damage_to_rects(
    report: &mut crate::FrameReport,
    rects: &[crate::primitives::rect::Rect],
) {
    use crate::ui::damage::Damage;
    use crate::ui::damage::region::DamageRegion;
    if rects.is_empty() {
        report.damage = Some(Damage::Full);
        return;
    }
    let mut region = DamageRegion::default();
    for r in rects {
        region.add(*r);
    }
    report.damage = Some(Damage::Partial(region));
}

/// Bench-public mirror of internal `ColorMode`. The user-facing
/// `PolylineColors` is the public surface for stroke color
/// storage, but the bench needs to A/B all three internal variants
/// directly to characterize each emission path.
pub enum TessColorMode {
    Single,
    PerPoint,
    PerSegment,
}

/// Bench-public mirror of internal `StrokeStyle`. Same fields,
/// different visibility — the internal one is `pub(crate)`.
pub struct TessStyle {
    pub mode: TessColorMode,
    pub cap: crate::shape::LineCap,
    pub join: crate::shape::LineJoin,
    pub width_phys: f32,
}

/// Bench entry point: run the stroke tessellator with externally-
/// owned scratch (so allocation doesn't dominate the measurement).
pub fn tessellate_polyline_for_bench(
    points: &[glam::Vec2],
    colors: &[crate::primitives::color::Color],
    style: TessStyle,
    out_verts: &mut Vec<crate::primitives::mesh::MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
    use crate::primitives::stroke_tessellate::{StrokeStyle, tessellate_polyline_aa};
    use crate::shape::ColorMode;
    let mode = match style.mode {
        TessColorMode::Single => ColorMode::Single,
        TessColorMode::PerPoint => ColorMode::PerPoint,
        TessColorMode::PerSegment => ColorMode::PerSegment,
    };
    tessellate_polyline_aa(
        points,
        colors,
        StrokeStyle {
            mode,
            cap: style.cap,
            join: style.join,
            width_phys: style.width_phys,
        },
        out_verts,
        out_indices,
    );
}
