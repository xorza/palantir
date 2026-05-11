//! Shared helpers for tests across the crate.

#![cfg(test)]

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::shapes::ShapeRecord;
#[allow(unused_imports)]
use crate::forest::tree::Layer;
use crate::forest::tree::{NodeId, Tree, TreeItem};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::rect::Rect;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::encoder::Encoder;
use crate::text::TextShaper;
use crate::ui::damage::region::DamageRegion;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Direct shapes of `node` — including panels whose direct shapes are
/// interleaved between children (scrollbar overlays, parent-pushed
/// sub-rects). Production callers on leaves use `Tree::leaf_shapes`
/// (direct slice, no per-item branch) instead.
pub(crate) fn shapes_of(tree: &Tree, node: NodeId) -> impl Iterator<Item = &ShapeRecord> + '_ {
    tree.tree_items(node).filter_map(|item| match item {
        TreeItem::ShapeRecord(s) => Some(s),
        TreeItem::Child(_) => None,
    })
}

pub(crate) fn begin(ui: &mut Ui, size: UVec2) {
    ui.pre_record(Display::from_physical(size, 1.0));
}

/// Drive one full frame through the production [`Ui::run_frame`]
/// path at the given surface size. Time is frozen at zero — tests
/// that exercise animation pass `now` themselves via `run_frame`
/// directly. Discards the returned [`crate::renderer::frontend::FrameOutput`]
/// so the caller can keep mutating `ui` afterwards.
pub(crate) fn run_at(ui: &mut Ui, size: UVec2, record: impl FnMut(&mut Ui)) {
    let display = Display::from_physical(size, 1.0);
    ui.run_frame(display, Duration::ZERO, record);
}

/// Same as [`run_at`] but additionally marks the frame as
/// submitted. Tests that drive frames without going through a real
/// `WgpuBackend::submit` need this — otherwise
/// [`Ui::pre_record`]'s auto-rewind kicks in and every subsequent
/// frame's damage escalates to `Full`.
pub(crate) fn run_at_acked(ui: &mut Ui, size: UVec2, record: impl FnMut(&mut Ui)) {
    let display = Display::from_physical(size, 1.0);
    let out = ui.run_frame(display, Duration::ZERO, record);
    out.frame_state.mark_submitted();
}

pub(crate) fn ui_at(size: UVec2) -> Ui {
    let mut ui = Ui::new();
    begin(&mut ui, size);
    ui
}

pub(crate) fn ui_with_text(size: UVec2) -> Ui {
    let mut ui = new_ui_text();
    begin(&mut ui, size);
    ui
}

pub(crate) fn new_ui_text() -> Ui {
    // Cosmic-text's `FontSystem` parses bundled font bytes on
    // construction — expensive (multiple ms). Tests that loop over
    // many widths or sizes call `ui_with_text` per iteration; sharing
    // one `TextShaper` per thread amortizes the parse across the
    // thread's lifetime and cuts cross-driver test runtime ~10×.
    // Fine because cosmic state across tests is just a glyph cache —
    // tests assert on layout output, not cache contents.
    thread_local! {
        static SHARED: TextShaper = TextShaper::with_bundled_fonts();
    }
    Ui::with_text(SHARED.with(|c| c.clone()))
}

/// Wrap the unit-under-test inside an outer `Fill` HStack so the panel
/// under test can express its own measured size — `ui.layout` always
/// forces the root to the surface rect, which would mask Hug/Fixed
/// sizing on the unit-under-test. Returns the inner node.
pub(crate) fn under_outer<F: FnMut(&mut Ui) -> NodeId>(
    ui: &mut Ui,
    surface: UVec2,
    mut f: F,
) -> NodeId {
    let mut inner = None;
    run_at(ui, surface, |ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                inner = Some(f(ui));
            });
    });
    inner.unwrap()
}

pub(crate) fn click_at(ui: &mut Ui, pos: Vec2) {
    ui.on_input(InputEvent::PointerMoved(pos));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
}

pub(crate) fn press_at(ui: &mut Ui, pos: Vec2) {
    ui.on_input(InputEvent::PointerMoved(pos));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
}

pub(crate) fn release_left(ui: &mut Ui) {
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
}

pub(crate) fn encode_cmds(ui: &Ui) -> RenderCmdBuffer {
    encode_cmds_filtered(ui, None)
}

pub(crate) fn encode_cmds_filtered(ui: &Ui, filter: Option<Rect>) -> RenderCmdBuffer {
    encode_cmds_with_region(ui, filter.map(DamageRegion::from).as_ref())
}

/// Multi-rect variant of [`encode_cmds_filtered`]. Builds a region
/// from `rects` (each fed through [`DamageRegion::add`] so the merge
/// policy applies) and encodes against it. Empty slice ⇒ no filter.
pub(crate) fn encode_cmds_with_rects(ui: &Ui, rects: &[Rect]) -> RenderCmdBuffer {
    let region = if rects.is_empty() {
        None
    } else {
        let mut r = DamageRegion::default();
        for rect in rects {
            r.add(*rect);
        }
        Some(r)
    };
    encode_cmds_with_region(ui, region.as_ref())
}

fn encode_cmds_with_region(ui: &Ui, region: Option<&DamageRegion>) -> RenderCmdBuffer {
    // Fresh `Encoder` per call → empty cache, every encode is a cold
    // build. Tests that want to verify cache-replay output use
    // `ui.frontend.encoder.cmds()` instead.
    let mut encoder = Encoder::default();
    encoder.encode(
        &ui.forest,
        &ui.layout,
        &ui.cascades.result,
        region,
        ui.display.logical_rect(),
    );
    std::mem::take(&mut encoder.cmds)
}
