use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::host::harness::Harness;
use crate::icons::icon_table::IconTable;
use crate::icons::internals::ONE_COLOR;
use crate::internals::panic_probe;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::brush::Brush;
use crate::theme::segmented::SegmentedTheme;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::radio_group::segment_option::coverage;
use crate::widgets::radio_group::segment_track::SegmentTrack;
use crate::widgets::radio_group::{PillMemory, RadioGroup, RadioLook, option_id};
use glam::UVec2;
use std::time::Duration;

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

/// A segment track of the three picks along `axis` in `slot`, its labels of three lengths, a Hug
/// group so the shares are the layout's to make equal.
fn track(ui: &mut Ui, slot: &SegmentedTheme, axis: Axis, cur: &mut Option<Pick>) {
    RadioGroup::new(cur)
        .id(group())
        .axis(axis)
        .look(RadioLook::Segment(Some(slot)))
        .show(ui, |ui, options| {
            for (pick, label) in PICKS.into_iter().zip(["A", "A much longer label", "C"]) {
                options.option(Some(pick)).label(label).show(ui);
            }
        });
}

fn pill() -> WidgetId {
    group().with("pill")
}

/// The segments take equal shares of the track whatever their labels, along either axis, and the
/// pill lies exactly on the checked one.
#[test]
fn the_pill_lies_on_the_checked_segment_of_equal_shares() {
    for axis in [Axis::X, Axis::Y] {
        let mut h = Harness::mono(UVec2::new(600, 300));
        let slot = h.ui().theme().segmented.clone();
        let mut cur = Some(Pick::B);
        for _ in 0..2 {
            h.frame(|ui| track(ui, &slot, axis, &mut cur));
        }
        let rects = [0, 1, 2].map(|idx| h.rect(option(idx)).expect("segment arranged"));
        let extent = |r: Rect| match axis {
            Axis::X => r.size.w,
            Axis::Y => r.size.h,
        };
        assert_eq!(extent(rects[0]), extent(rects[1]), "{axis:?}");
        assert_eq!(extent(rects[1]), extent(rects[2]), "{axis:?}");
        assert_eq!(h.rect(pill()), Some(rects[1]), "{axis:?}");
    }
}

/// A pick slides the pill along the track under the slot's spec, here 100 ms linear: half way
/// after 50 ms, on the new segment after 100.
#[test]
fn a_pick_slides_the_pill_to_the_new_segment() {
    let mut h = Harness::mono(UVec2::new(600, 200));
    let mut slot = h.ui().theme().segmented.clone();
    slot.slide = AnimationSpec::duration(Duration::from_millis(100), Easing::Linear);
    let mut cur = Some(Pick::A);
    for _ in 0..2 {
        h.frame(|ui| track(ui, &slot, Axis::X, &mut cur));
    }
    let [a, c] = [0, 2].map(|idx| h.rect(option(idx)).expect("segment arranged"));
    assert_eq!(h.rect(pill()), Some(a), "premise: the pill rests on A");

    h.click(c.center());
    h.frame(|ui| track(ui, &slot, Axis::X, &mut cur));
    assert_eq!(cur, Some(Pick::C));
    assert_eq!(
        h.rect(pill()).map(|r| r.min.x),
        Some(a.min.x),
        "it starts from A"
    );
    h.advance(Duration::from_millis(50));
    h.frame(|ui| track(ui, &slot, Axis::X, &mut cur));
    let half = h.rect(pill()).expect("pill arranged").min.x;
    // Two shares of travel at half way: one share past A, where B starts.
    let b = h.rect(option(1)).expect("segment arranged");
    assert!((half - b.min.x).abs() < 1e-3, "{half} against {}", b.min.x);
    h.advance(Duration::from_millis(60));
    h.frame(|ui| track(ui, &slot, Axis::X, &mut cur));
    assert_eq!(h.rect(pill()), Some(c), "it lands on C");
}

/// A group that appears with a checked option draws its pill on that first frame, as the frame
/// records again once the options have reported, and draws none while nothing is checked.
#[test]
fn a_track_draws_its_pill_on_its_first_frame() {
    let mut h = Harness::mono(UVec2::new(600, 200));
    let slot = h.ui().theme().segmented.clone();
    h.frame(|ui| {
        Block::new().id(WidgetId::from_hash("before")).show(ui);
    });
    let mut cur = Some(Pick::C);
    h.frame(|ui| track(ui, &slot, Axis::X, &mut cur));
    assert_eq!(h.rect(pill()), h.rect(option(2)));
    let mut none = None;
    h.frame(|ui| track(ui, &slot, Axis::X, &mut none));
    h.frame(|ui| track(ui, &slot, Axis::X, &mut none));
    assert_eq!(h.rect(pill()), None, "no option is checked");
}

/// An option added under a checked one moves the pill onto the new shares in the frame the count
/// changes, as the frame records again once the options report three where the pill drew for two.
#[test]
fn a_track_that_gains_an_option_moves_its_pill_that_frame() {
    let mut h = Harness::mono(UVec2::new(600, 200));
    let slot = h.ui().theme().segmented.clone();
    let mut cur = Some(Pick::B);
    let record = |h: &mut Harness, count: usize, cur: &mut Option<Pick>| {
        h.frame(|ui| {
            RadioGroup::new(cur)
                .id(group())
                .axis(Axis::X)
                .look(RadioLook::Segment(Some(&slot)))
                .size((Sizing::fixed(300.0), Sizing::HUG))
                .show(ui, |ui, options| {
                    for pick in PICKS.into_iter().take(count) {
                        options
                            .option(Some(pick))
                            .label(format!("{pick:?}"))
                            .show(ui);
                    }
                });
        });
    };
    for _ in 0..2 {
        record(&mut h, 2, &mut cur);
    }
    let two = h.rect(option(1)).expect("segment arranged");
    assert_eq!(h.rect(pill()), Some(two), "premise: on B of two");
    record(&mut h, 3, &mut cur);
    let three = h.rect(option(1)).expect("segment arranged");
    assert_ne!(three, two, "a third option narrows the shares");
    assert_eq!(h.rect(pill()), Some(three), "and the pill follows them");
}

/// A label lights by the share of its segment the pill covers: a pill a quarter past B covers
/// three quarters of B and a quarter of C, nothing of A; one past the ends clamps; with no pill
/// the checked segment is whole.
#[test]
fn a_label_lights_by_the_share_the_pill_covers() {
    let lit = |at: f32| [0, 1, 2].map(|idx| coverage(Some(at), idx, false));
    assert_eq!(lit(1.25), [0.0, 0.75, 0.25]);
    assert_eq!(lit(0.5), [0.5, 0.5, 0.0]);
    assert_eq!(lit(2.0), [0.0, 0.0, 1.0]);
    assert_eq!(lit(-0.25), [0.75, 0.0, 0.0]);
    assert_eq!(coverage(None, 1, true), 1.0);
    assert_eq!(coverage(None, 1, false), 0.0);
}

/// While motion is reduced a pick moves the pill at once and fades it in there under the slide's
/// spec, here 100 ms linear: clear on the frame of the pick, half after 50 ms, whole after 100;
/// unreduced it stays whole as it slides.
#[test]
fn reduced_motion_fades_the_pill_in_at_the_new_segment() {
    for reduced in [true, false] {
        let mut h = Harness::mono(UVec2::new(600, 200));
        h.set_system_motion_reduced(reduced);
        let mut slot = h.ui().theme().segmented.clone();
        slot.slide = AnimationSpec::duration(Duration::from_millis(100), Easing::Linear);
        let mut memory = PillMemory::default();
        let shown = |h: &mut Harness, target: u32, memory: &mut PillMemory| {
            h.frame_value(|ui| {
                let pill = SegmentTrack {
                    id: group(),
                    slot: &slot,
                    axis: Axis::X,
                }
                .pill(ui, memory, target, 3);
                let Brush::Solid(fill) = pill.chrome.paint.fill else {
                    panic!("the stock pill is solid");
                };
                (pill.at, fill.a)
            })
        };
        for _ in 0..2 {
            shown(&mut h, 0, &mut memory);
        }
        let accent = slot.pill.paint.fill.clone();
        let Brush::Solid(accent) = accent else {
            panic!("the stock pill is solid");
        };
        let picked = shown(&mut h, 2, &mut memory);
        h.advance(Duration::from_millis(50));
        let half = shown(&mut h, 2, &mut memory);
        h.advance(Duration::from_millis(60));
        let done = shown(&mut h, 2, &mut memory);
        if reduced {
            assert_eq!(picked, (2.0, 0.0));
            assert_eq!(half.0, 2.0);
            assert!((half.1 - 0.5 * accent.a).abs() < 1e-4, "{half:?}");
        } else {
            assert_eq!(picked, (0.0, accent.a));
            assert!((half.0 - 1.0).abs() < 1e-4, "{half:?}");
        }
        assert_eq!(done, (2.0, accent.a), "reduced {reduced}");
    }
}

/// An option's icon sits before its label, a text line square and a gap apart: the slot's gap in
/// a segment, the radio slot's in a pip row, after the box.
#[test]
fn an_option_shows_its_icon_before_its_label() {
    for look in LOOKS {
        let mut h = Harness::mono(UVec2::new(600, 200));
        let icons = h
            .ui()
            .load_icons(IconTable::from_svgs([("mark", ONE_COLOR)]).expect("the svg parses"));
        let mark = icons.by_name("mark").expect("registered");
        let mut cur = Some(Pick::A);
        for _ in 0..2 {
            h.frame(|ui| {
                RadioGroup::new(&mut cur)
                    .id(group())
                    .axis(Axis::X)
                    .look(look)
                    .show(ui, |ui, options| {
                        options
                            .option(Some(Pick::A))
                            .icon(mark)
                            .label("Auto")
                            .show(ui);
                    });
            });
        }
        let icon = h.rect(option(0).with("icon")).expect("icon arranged");
        let label = h.rect(option(0).with("label")).expect("label arranged");
        let theme = h.ui().theme();
        let line = theme.text.line_height();
        let gap = match look {
            RadioLook::Pip(_) => theme.radio.gap,
            RadioLook::Segment(_) => theme.segmented.gap,
        };
        assert_eq!((icon.size.w, icon.size.h), (line, line), "{look:?}");
        assert_eq!(label.min.x - icon.max().x, gap, "{look:?}");
        if let RadioLook::Pip(_) = look {
            let pip = h.rect(option(0).with("box")).expect("box arranged");
            assert_eq!(icon.min.x - pip.max().x, gap, "after the box");
        }
    }
}

/// `show_with` paints an app's own glyph where an icon would sit, handed the label's resolved
/// style: a checked segment's glyph gets the ink the pill lights, `on_accent`, and an unchecked
/// pip's the resting ink, each a line tall.
#[test]
fn show_with_paints_a_glyph_in_the_labels_ink() {
    for (look, checked) in [
        (RadioLook::Segment(None), true),
        (RadioLook::Pip(None), false),
    ] {
        let mut h = Harness::mono(UVec2::new(600, 200));
        let mut cur = if checked {
            Some(Pick::A)
        } else {
            Some(Pick::B)
        };
        let mut handed = None;
        for _ in 0..2 {
            h.frame(|ui| {
                RadioGroup::new(&mut cur)
                    .id(group())
                    .axis(Axis::X)
                    .look(look)
                    .show(ui, |ui, options| {
                        options
                            .option(Some(Pick::A))
                            .label("Auto")
                            .show_with(ui, |_, text| handed = Some(*text));
                        options.option(Some(Pick::B)).label("B").show(ui);
                    });
            });
        }
        let text = handed.expect("the glyph painted");
        let glyph = h.rect(option(0).with("icon")).expect("glyph arranged");
        assert_eq!(
            (glyph.size.w, glyph.size.h),
            (text.line_height(), text.line_height())
        );
        let theme = h.ui().theme();
        let ink = match look {
            RadioLook::Segment(_) => theme.segmented.segment.selected.normal.text.color,
            RadioLook::Pip(_) => theme.radio.unchecked.normal.text.color,
        };
        assert_eq!(Some(text.color), ink.or(Some(theme.text.color)), "{look:?}");
    }
}

/// An option paints one glyph, so an icon beside `show_with` is refused.
#[test]
fn an_option_refuses_an_icon_and_a_painted_glyph() {
    let mut h = Harness::mono(UVec2::new(200, 100));
    let icons = h
        .ui()
        .load_icons(IconTable::from_svgs([("mark", ONE_COLOR)]).expect("the svg parses"));
    let mark = icons.by_name("mark").expect("registered");
    let mut value = Some(Pick::A);
    panic_probe::assert_panics_with("a radio option paints one glyph", || {
        h.frame(|ui| {
            RadioGroup::new(&mut value)
                .id(group())
                .show(ui, |ui, options| {
                    options
                        .option(Some(Pick::A))
                        .icon(mark)
                        .show_with(ui, |_, _| {});
                });
        });
    });
}
