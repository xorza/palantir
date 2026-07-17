//! Measurement stability across focus transitions. A `Hug`-width
//! editor's desired width must not snap to zero when it gains focus
//! with an empty buffer — the placeholder shape is recorded with a
//! transparent brush in that state so the leaf still has content to
//! measure.
//!
//! See `text_edit::mod.rs::show` ("Text or placeholder…" block) and
//! `support::arrange_axis` for the two invariants this test guards.

use crate::forest::layer::Layer;
use crate::forest::tree::node::NodeId;
use crate::widgets::text_edit::tests::*;

const SIZE: UVec2 = UVec2::new(400, 80);
const PLACEHOLDER: &str = "type something here";

fn frame(ui: &mut Ui, buf: &mut String) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                node = Some(
                    TextEdit::new(buf)
                        .id(WidgetId::from_hash("editor"))
                        .placeholder(PLACEHOLDER)
                        .size((Sizing::HUG, Sizing::HUG))
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

const LONG: &str = "the quick brown fox jumps over the lazy dog";

/// Build one `container_w`-wide `Fixed` hstack holding a single-line
/// editor sized `editor_w` on the main axis. Two frames so the layout
/// cache stabilises, matching `frame` above.
fn sized_editor(ui: &mut Ui, buf: &mut String, container_w: f32, editor_w: Sizing) -> NodeId {
    let mut node: Option<NodeId> = None;
    let mut record = |ui: &mut Ui| {
        Panel::hstack()
            .auto_id()
            .size((Sizing::fixed(container_w), Sizing::fixed(40.0)))
            .show(ui, |ui| {
                node = Some(
                    TextEdit::new(buf)
                        .id(WidgetId::from_hash("editor"))
                        .size((editor_w, Sizing::fixed(40.0)))
                        .show(ui)
                        .node(),
                );
            });
    };
    ui.run_at_acked(UVec2::new(2100, 200), &mut record);
    ui.run_at_acked(UVec2::new(2100, 200), &mut record);
    node.unwrap()
}

/// A `Fill`-width single-line editor must shrink *below* its own text
/// content when its container is narrower than the text, and stretch to
/// exactly fill a container wider than the text. The editor clips
/// (`ClipMode::Rect`) and scrolls, so its recorded text uses
/// `TextWrap::Scroll` — zero min-content — and the Fill floor is the
/// editor's padding, not the buffer's natural width. Before this fix
/// `TextWrap::SingleLine` reported the full text width as min-content, so
/// the Fill floor froze at the text width: the field refused to get
/// smaller than its content and overflowed any narrower container.
///
/// A `Hug`-width editor still hugs its buffer (its own `min_size.w`
/// reservation floors it) — checked here as the natural-width baseline.
#[test]
fn fill_width_editor_shrinks_below_text_content() {
    const NARROW_W: f32 = 120.0;
    let mut ui = Ui::for_test();

    // Natural text width: a Hug editor in a wide container hugs its buffer.
    let mut buf = LONG.to_string();
    let hug = sized_editor(&mut ui, &mut buf, 2000.0, Sizing::HUG);
    let text_w = ui.layout[Layer::Main].rect[hug.idx()].size.w;
    assert!(
        text_w > NARROW_W,
        "fixture requires the text ({text_w}) to be wider than the narrow container ({NARROW_W})",
    );

    // Fill editor in a narrow container shrinks to fill it, well below the text.
    let mut buf = LONG.to_string();
    let fill = sized_editor(&mut ui, &mut buf, NARROW_W, Sizing::FILL);
    let fill_w = ui.layout[Layer::Main].rect[fill.idx()].size.w;
    assert!(
        (fill_w - NARROW_W).abs() < 0.5,
        "sole Fill child must stretch to its {NARROW_W}px container, got {fill_w}",
    );
    assert!(
        fill_w < text_w,
        "Fill editor ({fill_w}) must be narrower than its text content ({text_w})",
    );
}
