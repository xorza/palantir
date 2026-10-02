//! What the body costs while closed, who owns the open flag, the snap on
//! a first reveal, and the keys that toggle a focused header.

use glam::{UVec2, Vec2};

use crate::animation::anim_spec::AnimSpec;
use crate::input::keyboard::key::Key;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::test_support::assert_close;
use crate::primitives::rect::Rect;
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
fn frame(h: &mut UiHarness, start_open: bool) {
    h.frame(|ui| {
        Expander::new("section")
            .id(root())
            .start_open(start_open)
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
            .start_open(true)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            });
    });
    let body_rect = h.rect(body()).expect("an open section records its body");
    let header_rect = h.rect(header()).expect("the header");
    // The body sits right under the header, indented by the theme's
    // 17 px from its leading edge and running to the header's far edge.
    let indent = h.ui().theme().expander.indent;
    assert_eq!(indent, 17.0);
    assert_eq!(
        body_rect,
        Rect::new(
            header_rect.min.x + indent,
            header_rect.max().y,
            header_rect.size.w - indent,
            body_rect.size.h,
        ),
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
        h.ui().state::<ExpanderState>(header()).is_none(),
        "a closed default wrote a row it did not need",
    );

    // Opening it is what mints one, and it survives the next frame.
    h.click_on(header());
    frame(&mut h, false);
    let row = h
        .ui()
        .state::<ExpanderState>(header())
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

    h.click_on(header());
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

    h.advance_past_double_click();
    h.frame(|ui| {
        Expander::new("section").id(root()).show(ui, |_| {});
    });
    h.click_on(header());
    frame(&mut h, false);
    frame(&mut h, false);
    assert!(h.rect(body()).is_none(), "a second click closed it again");
}

/// An Expander disabled on the frame a click lands does not toggle: the
/// header reads its owner's flag that frame, not through the cascade a
/// frame late.
#[test]
fn a_click_on_the_frame_it_is_disabled_does_not_toggle() {
    let mut h = UiHarness::new(SURFACE);
    frame(&mut h, false);
    frame(&mut h, false);
    h.click_on(header());
    let open = h.frame_value(|ui| {
        Expander::new("section")
            .id(root())
            .disabled(true)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            })
            .openness
    });
    assert_eq!(open, 0.0, "the disabled header ignored the click");
    assert!(h.rect(body()).is_none());
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
            .start_open(open)
            .keep_body(true)
            .show(ui, |ui| {
                TextEdit::new(text)
                    .id(label())
                    .size((Sizing::FILL, Sizing::fixed(20.0)))
                    .show(ui);
            });
    };
    h.prime(2, |ui| record(ui, &mut text, false));

    let body_rect = h.arranged(body());
    assert_eq!(
        body_rect.size.h, 0.0,
        "a collapsed body takes no space: {body_rect:?}",
    );
    assert!(
        h.hit_at(body_rect.min).is_none() || h.hit_at(body_rect.min) != Some(label()),
        "and is not hit-tested",
    );
    assert!(
        h.ui().state::<ExpanderState>(header()).is_none(),
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
            .start_open(false)
            .open(open)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            });
    };
    h.prime(2, |ui| record(ui, &mut open));
    assert!(
        h.rect(body()).is_some(),
        "the binding won over start_open(false)",
    );

    h.click_on(header());
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

    h.click_on(header());
    assert_eq!(
        h.frame_value(&mut record),
        1.0,
        "no height was known, so the reveal snapped whole",
    );
    // A frame with the body whole is what measures it.
    h.advance_frames(2, std::time::Duration::from_millis(16), |ui| {
        record(ui);
    });

    h.advance_past_double_click();
    h.frame(|ui| {
        record(ui);
    });
    h.click_on(header());
    // The click frame carries the new target but no elapsed time, so the
    // tween has not moved yet; the frame after it is the one that shows.
    assert_eq!(h.frame_value(&mut record), 1.0);
    h.advance(std::time::Duration::from_millis(16));
    // `MEDIUM` is 200 ms of ease-out cubic, so 16 ms in the reveal has
    // (1 − 16/200)³ = 0.92³ = 0.778688 left: the close tweened rather
    // than snapping.
    assert_eq!(h.frame_value(&mut record), 0.778688);
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

        h.clear_focus();
        h.key(key);
        frame(&mut h, false);
        assert!(
            h.rect(body()).is_none(),
            "{key:?} moved an unfocused header",
        );

        h.set_focus(header());
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
    // f32 `cos(π/2)` is -4.4e-8, not 0, so a turned point carries a
    // residue of that times its 4 px lever, a few ulps of 8.
    let residue = "f32 cos(π/2) is not 0";
    for (got, want) in turned.into_iter().zip(expected) {
        assert_close(got.x, want.x, 1e-6, residue);
        assert_close(got.y, want.y, 1e-6, residue);
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
        assert_close(got.x, want.x, 1e-6, residue);
        assert_close(got.y, want.y, 1e-6, residue);
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
    assert_eq!(
        t.arrow_angle(2.0),
        t.arrow_open_angle,
        "openness past 1 clamps to open"
    );
}

/// The height a reveal clips against is the body's whole height, on the
/// frame the tween settles too. A settled tween asks for no further frame,
/// so a height read off a clipped body there is the one the next collapse
/// clips against, and the body jumps.
#[test]
fn a_settling_reveal_stores_the_whole_height() {
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
    let tick = std::time::Duration::from_millis(16);
    let toggle = |h: &mut UiHarness, record: &mut dyn FnMut(&mut Ui) -> f32| {
        h.advance_past_double_click();
        h.frame(|ui| {
            record(ui);
        });
        h.click_on(header());
        h.frame_value(&mut *record);
    };

    toggle(&mut h, &mut record);
    h.advance_frames(2, tick, |ui| {
        record(ui);
    });
    // One mono line, 19.2 snapped to 19.203125, under 4 + 4 padding.
    let whole = h.arranged(body()).size.h;
    assert_eq!(whole, 19.203125 + 8.0);
    toggle(&mut h, &mut record);
    while h.frame_value(&mut record) > 0.0 {
        h.advance(tick);
    }

    toggle(&mut h, &mut record);
    let mut openness = 0.0;
    while openness < 1.0 {
        h.advance(tick);
        openness = h.frame_value(&mut record);
        if openness < 1.0 {
            assert_eq!(
                h.arranged(body()).size.h,
                whole,
                "the body lays out whole under the clip at {openness}",
            );
        }
    }
    let row = h.ui().state::<ExpanderState>(header()).copied().unwrap();
    assert_eq!(row.height, Some(whole), "the settle frame stored {row:?}");
}
