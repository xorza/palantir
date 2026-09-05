//! What the body costs while closed, who owns the open flag, the snap on
//! a first reveal, and the keys that toggle a focused header.

use glam::{UVec2, Vec2};

use crate::animation::anim_spec::AnimSpec;
use crate::input::keyboard::key::Key;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::ui::harness::UiHarness;
use crate::widgets::arrow::Arrow;
use crate::widgets::configure::Configure;
use crate::widgets::expander::{Expander, ExpanderState};
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::expander::ExpanderTheme;
use crate::widgets::theme::widget_look::theme_slot::SlotDefaults;

const SURFACE: UVec2 = UVec2::new(320, 240);

fn root() -> WidgetId {
    WidgetId::from_hash("test.expander")
}

fn header() -> WidgetId {
    root().with("header")
}

fn body() -> WidgetId {
    root().with("body")
}

fn label() -> WidgetId {
    root().with("body").with("inner")
}

/// One frame of a plain expander over a single label.
fn frame(h: &mut UiHarness, default_open: bool) {
    h.frame(|ui| {
        Expander::new("section")
            .id(root())
            .default_open(default_open)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            });
    });
}

/// A closed section records nothing below its header — which is the
/// whole reason the control exists, and the reason its body's state is
/// swept.
#[test]
fn a_closed_body_is_not_recorded_and_an_open_one_is() {
    let mut h = UiHarness::new(SURFACE);
    h.prime(2, |ui| {
        Expander::new("section").id(root()).show(ui, |ui| {
            Text::new("body").id(label()).show(ui);
        });
    });
    assert!(h.rect(header()).is_some(), "the header always records");
    assert!(h.rect(body()).is_none(), "a closed section records no body");
    assert!(h.rect(label()).is_none(), "and nothing inside it");

    h.prime(2, |ui| {
        Expander::new("section")
            .id(root())
            .default_open(true)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            });
    });
    let body_rect = h.rect(body()).expect("an open section records its body");
    let header_rect = h.rect(header()).expect("the header");
    assert!(
        body_rect.min.y >= header_rect.max().y - 1.0,
        "the body sits under the header: {body_rect:?} against {header_rect:?}",
    );
    assert!(
        body_rect.min.x > header_rect.min.x,
        "and is indented from its leading edge: {body_rect:?} in {header_rect:?}",
    );
    assert!(h.rect(label()).is_some(), "the body's own content records");
}

/// A section nobody has touched keeps no cross-frame row at all — the
/// probe-don't-insert path `ComboBox` takes for its own open flag.
#[test]
fn an_untouched_section_mints_no_state_row() {
    let mut h = UiHarness::new(SURFACE);
    frame(&mut h, false);
    frame(&mut h, false);
    assert!(
        h.ui().try_state::<ExpanderState>(header()).is_none(),
        "a closed default wrote a row it did not need",
    );

    // Opening it is what mints one, and it survives the next frame.
    let at = h.center_of(header());
    h.click_at(at);
    frame(&mut h, false);
    let row = h
        .ui()
        .try_state::<ExpanderState>(header())
        .copied()
        .expect("the toggle wrote a row");
    assert!(row.open, "the click opened it");
}

/// A click toggles, and the body it revealed records on the same frame —
/// the header resolves the click before it records anything below it.
#[test]
fn a_click_toggles_and_reveals_on_the_same_frame() {
    let mut h = UiHarness::new(SURFACE);
    frame(&mut h, false);
    frame(&mut h, false);
    assert!(h.rect(body()).is_none());

    let at = h.center_of(header());
    h.click_at(at);
    let open = h.frame_value(|ui| {
        Expander::new("section")
            .id(root())
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            })
            .openness
    });
    assert_eq!(open, 1.0, "the reveal snapped, having no height to tween");
    assert!(
        h.rect(body()).is_some(),
        "the body recorded on the frame the click landed",
    );

    h.advance_past_double_click(|ui| {
        Expander::new("section").id(root()).show(ui, |_| {});
    });
    let at = h.center_of(header());
    h.click_at(at);
    frame(&mut h, false);
    frame(&mut h, false);
    assert!(h.rect(body()).is_none(), "a second click closed it again");
}

/// `keep_body` trades a record per frame for the state inside it. The
/// collapsed body takes no space and paints nothing, but its ids stay
/// live, so a `TextEdit` in there still holds its text.
#[test]
fn keep_body_records_a_collapsed_body_and_holds_its_state() {
    let mut h = UiHarness::new(SURFACE);
    let mut text = String::from("draft");
    let record = |ui: &mut Ui, text: &mut String, open: bool| {
        Expander::new("section")
            .id(root())
            .default_open(open)
            .keep_body(true)
            .show(ui, |ui| {
                TextEdit::new(text)
                    .id(label())
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
            });
    };
    h.prime(2, |ui| record(ui, &mut text, false));

    let body_rect = h.layout_rect(body()).expect("a kept body still records");
    assert_eq!(
        body_rect.size.h, 0.0,
        "a collapsed body takes no space: {body_rect:?}",
    );
    assert!(
        h.hit_at(body_rect.min).is_none() || h.hit_at(body_rect.min) != Some(label()),
        "and is not hit-tested",
    );
    assert!(
        h.ui().try_state::<ExpanderState>(header()).is_none(),
        "keeping the body is not itself a toggle",
    );
}

/// The binding wins over the default, and every toggle is written back
/// through it.
#[test]
fn a_bound_flag_is_read_and_written() {
    let mut h = UiHarness::new(SURFACE);
    let mut open = true;
    let record = |ui: &mut Ui, open: &mut bool| {
        Expander::new("section")
            .id(root())
            .default_open(false)
            .open(open)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            });
    };
    h.prime(2, |ui| record(ui, &mut open));
    assert!(
        h.rect(body()).is_some(),
        "the binding won over default_open(false)",
    );

    let at = h.center_of(header());
    h.click_at(at);
    h.frame(|ui| record(ui, &mut open));
    assert!(!open, "the toggle was written back through the binding");

    // The caller's own write is read on the next frame.
    open = true;
    h.frame(|ui| record(ui, &mut open));
    h.frame(|ui| record(ui, &mut open));
    assert!(h.rect(body()).is_some(), "the caller reopened it");
}

/// The first reveal snaps because there is no measured height to tween
/// against; every one after it animates, which is what the remembered
/// height buys.
#[test]
fn the_first_reveal_snaps_and_the_next_one_animates() {
    let base = ExpanderTheme::default();
    let theme = ExpanderTheme {
        defaults: SlotDefaults {
            anim: Some(AnimSpec::MEDIUM),
            ..base.defaults
        },
        ..base
    };
    let mut h = UiHarness::new(SURFACE);
    let mut record = |ui: &mut Ui| {
        Expander::new("section")
            .id(root())
            .style(&theme)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            })
            .openness
    };
    h.prime(2, |ui| {
        record(ui);
    });

    let at = h.center_of(header());
    h.click_at(at);
    assert_eq!(
        h.frame_value(&mut record),
        1.0,
        "no height was known, so the reveal snapped whole",
    );
    // A frame with the body whole is what measures it.
    h.advance_frames(2, std::time::Duration::from_millis(16), |ui| {
        record(ui);
    });

    h.advance_past_double_click(|ui| {
        record(ui);
    });
    let at = h.center_of(header());
    h.click_at(at);
    // The click frame carries the new target but no elapsed time, so the
    // tween has not moved yet; the frame after it is the one that shows.
    assert_eq!(h.frame_value(&mut record), 1.0);
    h.advance(std::time::Duration::from_millis(16));
    let closing = h.frame_value(&mut record);
    assert!(
        closing > 0.0 && closing < 1.0,
        "the close tweened against the remembered height, got {closing}",
    );
}

/// Space and Enter toggle a focused header, and nothing else does. The
/// header claims `KeyClass::Text` while it holds focus, which is the
/// same claim a text field makes — and right for a target that is not
/// one.
#[test]
fn space_and_enter_toggle_a_focused_header() {
    for key in [Key::Char(' '), Key::Enter] {
        let mut h = UiHarness::new(SURFACE);
        frame(&mut h, false);
        frame(&mut h, false);

        h.request_focus(None);
        h.key(key);
        frame(&mut h, false);
        assert!(
            h.rect(body()).is_none(),
            "{key:?} moved an unfocused header",
        );

        h.request_focus(Some(header()));
        frame(&mut h, false);
        h.key(key);
        frame(&mut h, false);
        assert!(h.rect(body()).is_some(), "{key:?} opened a focused header");
    }
}

/// The arrow is one shape at two sizes, and a quarter turn takes the
/// dropdown's `v` to the disclosure `>`.
#[test]
fn a_quarter_turn_points_the_arrow_at_the_label() {
    let c = Arrow {
        size: Vec2::new(8.0, 8.0),
    };
    assert_eq!(
        c.points(),
        [
            Vec2::new(0.0, 0.0),
            Vec2::new(4.0, 8.0),
            Vec2::new(8.0, 0.0)
        ],
        "the tip is the middle point, on the bottom edge",
    );

    let turned = c.rotated(-std::f32::consts::FRAC_PI_2);
    let expected = [
        Vec2::new(0.0, 8.0),
        Vec2::new(8.0, 4.0),
        Vec2::new(0.0, 0.0),
    ];
    for (got, want) in turned.into_iter().zip(expected) {
        assert!(
            (got - want).length() < 1e-4,
            "a quarter turn about the centre: got {turned:?}, want {expected:?}",
        );
    }

    // Rounded by 1: the same turn on a 6 px arrow one px in from every
    // edge, so the dilated shape's extents are the box's again.
    let rounded = c.rounded(1.0, -std::f32::consts::FRAC_PI_2);
    let expected = [
        Vec2::new(1.0, 7.0),
        Vec2::new(7.0, 4.0),
        Vec2::new(1.0, 1.0),
    ];
    for (got, want) in rounded.into_iter().zip(expected) {
        assert!(
            (got - want).length() < 1e-4,
            "vertices one radius in: got {rounded:?}, want {expected:?}",
        );
    }
    assert_eq!(
        c.rounded(0.0, 0.0),
        c.points(),
        "a sharp triangle is the arrow itself"
    );

    // Both angles the theme names, resolved through it.
    let t = ExpanderTheme::default();
    assert_eq!(t.arrow_angle(0.0), t.arrow_closed_angle);
    assert_eq!(t.arrow_angle(1.0), t.arrow_open_angle);
    assert!(t.arrow_angle(2.0).abs() <= t.arrow_closed_angle.abs());
}
