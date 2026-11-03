//! What the body costs while closed, who owns the open flag, the first-reveal snap, and the keys that toggle a header.

use glam::{UVec2, Vec2};

use crate::animation::animation_spec::AnimationSpec;
use crate::host::harness::Harness;
use crate::input::keyboard::key::Key;
use crate::primitives::geometry::arrow::Arrow;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain::internals::assert_close;
use crate::theme::expander::ExpanderTheme;
use crate::theme::look::slot_defaults::SlotDefaults;
use crate::ui::Ui;
use crate::widget_core::configure::{Configure, ThemeDefaults};
use crate::widgets::expander::{Expander, ExpanderState};
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use std::f32::consts;
use std::time;

const SURFACE: UVec2 = UVec2::new(320, 240);

fn root() -> WidgetId {
    WidgetId::from_hash("test.expander")
}

fn header() -> WidgetId {
    Expander::header_id(root())
}

fn body() -> WidgetId {
    root().with("body")
}

fn label() -> WidgetId {
    root().with("body").with("inner")
}

/// One frame of a plain expander over a single label; whether its header reports a click.
fn frame(h: &mut Harness, start_open: bool) -> bool {
    h.frame_value(|ui| {
        Expander::new("section")
            .id(root())
            .start_open(start_open)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            })
            .response
            .clicked()
    })
}

/// A closed section records nothing below its header.
#[test]
fn a_closed_body_is_not_recorded_and_an_open_one_is() {
    let mut h = Harness::mono(SURFACE);
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
    let indent = h.ui().theme().expander.indent;
    assert_eq!(indent, 19.0, "the 11 px arrow plus the 8 px gap");
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

    // The section fills the width by default, and a wrapper's `default_size` replaces that.
    assert_eq!(
        h.arranged(root()).unwrap().size.w,
        320.0,
        "FILL across the surface"
    );
    h.prime(2, |ui| {
        Expander::new("section")
            .id(root())
            .default_size((Sizing::fixed(100.0), Sizing::HUG))
            .show(ui, |_| {});
    });
    assert_eq!(
        h.arranged(root()).unwrap().size.w,
        100.0,
        "a default size wins over FILL"
    );
}

/// A section nobody has touched keeps no cross-frame row.
#[test]
fn an_untouched_section_mints_no_state_row() {
    let mut h = Harness::mono(SURFACE);
    frame(&mut h, false);
    frame(&mut h, false);
    assert!(
        h.ui().state::<ExpanderState>(header()).is_none(),
        "a closed default wrote a row it did not need",
    );

    h.click(header());
    frame(&mut h, false);
    let row = h
        .ui()
        .state::<ExpanderState>(header())
        .copied()
        .expect("the toggle wrote a row");
    assert!(row.open, "the click opened it");
}

/// A click toggles and the revealed body records on the same frame.
#[test]
fn a_click_toggles_and_reveals_on_the_same_frame() {
    let mut h = Harness::mono(SURFACE);
    frame(&mut h, false);
    frame(&mut h, false);
    assert!(h.rect(body()).is_none());

    h.click(header());
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
    h.click(header());
    frame(&mut h, false);
    frame(&mut h, false);
    assert!(h.rect(body()).is_none(), "a second click closed it again");
}

/// An Expander disabled on the frame a click lands does not toggle.
#[test]
fn a_click_on_the_frame_it_is_disabled_does_not_toggle() {
    let mut h = Harness::mono(SURFACE);
    frame(&mut h, false);
    frame(&mut h, false);
    h.click(header());
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

/// `keep_body` keeps a collapsed body's ids live (and its `TextEdit` text) at a record per frame.
#[test]
fn keep_body_records_a_collapsed_body_and_holds_its_state() {
    let mut h = Harness::mono(SURFACE);
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
        body_rect.unwrap().size.h,
        0.0,
        "a collapsed body takes no space: {body_rect:?}",
    );
    assert!(
        h.hit_at(body_rect.unwrap().min).is_none()
            || h.hit_at(body_rect.unwrap().min) != Some(label()),
        "and is not hit-tested",
    );
    assert!(
        h.ui().state::<ExpanderState>(header()).is_none(),
        "keeping the body is not itself a toggle",
    );
}

/// `open`, `close` and `is_open` drive a section from outside by its id: a section `start_open`
/// opens reads open, a click closes it, `open` and `close` flip it on its next show, and a section
/// never shown reads closed with no row minted.
#[test]
fn open_close_and_is_open_drive_it_from_outside() {
    let mut h = Harness::mono(SURFACE);
    let record = |ui: &mut Ui| {
        Expander::new("section")
            .id(root())
            .start_open(true)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            });
    };
    assert!(!Expander::is_open(h.ui(), root()));
    assert!(
        h.ui().state::<ExpanderState>(header()).is_none(),
        "no row minted"
    );
    h.prime(2, record);
    assert!(Expander::is_open(h.ui(), root()), "start_open opens it");
    assert!(h.rect(body()).is_some());

    h.click(header());
    h.frame(record);
    assert!(!Expander::is_open(h.ui(), root()), "a click closes it");

    Expander::open(h.ui_mut(), root());
    h.prime(2, record);
    assert!(Expander::is_open(h.ui(), root()));
    assert!(h.rect(body()).is_some(), "open reveals the body");

    Expander::close(h.ui_mut(), root());
    h.prime(2, record);
    assert!(!Expander::is_open(h.ui(), root()));
    assert!(h.rect(body()).is_none(), "close hides the body");
}

/// The first reveal snaps (no measured height to tween against); later ones animate.
#[test]
fn the_first_reveal_snaps_and_the_next_one_animates() {
    let base = ExpanderTheme::default();
    let theme = ExpanderTheme {
        defaults: SlotDefaults {
            animation: Some(AnimationSpec::MEDIUM),
            ..base.defaults
        },
        ..base
    };
    let mut h = Harness::mono(SURFACE);
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

    h.click(header());
    assert_eq!(
        h.frame_value(&mut record),
        1.0,
        "no height was known, so the reveal snapped whole",
    );
    h.advance_frames(2, time::Duration::from_millis(16), |ui| {
        record(ui);
    });

    h.advance_past_double_click();
    h.frame(|ui| {
        record(ui);
    });
    h.click(header());
    // The click frame has no elapsed time; the next shows the tween.
    assert_eq!(h.frame_value(&mut record), 1.0);
    h.advance(time::Duration::from_millis(16));
    // `MEDIUM` is 200 ms ease-out cubic: at 16 ms (1 − 16/200)³ = 0.778688 is left.
    assert_eq!(h.frame_value(&mut record), 0.778688);
}

/// An underdamped close swings below 0 and back above it. The section reads closed from the
/// frame the swing first reaches 0: openness 0 and no body, while the spring still swings.
#[test]
fn a_spring_close_stays_closed_through_its_swing() {
    let base = ExpanderTheme::default();
    // Decay a = 3/s at ωd = sqrt(200 − 9) = 13.82 rad/s, from 1 at rest: e^(−at)(cos ωd·t +
    // (a/ωd) sin ωd·t) reaches 0 at (π − atan(ωd/a))/ωd = 129 ms, −0.51 at π/ωd = 227 ms, and
    // +0.26 at 2π/ωd = 455 ms.
    let theme = ExpanderTheme {
        defaults: SlotDefaults {
            animation: Some(AnimationSpec::spring(200.0, 6.0)),
            ..base.defaults
        },
        ..base
    };
    let mut h = Harness::mono(SURFACE);
    let mut record = |ui: &mut Ui| {
        Expander::new("section")
            .id(root())
            .style(&theme)
            .start_open(true)
            .show(ui, |ui| {
                Text::new("body").id(label()).show(ui);
            })
            .openness
    };
    h.prime(2, |ui| {
        record(ui);
    });
    h.click(header());
    assert_eq!(
        h.frame_value(&mut record),
        1.0,
        "the click frame has no elapsed time"
    );

    let tick = time::Duration::from_millis(16);
    let mut closed_at = None;
    for frame in 1..=200 {
        h.advance(tick);
        let openness = h.frame_value(&mut record);
        let shown = h.rect(body()).is_some();
        assert!((0.0..=1.0).contains(&openness), "frame {frame}: {openness}");
        assert_eq!(shown, openness > 0.0, "frame {frame}: {openness}");
        match closed_at {
            None if !shown => closed_at = Some(frame),
            Some(at) => assert!(!shown, "closed at frame {at}, shown again at {frame}"),
            None => {}
        }
    }
    // Frame 8 at 128 ms is still above 0, frame 9 at 144 ms past it.
    assert_eq!(closed_at, Some(9));
}

/// Space and Enter toggle a focused header, nothing else, and its response reads them as the
/// click a pointer would make.
#[test]
fn space_and_enter_toggle_a_focused_header() {
    for key in [Key::Char(' '), Key::Enter] {
        let mut h = Harness::mono(SURFACE);
        frame(&mut h, false);
        frame(&mut h, false);

        h.clear_focus();
        h.key(key);
        assert!(!frame(&mut h, false), "{key:?} clicked an unfocused header");
        assert!(
            h.rect(body()).is_none(),
            "{key:?} moved an unfocused header",
        );

        h.set_focus(header());
        frame(&mut h, false);
        h.key(key);
        assert!(frame(&mut h, false), "{key:?} clicks a focused header");
        assert!(h.rect(body()).is_some(), "{key:?} opened a focused header");
    }
}

/// A quarter turn takes the dropdown `v` to the disclosure `>`.
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

    let turned = c.rotated(-consts::FRAC_PI_2);
    let expected = [
        Vec2::new(0.0, 8.0),
        Vec2::new(8.0, 4.0),
        Vec2::new(0.0, 0.0),
    ];
    // f32 `cos(π/2)` is -4.4e-8, leaving a few ulps of residue on the 4 px lever.
    let residue = "f32 cos(π/2) is not 0";
    for (got, want) in turned.into_iter().zip(expected) {
        assert_close(got.x, want.x, 1e-6, residue);
        assert_close(got.y, want.y, 1e-6, residue);
    }

    let t = ExpanderTheme::default();
    assert_eq!(t.arrow_angle(0.0), t.arrow_closed_angle);
    assert_eq!(t.arrow_angle(1.0), t.arrow_open_angle);
    assert_eq!(
        t.arrow_angle(2.0),
        t.arrow_open_angle,
        "openness past 1 clamps to open"
    );
}

/// The reveal clips against the body's whole height even on the frame the tween settles, or the next collapse jumps.
#[test]
fn a_settling_reveal_stores_the_whole_height() {
    let base = ExpanderTheme::default();
    let theme = ExpanderTheme {
        defaults: SlotDefaults {
            animation: Some(AnimationSpec::MEDIUM),
            ..base.defaults
        },
        ..base
    };
    let mut h = Harness::mono(SURFACE);
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
    let tick = time::Duration::from_millis(16);
    let toggle = |h: &mut Harness, record: &mut dyn FnMut(&mut Ui) -> f32| {
        h.advance_past_double_click();
        h.frame(|ui| {
            record(ui);
        });
        h.click(header());
        h.frame_value(&mut *record);
    };

    toggle(&mut h, &mut record);
    h.advance_frames(2, tick, |ui| {
        record(ui);
    });
    // One mono line, 19.2 snapped to 19.203125, under 4 + 4 padding.
    let whole = h.arranged(body()).unwrap().size.h;
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
                h.arranged(body()).unwrap().size.h,
                whole,
                "the body lays out whole under the clip at {openness}",
            );
        }
    }
    let row = h.ui().state::<ExpanderState>(header()).copied().unwrap();
    assert_eq!(row.height, Some(whole), "the settle frame stored {row:?}");
}
