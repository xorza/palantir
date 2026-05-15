//! Shared helpers for in-tree tests.

#![cfg(test)]

use crate::Ui;
use crate::common::frame_arena::new_handle;
use crate::forest::element::Configure;
use crate::forest::shapes::record::ShapeRecord;
use crate::forest::tree::{NodeId, Tree, TreeItem};
use crate::input::{InputEvent, PointerButton};
use crate::layout::types::{display::Display, sizing::Sizing};
use crate::primitives::rect::Rect;
use crate::renderer::frontend::Frontend;
use crate::renderer::frontend::cmd_buffer::RenderCmdBuffer;
use crate::renderer::frontend::encoder::encode;
use crate::text::TextShaper;
use crate::ui::damage::region::DamageRegion;
use crate::ui::frame_report::RenderPlan;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};
use std::time::Duration;

/// `Ui` with the mono-fallback shaper — predictable 8 px/char widths.
pub(crate) fn new_ui() -> Ui {
    Ui::new(TextShaper::default(), new_handle())
}

/// `Ui` with a thread-shared cosmic shaper (font DB built once per thread).
pub(crate) fn new_ui_text() -> Ui {
    thread_local! {
        static SHARED: TextShaper = TextShaper::with_bundled_fonts();
    }
    Ui::new(SHARED.with(|c| c.clone()), new_handle())
}

/// `Frontend` with a private (disjoint-from-Ui) frame arena.
pub(crate) fn new_frontend() -> Frontend {
    Frontend::new(new_handle())
}

/// Direct shapes of `node`, including parent-pushed sub-rects interleaved between children.
pub(crate) fn shapes_of(tree: &Tree, node: NodeId) -> impl Iterator<Item = &ShapeRecord> + '_ {
    tree.tree_items(node).filter_map(|item| match item {
        TreeItem::ShapeRecord(_, s) => Some(s),
        TreeItem::Child(_) => None,
    })
}

/// One frame at `size`, time frozen at zero.
pub(crate) fn run_at(ui: &mut Ui, size: UVec2, record: impl FnMut(&mut Ui)) {
    let display = Display::from_physical(size, 1.0);
    ui.frame(display, Duration::ZERO, &mut (), record);
}

/// `run_at` then mark the frame as submitted (suppress next-frame auto-rewind to `Full`).
pub(crate) fn run_at_acked(ui: &mut Ui, size: UVec2, record: impl FnMut(&mut Ui)) {
    run_at(ui, size, record);
    ui.frame_state.mark_submitted();
}

/// `Ui` pre-stamped with display dimensions; no frame driven yet.
pub(crate) fn ui_at(size: UVec2) -> Ui {
    let mut ui = new_ui();
    ui.display = Display::from_physical(size, 1.0);
    ui
}

pub(crate) fn ui_with_text(size: UVec2) -> Ui {
    let mut ui = new_ui_text();
    ui.display = Display::from_physical(size, 1.0);
    ui
}

/// Wrap UUT inside a Fill HStack so the panel can express its own measured size.
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

/// Multi-rect variant; each rect is fed through `DamageRegion::add` so merge policy applies.
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
    let clear = ui.theme.window_clear;
    let plan = match region {
        Some(region) => RenderPlan::Partial { clear, region },
        None => RenderPlan::Full { clear },
    };
    let mut cmds = RenderCmdBuffer::default();
    encode(ui, plan, &mut cmds);
    cmds
}
