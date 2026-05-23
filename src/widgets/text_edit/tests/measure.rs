//! Measurement stability across focus transitions. A `Hug`-width
//! editor's desired width must not snap to zero when it gains focus
//! with an empty buffer — the placeholder shape is recorded with a
//! transparent brush in that state so the leaf still has content to
//! measure.
//!
//! See `text_edit::mod.rs::show` ("Text or placeholder…" block) and
//! `support::place_axis` for the two invariants this test guards.

use super::*;
use crate::forest::Layer;
use crate::forest::tree::NodeId;

const SIZE: UVec2 = UVec2::new(400, 80);
const PLACEHOLDER: &str = "type something here";

fn frame(ui: &mut Ui, buf: &mut String) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                node = Some(
                    TextEdit::new(buf)
                        .id(WidgetId::from_hash("editor"))
                        .placeholder(PLACEHOLDER)
                        .size((Sizing::Hug, Sizing::Hug))
                        .show(ui)
                        .node(),
                );
            });
    };
    ui.run_at_acked(SIZE, &mut record);
    node.unwrap()
}

/// Empty-buffer editor inside a `Hug` parent: width when focused must
/// equal width when unfocused. Previously the focused branch skipped
/// recording the placeholder shape (only the buffer, which is empty,
/// was recorded), so the leaf measured to zero content and the parent
/// snapped to the editor's `min_size` floor — visible jitter every
/// click.
#[test]
fn empty_editor_width_is_stable_across_focus() {
    let mut ui = Ui::for_test();
    let mut buf = String::new();
    let id = WidgetId::from_hash("editor");

    // Two unfocused warm-up frames so layout cache stabilises.
    frame(&mut ui, &mut buf);
    let node = frame(&mut ui, &mut buf);
    let w_unfocused = ui.layout[Layer::Main].rect[node.idx()].size.w;

    // Focus the editor and re-measure.
    ui.request_focus(Some(id));
    frame(&mut ui, &mut buf);
    let node = frame(&mut ui, &mut buf);
    let w_focused = ui.layout[Layer::Main].rect[node.idx()].size.w;

    assert!(
        w_unfocused > 0.0,
        "unfocused empty editor with a placeholder should have positive width, got {w_unfocused}",
    );
    assert!(
        (w_focused - w_unfocused).abs() < 1e-3,
        "focus must not change desired width; unfocused={w_unfocused} focused={w_focused}",
    );
}
