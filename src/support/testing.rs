//! Shared helpers for tests across the crate.

#![cfg(test)]

use crate::Ui;
use crate::common::frame_arena::new_handle;
use crate::forest::element::Configure;
use crate::forest::shapes::record::ShapeRecord;
use crate::renderer::frontend::Frontend;
use crate::text::TextShaper;

/// Test-only `Ui` with the **mono-fallback** shaper and a fresh
/// private frame arena. Replaces the deleted `impl Default for Ui`;
/// many tests rely on the predictable 8 px/char widths that the mono
/// fallback gives. Tests that need real text shaping use
/// [`new_ui_text`] instead.
pub(crate) fn new_ui() -> Ui {
    Ui::new(TextShaper::default(), new_handle())
}

/// Test-only `Ui` with a thread-shared cosmic shaper. Parsed once per
/// thread (font-database build is multi-ms) so calling this in a
/// tight loop pays the per-thread cost once. Cosmic state across
/// tests is just a glyph cache — fine for layout-output assertions.
pub(crate) fn new_ui_text() -> Ui {
    thread_local! {
        static SHARED: TextShaper = TextShaper::with_bundled_fonts();
    }
    Ui::new(SHARED.with(|c| c.clone()), new_handle())
}

/// Test-only `Frontend` with a private (disjoint-from-Ui) frame
/// arena. Production wiring goes through [`crate::Host::new`] which
/// builds both `Ui` and `Frontend` with the same shared handle.
/// Fine for tests that don't push user-mesh or polyline shapes —
/// rect-only fixtures don't read from the arena.
pub(crate) fn new_frontend() -> Frontend {
    Frontend::new(new_handle())
}
#[allow(unused_imports)]
use crate::forest::tree::Layer;
use crate::forest::tree::{NodeId, Tree, TreeItem};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::rect::Rect;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::encoder::encode;
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

/// Drive one full frame through [`Ui::frame`] at the given surface
/// size. Time is frozen at zero — tests that exercise animation pass
/// `now` themselves via `frame` directly.
pub(crate) fn run_at(ui: &mut Ui, size: UVec2, record: impl FnMut(&mut Ui)) {
    let display = Display::from_physical(size, 1.0);
    ui.frame(display, Duration::ZERO, &mut (), record);
}

/// Same as [`run_at`] but additionally marks the frame as submitted.
/// Tests that drive frames without a real `WgpuBackend::submit` need
/// this — otherwise the auto-rewind kicks in and every subsequent
/// frame's damage escalates to `Full`.
pub(crate) fn run_at_acked(ui: &mut Ui, size: UVec2, record: impl FnMut(&mut Ui)) {
    let display = Display::from_physical(size, 1.0);
    ui.frame(display, Duration::ZERO, &mut (), record);
    ui.frame_state.mark_submitted();
}

/// Construct a `Ui` and stamp the display dimensions, but do not yet
/// drive a frame. For tests that introspect `ui.display` before
/// recording or pre-seed `Ui` state.
pub(crate) fn ui_at(size: UVec2) -> Ui {
    let mut ui = crate::support::testing::new_ui();
    ui.display = Display::from_physical(size, 1.0);
    ui
}

pub(crate) fn ui_with_text(size: UVec2) -> Ui {
    let mut ui = new_ui_text();
    ui.display = Display::from_physical(size, 1.0);
    ui
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

pub(crate) fn secondary_click_at(ui: &mut Ui, pos: Vec2) {
    ui.on_input(InputEvent::PointerMoved(pos));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Right));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Right));
}

pub(crate) fn encode_cmds(ui: &Ui) -> RenderCmdBuffer {
    encode_cmds_filtered(ui, None)
}

pub(crate) fn encode_cmds_filtered(ui: &Ui, filter: Option<Rect>) -> RenderCmdBuffer {
    encode_cmds_with_region(ui, filter.map(DamageRegion::from))
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
    encode_cmds_with_region(ui, region)
}

fn encode_cmds_with_region(ui: &Ui, region: Option<DamageRegion>) -> RenderCmdBuffer {
    // Fresh `RenderCmdBuffer` per call → cold build. `None` ⇒
    // `Damage::Full` (no filter), `Some(region)` ⇒ `Damage::Partial`.
    use crate::ui::damage::Damage;
    let damage = match region {
        Some(r) => Damage::Partial(r),
        None => Damage::Full,
    };
    let mut cmds = RenderCmdBuffer::default();
    encode(ui, damage, &mut cmds);
    cmds
}
