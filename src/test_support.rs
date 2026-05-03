//! Shared helpers for tests across the crate.

#![cfg(test)]

use crate::Ui;
use crate::primitives::Display;
use crate::renderer::RenderCmdBuffer;
use crate::text::{CosmicMeasure, share};
use glam::UVec2;

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

pub(crate) fn encode_cmds(ui: &Ui) -> RenderCmdBuffer {
    let mut cmds = RenderCmdBuffer::new();
    crate::renderer::encode(
        ui.tree(),
        ui.layout_engine.result(),
        ui.cascades.result(),
        None,
        &mut cmds,
    );
    cmds
}
