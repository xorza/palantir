//! Reaching a widget by id: the rect that answers, and the occlusion that
//! refuses.

use crate::layout::types::sizing::Sizing;
use crate::primitives::size::Size;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::harness::tests::support::{INSIDE, OUTSIDE, SURFACE, button, target};
use crate::ui::harness::*;
use crate::widgets::button::Button;
use crate::widgets::panel::Panel;

#[test]
fn a_press_outside_the_widget_does_not_click_it() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);
    harness.click_at(OUTSIDE);
    assert!(!harness.response_in(target(), button).left.clicked());
}

#[test]
fn hit_at_reports_what_the_pointer_would_reach() {
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    assert_eq!(
        harness.hit_at(INSIDE),
        Some(target()),
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
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, button);

    let rect = harness.rect(target()).expect("the button arranged");
    // A 100×40 button, first child of an origin-anchored hstack.
    assert_eq!(rect.size.w, 100.0);
    assert_eq!(rect.size.h, 40.0);
    assert_eq!(harness.center_of(target()), rect.center());
    assert_eq!(harness.hit_at(harness.center_of(target())), Some(target()));

    // Addressing the widget instead of its coordinates lands the same
    // click as the hand-computed `INSIDE`.
    harness.click_on(target());
    assert!(harness.response_in(target(), button).left.clicked());
}

#[test]
fn addressing_a_widget_refuses_to_click_through_something_on_top() {
    // The whole reason the `_on` helpers check rather than just aiming:
    // a covered widget's center belongs to whatever is over it, so an
    // unchecked `click_on` would report success while the event went
    // somewhere else entirely.
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

    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, stacked);

    // Both occupy the same rect, so their centers coincide — geometry
    // alone cannot tell them apart.
    assert_eq!(harness.center_of(under), harness.center_of(over));

    harness.click_on(over);
    assert!(
        harness.response_in(over, stacked).left.clicked(),
        "the topmost widget is reachable by id",
    );

    let covered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut harness = UiHarness::new(SURFACE);
        harness.prime(2, stacked);
        harness.click_on(under);
    }));
    assert!(
        covered.is_err(),
        "a covered widget must refuse, not silently click the cover",
    );
}

#[test]
fn layout_rect_is_pre_transform_and_rect_is_what_the_pointer_hits() {
    // The two accessors agree until an ancestor transforms or clips, and
    // disagree only there — so a test that reaches for the wrong one
    // passes everywhere except the scroll and canvas cases. Pinning both
    // sides here is what keeps that from being discovered the hard way.
    use crate::primitives::translate_scale::TranslateScale;

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
    let mut harness = UiHarness::new(SURFACE);
    harness.prime(2, scaled);

    let arranged = harness.layout_rect(inner).expect("the button arranged");
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

    // And it is exactly the layout pass output — the same value a test
    // gets by carrying a `NodeId` and indexing by hand.
    let node = harness.node_for_widget_id(inner);
    assert_eq!(
        arranged,
        harness.ui.layout[Layer::Main].rect[node.idx()],
        "layout_rect == the arrange output for that widget's node",
    );

    // Untransformed, the distinction vanishes — which is why picking the
    // wrong one is invisible in most of the suite.
    let mut plain = UiHarness::new(SURFACE);
    plain.prime(2, button);
    assert_eq!(plain.layout_rect(target()), plain.rect(target()));
}
