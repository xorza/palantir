//! Shared helpers for tests across the crate.

#![cfg(test)]

use crate::Ui;
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::rect::Rect;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::encoder::Encoder;
use crate::shape::Shape;
use crate::text::SharedCosmic;
#[allow(unused_imports)]
use crate::tree::Layer;
use crate::tree::element::Configure;
use crate::tree::{NodeId, Tree, TreeItem};
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// Direct shapes of `node` — including panels whose direct shapes are
/// interleaved between children (scrollbar overlays, parent-pushed
/// sub-rects). Production callers on leaves use `Tree::leaf_shapes`
/// (direct slice, no per-item branch) instead.
pub(crate) fn shapes_of(tree: &Tree, node: NodeId) -> impl Iterator<Item = &Shape> + '_ {
    tree.tree_items(node).filter_map(|item| match item {
        TreeItem::Shape(s) => Some(s),
        TreeItem::Child(_) => None,
    })
}

pub(crate) fn begin(ui: &mut Ui, size: UVec2) {
    ui.begin_frame(Display::from_physical(size, 1.0));
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
    // one `SharedCosmic` per thread amortizes the parse across the
    // thread's lifetime and cuts cross-driver test runtime ~10×.
    // Fine because cosmic state across tests is just a glyph cache —
    // tests assert on layout output, not cache contents.
    thread_local! {
        static SHARED: SharedCosmic = SharedCosmic::with_bundled_fonts();
    }
    let mut ui = Ui::new();
    SHARED.with(|c| ui.set_cosmic(c.clone()));
    ui
}

/// Wrap the unit-under-test inside an outer `Fill` HStack so the panel
/// under test can express its own measured size — `ui.layout` always
/// forces the root to the surface rect, which would mask Hug/Fixed
/// sizing on the unit-under-test. Returns the inner node.
pub(crate) fn under_outer<F: FnOnce(&mut Ui) -> NodeId>(
    ui: &mut Ui,
    surface: UVec2,
    f: F,
) -> NodeId {
    begin(ui, surface);
    let mut inner = None;
    Panel::hstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            inner = Some(f(ui));
        });
    ui.end_frame();
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
    // Fresh `Encoder` per call → empty cache, every encode is a cold
    // build. Tests that want to verify cache-replay output use
    // `ui.frontend.encoder.cmds()` instead.
    let mut encoder = Encoder::default();
    encoder.encode(
        &ui.forest,
        &ui.layout.result,
        &ui.cascades.result,
        filter,
        ui.display.logical_rect(),
    );
    std::mem::take(&mut encoder.cmds)
}
