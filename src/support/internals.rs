//! Bench/test reach-in surface. Gated `cfg(any(test, feature = "internals"))`
//! so production builds never see this module.

use crate::FrameReport;
use crate::Host;
use crate::Ui;
use crate::animation::animatable::Animatable;
use crate::forest::tree::{Layer, NodeId};
use crate::layout::scroll::ScrollLayoutState;
use crate::layout::types::display::Display;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::mesh::MeshVertex;
use crate::primitives::rect::Rect;
use crate::primitives::stroke_tessellate::{StrokeStyle, tessellate_polyline_aa};
use crate::primitives::widget_id::WidgetId;
use crate::shape::{ColorMode, LineCap, LineJoin};
use crate::text::TextShaper;
use crate::ui::damage::Damage;
use crate::ui::damage::region::DamageRegion;
use crate::ui::frame_report::RenderPlan;
use crate::widgets::Response;

/// CPU half of `Host::frame` — runs `Ui::frame` without acquiring a swapchain.
pub fn host_cpu_frame<T: 'static>(
    host: &mut Host,
    display: Display,
    state: &mut T,
    record: impl FnMut(&mut Ui),
) -> FrameReport {
    host.cpu_frame(display, state, record)
}

/// GPU half of `Host::frame` against a caller-supplied texture.
pub fn host_render_to_texture(host: &mut Host, target: &wgpu::Texture, report: &FrameReport) {
    host.render_to_texture(target, report);
}

/// Drop every measure-cache entry, forcing full re-measure next frame.
pub fn clear_measure_cache(ui: &mut Ui) {
    let cache = &mut ui.layout_engine.cache;
    cache.nodes.clear();
    cache.hugs.clear();
    cache.text_shapes_arena.clear();
    cache.snapshots.clear();
}

/// Run only the cascade pass against the just-finished frame.
pub fn run_cascades(ui: &mut Ui) {
    ui.cascades_engine.run(&ui.forest, &mut ui.layout);
}

/// Count of frames so far that took the paint-anim-only
/// short-circuit (skipped `pre_record` + user closure +
/// `post_record` + layout + cascades + finalize). Bumped inside
/// `Ui::paint_anim_only_pass`. Read by tests to assert the
/// short-circuit fired (or didn't).
pub fn paint_anim_only_frames(ui: &Ui) -> u64 {
    ui.paint_anim_only_frame_count
}

/// Animation rows currently allocated for `T`, or 0 if no typed map exists.
pub fn anim_row_count<T: Animatable>(ui: &mut Ui) -> usize {
    ui.anim.try_typed_mut::<T>().map_or(0, |t| t.rows.len())
}

/// Scroll-state row for `id` (inserting default if absent).
#[allow(dead_code)]
pub(crate) fn scroll_state(ui: &mut Ui, id: WidgetId) -> &mut ScrollLayoutState {
    ui.layout_engine.scroll_states.entry(id).or_default()
}

/// `Layer::Main` node whose `widget_id` matches `id`. Panics if absent.
#[allow(dead_code)]
pub(crate) fn node_for_widget_id(ui: &Ui, id: WidgetId) -> NodeId {
    let tree = ui.forest.tree(Layer::Main);
    let idx = tree
        .records
        .widget_id()
        .iter()
        .position(|w| *w == id)
        .unwrap_or_else(|| panic!("no node found for widget_id {id:?}"));
    NodeId(idx as u32)
}

/// Old `Response.node` field as an extension method.
#[allow(dead_code)]
pub(crate) trait ResponseNodeExt {
    fn node(&self, ui: &Ui) -> NodeId;
}

impl ResponseNodeExt for Response {
    fn node(&self, ui: &Ui) -> NodeId {
        node_for_widget_id(ui, self.id)
    }
}

/// Total cache-miss `measure` dispatches on `shaper`.
pub fn text_shaper_measure_calls(shaper: &TextShaper) -> u64 {
    shaper.inner.borrow().measure_calls
}

/// `true` iff a reuse entry exists for `(wid, ordinal)`.
pub fn text_shaper_has_reuse_entry(shaper: &TextShaper, wid: WidgetId, ordinal: u16) -> bool {
    shaper.inner.borrow().reuse.contains_key(&(wid, ordinal))
}

/// Damage rects produced by the most recent `post_record`.
pub fn damage_rect_count(ui: &Ui) -> usize {
    ui.damage_engine.region.iter_rects().count()
}

/// Subtree-skip jumps the last damage diff performed.
pub fn damage_subtree_skips(ui: &Ui) -> u32 {
    ui.damage_engine.subtree_skips
}

/// `"skip"` / `"partial"` / `"full"` — the frame's final paint decision.
pub fn damage_paint_kind(ui: &Ui) -> &'static str {
    match ui.damage_engine.filter(ui.display.logical_rect()) {
        Damage::None => "skip",
        Damage::Full => "full",
        Damage::Partial(_) => "partial",
    }
}

/// `DamageRegion` rect count after adding `rects` in order (merge policy runs).
pub fn damage_region_after_adds(rects: &[Rect]) -> usize {
    let mut region = DamageRegion::default();
    for r in rects {
        region.add(*r);
    }
    region.iter_rects().count()
}

/// Simulate a successful submit so the next frame doesn't auto-rewind to `Full`.
pub fn mark_frame_submitted(ui: &Ui) {
    ui.frame_state.mark_submitted();
}

/// Overwrite `report.plan`: empty ⇒ `Full { clear }`, otherwise `Partial`
/// built by adding each rect. Clear colour read from the UI theme.
pub fn force_report_damage_to_rects(report: &mut FrameReport, rects: &[Rect], clear: Color) {
    if rects.is_empty() {
        report.plan = Some(RenderPlan::Full { clear });
        return;
    }
    let mut region = DamageRegion::default();
    for r in rects {
        region.add(*r);
    }
    report.plan = Some(RenderPlan::Partial { clear, region });
}

/// Bench-public mirror of internal `ColorMode`.
pub enum TessColorMode {
    Single,
    PerPoint,
    PerSegment,
}

/// Bench-public mirror of internal `StrokeStyle`.
pub struct TessStyle {
    pub mode: TessColorMode,
    pub cap: LineCap,
    pub join: LineJoin,
    pub width_phys: f32,
}

/// Stroke tessellator with caller-owned scratch.
pub fn tessellate_polyline_for_bench(
    points: &[glam::Vec2],
    colors: &[ColorU8],
    style: TessStyle,
    out_verts: &mut Vec<MeshVertex>,
    out_indices: &mut Vec<u16>,
) {
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
