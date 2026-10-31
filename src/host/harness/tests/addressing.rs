//! Reaching a widget by id: the rect that answers and the occlusion that refuses.

use crate::host::harness::tests::support::{self, INSIDE, OUTSIDE, SURFACE};
use crate::host::harness::*;
use crate::internals::panic_probe;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;

#[test]
fn a_press_outside_the_widget_does_not_click_it() {
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, support::button);
    harness.click(OUTSIDE);
    assert!(
        !harness
            .response_in(support::target(), support::button)
            .left
            .clicked()
    );
}

#[test]
fn hit_at_reports_what_the_pointer_would_reach() {
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, support::button);

    assert_eq!(
        harness.hit_at(INSIDE),
        Some(support::target()),
        "the button senses hover at its own center",
    );
    assert_eq!(
        harness.hit_at(OUTSIDE),
        None,
        "nothing senses input on bare surface",
    );
}

#[test]
fn center_of_matches_the_arranged_rect() {
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, support::button);

    let rect = harness
        .rect(support::target())
        .expect("the button arranged");
    // A 100×40 button, first child of an origin-anchored hstack.
    assert_eq!(rect.size.w, 100.0);
    assert_eq!(rect.size.h, 40.0);
    assert_eq!(harness.center_of(support::target()), rect.center());
    assert_eq!(
        harness.hit_at(harness.center_of(support::target())),
        Some(support::target())
    );

    // Addressing the widget lands the same click as the hand-computed `INSIDE`.
    harness.click(support::target());
    assert!(
        harness
            .response_in(support::target(), support::button)
            .left
            .clicked()
    );
}

#[test]
fn addressing_a_widget_refuses_to_click_through_something_on_top() {
    // A click by id checks rather than aims: a covered widget's center belongs to whatever is
    // over it, so an unchecked click would report success while the event went elsewhere.
    let under = WidgetId::from_hash("under");
    let over = WidgetId::from_hash("over");
    let stacked = |ui: &mut Ui| {
        Panel::zstack().auto_id().show(ui, |ui| {
            Button::new()
                .id(under)
                .label("under")
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
            Button::new()
                .id(over)
                .label("over")
                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                .show(ui);
        });
    };

    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, stacked);

    // Same rect, so centers coincide; geometry alone can't tell them apart.
    assert_eq!(harness.center_of(under), harness.center_of(over));

    harness.click(over);
    assert!(
        harness.response_in(over, stacked).left.clicked(),
        "the topmost widget is reachable by id",
    );

    // A covered widget must refuse, not click the cover.
    panic_probe::assert_panics_with("lies under another widget", || {
        let mut harness = Harness::mono(SURFACE);
        harness.prime(2, stacked);
        harness.click(under);
    });
}

#[test]
fn layout_rect_is_pre_transform_and_rect_is_what_the_pointer_hits() {
    // The two accessors agree until an ancestor transforms or clips; pinning both sides keeps a wrong pick from passing everywhere but scroll and canvas.
    use crate::primitives::geometry::translate_scale::TranslateScale;

    let inner = WidgetId::from_hash("scaled-inner");
    let scaled = |ui: &mut Ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("scaled-outer"))
            .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
            .transform(TranslateScale::from_scale(2.0))
            .show(ui, |ui| {
                Button::new()
                    .id(inner)
                    .label("x")
                    .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
                    .show(ui);
            });
    };
    let mut harness = Harness::mono(SURFACE);
    harness.prime(2, scaled);

    let arranged = harness.arranged(inner);
    let visible = harness.rect(inner).expect("the button is on screen");
    assert_eq!(
        arranged.size,
        Size::new(40.0, 20.0),
        "layout_rect is the size the layout pass assigned",
    );
    assert_eq!(
        visible.size,
        Size::new(80.0, 40.0),
        "rect carries the ancestor's 2x scale",
    );

    // It is exactly the layout pass output, as carrying a `NodeId` and indexing gives.
    let node = harness.node_for_widget_id(inner);
    assert_eq!(
        arranged,
        harness.ui().layout(Layer::Main).rect[node.idx()],
        "layout_rect == the arrange output for that widget's node",
    );

    // Untransformed, the distinction vanishes, hiding a wrong pick in most of the suite.
    let mut plain = Harness::mono(SURFACE);
    plain.prime(2, support::button);
    assert_eq!(
        plain.layout_rect(support::target()),
        plain.rect(support::target())
    );
}
