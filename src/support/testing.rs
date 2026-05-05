//! Shared helpers for tests across the crate.

#![cfg(test)]

use crate::Ui;
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::rect::Rect;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::encoder::Encoder;
use crate::text::{cosmic::CosmicMeasure, share};
use crate::tree::NodeId;
use crate::tree::element::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

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
    let mut ui = Ui::new();
    ui.set_cosmic(share(CosmicMeasure::with_bundled_fonts()));
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
    // `ui.pipeline.frontend.encoder.cmds()` instead.
    let mut encoder = Encoder::default();
    encoder.encode(
        &ui.tree,
        &ui.pipeline.layout.result,
        &ui.cascades.result,
        filter,
    );
    std::mem::take(&mut encoder.cmds)
}
