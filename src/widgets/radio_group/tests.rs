use crate::host::harness::Harness;
use crate::internals::panic_probe;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::layout::axis::Axis;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::radio_group::{RadioGroup, RadioLook, option_id};
use glam::UVec2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pick {
    A,
    B,
    C,
}

const PICKS: [Pick; 3] = [Pick::A, Pick::B, Pick::C];
const LOOKS: [RadioLook<'static>; 2] = [RadioLook::Pip(None), RadioLook::Segment(None)];

fn group() -> WidgetId {
    WidgetId::from_hash("group")
}

fn option(idx: u32) -> WidgetId {
    option_id(group(), idx)
}

/// One frame of the group's options, in order: each one's rect and `changed`, and the group's.
#[derive(Debug)]
struct Rows {
    rects: [Option<Rect>; 3],
    changed: [bool; 3],
    group_changed: bool,
}

/// A Tab stop, the group of the three picks in `look`, and another Tab stop, in record order.
fn record(ui: &mut Ui, look: RadioLook<'_>, disabled: bool, cur: &mut Option<Pick>) -> Rows {
    let mut rows = Rows {
        rects: [None; 3],
        changed: [false; 3],
        group_changed: false,
    };
    Panel::vstack().auto_id().show(ui, |ui| {
        Block::new()
            .id(WidgetId::from_hash("before"))
            .focusable(true)
            .show(ui);
        rows.group_changed = RadioGroup::new(cur)
            .id(group())
            .axis(Axis::X)
            .look(look)
            .disabled(disabled)
            .show(ui, |ui, options| {
                for (idx, pick) in PICKS.into_iter().enumerate() {
                    let r = options
                        .option(Some(pick))
                        .label(format!("{pick:?}"))
                        .show(ui);
                    rows.rects[idx] = r.response.rect;
                    rows.changed[idx] = r.changed;
                }
            })
            .changed;
        Block::new()
            .id(WidgetId::from_hash("after"))
            .focusable(true)
            .show(ui);
    });
    rows
}

/// A harness two frames into the scene, so last frame's options are known.
fn settled(look: RadioLook<'_>, cur: &mut Option<Pick>) -> Harness {
    let mut h = Harness::mono(UVec2::new(400, 200));
    for _ in 0..2 {
        h.frame(|ui| {
            record(ui, look, false, cur);
        });
    }
    h
}

/// A click checks its option and only that option reports `changed`; a click on the checked one
/// is `clicked()` but no pick.
#[test]
fn a_click_checks_its_option_in_each_look() {
    for look in LOOKS {
        let mut cur = Some(Pick::A);
        let mut h = settled(look, &mut cur);
        let rows = h.frame_value(|ui| record(ui, look, false, &mut cur));
        assert_eq!(rows.changed, [false; 3], "{look:?}: an untouched frame");
        let center = |rect: Option<Rect>| rect.expect("arranged").center();
        let (b, c) = (center(rows.rects[1]), center(rows.rects[2]));

        h.click(b);
        let rows = h.frame_value(|ui| record(ui, look, false, &mut cur));
        assert_eq!(cur, Some(Pick::B), "{look:?}");
        assert_eq!(rows.changed, [false, true, false], "{look:?}");
        assert!(rows.group_changed, "{look:?}: the group reports the pick");

        h.click(c);
        h.frame(|ui| {
            record(ui, look, false, &mut cur);
        });
        h.click(c);
        let rows = h.frame_value(|ui| record(ui, look, false, &mut cur));
        assert_eq!(cur, Some(Pick::C), "{look:?}");
        assert_eq!(
            rows.changed, [false; 3],
            "{look:?}: the checked option latches"
        );
        assert!(!rows.group_changed, "{look:?}");
    }
}

/// The group is one Tab stop: Tab from the stop before lands on the checked option, or on the
/// first while none is checked, and the next Tab leaves the group, as APG's radio group has it.
#[test]
fn the_group_is_one_tab_stop_on_its_checked_option() {
    for look in LOOKS {
        for (cur, lands) in [(Some(Pick::B), option(1)), (None, option(0))] {
            let mut cur = cur;
            let mut h = settled(look, &mut cur);
            h.ui_mut().set_focus(WidgetId::from_hash("before"));
            let mut walk = Vec::new();
            for _ in 0..2 {
                h.key(Key::Tab);
                h.frame(|ui| {
                    record(ui, look, false, &mut cur);
                });
                walk.push(h.ui().focus());
            }
            assert_eq!(
                walk,
                [Some(lands), Some(WidgetId::from_hash("after"))],
                "{look:?} from {cur:?}"
            );
            assert_eq!(
                cur.is_none(),
                lands == option(0),
                "{look:?}: Tab checks nothing"
            );
        }
    }
}

/// An arrow moves focus and the pick together, Right and Down forward, Left and Up back, wrapping
/// at both ends; the frame it lands in reports the pick. A modified arrow moves nothing.
#[test]
fn arrows_move_focus_and_the_pick_together() {
    for look in LOOKS {
        let mut cur = Some(Pick::B);
        let mut h = settled(look, &mut cur);
        h.ui_mut().set_focus(option(1));
        h.frame(|ui| {
            record(ui, look, false, &mut cur);
        });
        for (key, want) in [
            (Key::ArrowRight, 2),
            (Key::ArrowDown, 0),
            (Key::ArrowLeft, 2),
            (Key::ArrowUp, 1),
            (Key::ArrowUp, 0),
            (Key::ArrowLeft, 2),
        ] {
            h.key(key);
            let rows = h.frame_value(|ui| record(ui, look, false, &mut cur));
            let label = format!("{look:?} {key:?} to {want}");
            assert_eq!(cur, Some(PICKS[want]), "{label}");
            assert!(rows.group_changed, "{label}: the pick reports");
            h.frame(|ui| {
                record(ui, look, false, &mut cur);
            });
            assert_eq!(h.ui().focus(), Some(option(want as u32)), "{label}");
        }

        h.hold_modifiers(Modifiers::SHIFT);
        h.key(Key::ArrowRight);
        h.frame(|ui| {
            record(ui, look, false, &mut cur);
        });
        assert_eq!(cur, Some(Pick::C), "{look:?}: Shift+Right picks nothing");
    }
}

/// Space checks the focused option, as on the first one Tab lands on while none is checked.
#[test]
fn space_checks_the_focused_option() {
    for look in LOOKS {
        let mut cur = None;
        let mut h = settled(look, &mut cur);
        h.ui_mut().set_focus(option(0));
        h.frame(|ui| {
            record(ui, look, false, &mut cur);
        });
        h.key(Key::Char(' '));
        let rows = h.frame_value(|ui| record(ui, look, false, &mut cur));
        assert_eq!(cur, Some(Pick::A), "{look:?}");
        assert_eq!(rows.changed, [true, false, false], "{look:?}");
    }
}

/// A disabled group answers no arrow and no click.
#[test]
fn a_disabled_group_moves_nothing() {
    for look in LOOKS {
        let mut cur = Some(Pick::A);
        let mut h = settled(look, &mut cur);
        let rows = h.frame_value(|ui| record(ui, look, true, &mut cur));
        h.ui_mut().set_focus(option(0));
        h.key(Key::ArrowRight);
        h.frame(|ui| {
            record(ui, look, true, &mut cur);
        });
        h.click(rows.rects[2].expect("arranged").center());
        h.frame(|ui| {
            record(ui, look, true, &mut cur);
        });
        assert_eq!(cur, Some(Pick::A), "{look:?}");
    }
}

/// An arrow passes a disabled option by, both ways, and while no enabled option is checked Tab
/// enters at the first enabled one, as APG's radio group skips a disabled radio.
#[test]
fn arrows_and_tab_pass_a_disabled_option_by() {
    // First B alone is disabled, then A and B, which leaves C the first enabled option.
    let scene = |off: [bool; 3]| {
        move |ui: &mut Ui, cur: &mut Option<Pick>| {
            RadioGroup::new(cur).id(group()).show(ui, |ui, options| {
                for (pick, off) in PICKS.into_iter().zip(off) {
                    options
                        .option(Some(pick))
                        .label(format!("{pick:?}"))
                        .disabled(off)
                        .show(ui);
                }
            });
            Block::new()
                .id(WidgetId::from_hash("after"))
                .focusable(true)
                .show(ui);
        }
    };
    let only_b = scene([false, true, false]);
    let mut cur = Some(Pick::A);
    let mut h = Harness::mono(UVec2::new(400, 200));
    for _ in 0..2 {
        h.frame(|ui| only_b(ui, &mut cur));
    }
    h.ui_mut().set_focus(option(0));
    h.frame(|ui| only_b(ui, &mut cur));
    for (key, want) in [(Key::ArrowRight, Pick::C), (Key::ArrowLeft, Pick::A)] {
        h.key(key);
        h.frame(|ui| only_b(ui, &mut cur));
        h.frame(|ui| only_b(ui, &mut cur));
        assert_eq!(cur, Some(want), "{key:?} passes B by");
    }

    let a_and_b = scene([true, true, false]);
    let mut cur = None;
    for _ in 0..2 {
        h.frame(|ui| a_and_b(ui, &mut cur));
    }
    h.ui_mut().set_focus(WidgetId::from_hash("after"));
    h.hold_modifiers(Modifiers::SHIFT);
    h.key(Key::Tab);
    h.frame(|ui| a_and_b(ui, &mut cur));
    assert_eq!(h.ui().focus(), Some(option(2)), "Shift+Tab enters at C");
    assert_eq!(cur, None, "entering checks nothing");

    // A checked option that is disabled is no stop: Tab enters at the first enabled one.
    let mut cur = Some(Pick::A);
    for _ in 0..2 {
        h.frame(|ui| a_and_b(ui, &mut cur));
    }
    h.ui_mut().set_focus(WidgetId::from_hash("after"));
    h.key(Key::Tab);
    h.frame(|ui| a_and_b(ui, &mut cur));
    assert_eq!(
        h.ui().focus(),
        Some(option(2)),
        "Shift+Tab passes the checked A by"
    );
    assert_eq!(cur, Some(Pick::A), "and checks nothing");
}

/// The group sets each option's id, so an id of the caller's is refused, in every build.
#[test]
fn an_option_refuses_an_id_of_its_own() {
    let mut h = Harness::mono(UVec2::new(200, 100));
    let mut value = Some(Pick::A);
    panic_probe::assert_panics_with("a radio option takes its id from its group", || {
        h.frame(|ui| {
            RadioGroup::new(&mut value)
                .id(group())
                .show(ui, |ui, options| {
                    options
                        .option(Some(Pick::A))
                        .id(WidgetId::from_hash("own"))
                        .show(ui);
                });
        });
    });
}

/// Each look reads the slot its variant carries, and the theme's while that is `None`: the pip's
/// gap between box and label, the segment track's inset before the first segment. The theme and
/// each override hold distinct values, so no look reads the other's slot or the wrong source.
#[test]
fn each_look_reads_its_own_slot_over_the_themes() {
    let mut h = Harness::mono(UVec2::new(400, 200));
    h.ui_mut().theme_mut().radio.gap = 5.0;
    h.ui_mut().theme_mut().segmented.track_padding = Spacing::all(3.0);
    let mut pip = h.ui().theme().radio.clone();
    pip.gap = 17.0;
    let mut segment = h.ui().theme().segmented.clone();
    segment.track_padding = Spacing::all(11.0);

    let measure = |h: &mut Harness, look: RadioLook<'_>| {
        let mut cur = Some(Pick::A);
        h.frame(|ui| {
            record(ui, look, false, &mut cur);
        });
        match look {
            RadioLook::Pip(_) => {
                let label = h.rect(option(0).with("label")).expect("label arranged");
                let pip = h.rect(option(0).with("box")).expect("box arranged");
                label.min.x - pip.max().x
            }
            RadioLook::Segment(_) => {
                let first = h.rect(option(0)).expect("segment arranged");
                first.min.x - h.rect(group()).expect("group arranged").min.x
            }
        }
    };
    assert_eq!(measure(&mut h, RadioLook::Pip(None)), 5.0);
    assert_eq!(measure(&mut h, RadioLook::Pip(Some(&pip))), 17.0);
    assert_eq!(measure(&mut h, RadioLook::Segment(None)), 3.0);
    assert_eq!(measure(&mut h, RadioLook::Segment(Some(&segment))), 11.0);
}
