//! Chip identity, the reserved badge box, close-over-activate ordering,
//! keyboard travel, and the page binding a tabbed view writes.

use glam::{UVec2, Vec2};

use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::tabs::tab_item::{TabBadge, TabItem};
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use crate::widgets::tabs::tabbed_view::{TabbedView, TabsAction};
use crate::widgets::text::Text;
use std::iter;

const SURFACE: UVec2 = UVec2::new(600, 200);

fn strip_id() -> WidgetId {
    WidgetId::from_hash("test.strip")
}

/// Three chips, keyed 10 / 20 / 30 so a key can never be mistaken for a
/// slot.
fn items(ui: &mut Ui, badge: TabBadge) -> Vec<TabItem> {
    [(10u64, "alpha"), (20, "beta"), (30, "gamma")]
        .into_iter()
        .map(|(key, label)| TabItem {
            badge: if key == 10 { badge } else { TabBadge::None },
            ..TabItem::new(key, ui.intern(label))
        })
        .collect()
}

/// One frame of a three-chip strip, with `selected` capped.
fn strip_frame(h: &mut UiHarness, selected: usize, badge: TabBadge) {
    h.frame(|ui| {
        let items = items(ui, badge);
        TabStrip::new(&items)
            .id(strip_id())
            .selected(selected)
            .show(ui);
    });
}

/// Chip ids come from the item key, never from the slot. A strip that
/// reorders between frames must hand the *same* id to the same tab, or a
/// click read one phase later would land on whatever slid into the slot.
#[test]
fn chip_ids_follow_the_key_and_not_the_slot() {
    let mut h = UiHarness::new(SURFACE);
    h.prime(2, |ui| {
        let items = items(ui, TabBadge::None);
        TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
    });
    let alpha = TabStrip::chip_id(strip_id(), 10);
    let before = h.rect(alpha).expect("alpha arranged");

    // The same three items, reversed. Alpha keeps its id and moves from
    // the leading slot to the trailing one.
    h.prime(2, |ui| {
        let mut items = items(ui, TabBadge::None);
        items.reverse();
        TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
    });
    let after = h
        .rect(alpha)
        .expect("alpha still arranged under its own id");
    assert_eq!(
        before.size.w, after.size.w,
        "the same chip, so the same width"
    );
    assert_ne!(
        before.min.x, after.min.x,
        "the reorder moved it: {before:?} then {after:?}"
    );
    assert_eq!(
        TabStrip::chip_id(strip_id(), 10),
        alpha,
        "the derivation is a pure function of strip and key"
    );
    assert_ne!(
        TabStrip::chip_id(strip_id(), 20),
        TabStrip::chip_id(strip_id(), 30)
    );
}

/// The badge is a visibility change, never a layout one: inking the dot
/// must leave the chip exactly the size it was, or every chip to its
/// right would shift.
#[test]
fn the_badge_reserves_the_same_box_idle_and_inked() {
    let mut h = UiHarness::new(SURFACE);
    let alpha = TabStrip::chip_id(strip_id(), 10);
    let dot = alpha.with("badge");
    let beta = TabStrip::chip_id(strip_id(), 20);

    h.prime(2, |ui| {
        let items = items(ui, TabBadge::Idle);
        TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
    });
    let idle_chip = h.rect(alpha).expect("alpha arranged");
    let idle_dot = h.rect(dot).expect("an idle badge still reserves its box");

    h.prime(2, |ui| {
        let items = items(ui, TabBadge::On);
        TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
    });
    let inked_chip = h.rect(alpha).expect("still there");
    let inked_dot = h.rect(dot).expect("still there");

    assert_eq!(
        (idle_chip.size.w, idle_chip.size.h),
        (inked_chip.size.w, inked_chip.size.h),
        "the dot resized the chip: {idle_chip:?} idle against {inked_chip:?} inked",
    );
    assert_eq!(
        (idle_dot.size.w, idle_dot.size.h),
        (inked_dot.size.w, inked_dot.size.h),
        "the inked dot fills exactly the box the idle one reserved",
    );
    assert!(
        inked_dot.max().x <= inked_chip.max().x,
        "the dot overflowed its chip: {inked_dot:?} in {inked_chip:?}",
    );
    assert!(
        h.rect(beta.with("badge")).is_none(),
        "a chip with no badge reserves no box",
    );
}

/// The selection cap adds no height. The selected chip lifts its inner
/// top inset by exactly the cap, so both chips measure the same.
#[test]
fn the_selection_cap_adds_no_height() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);
    let selected = h.rect(TabStrip::chip_id(strip_id(), 10)).expect("arranged");
    let plain = h.rect(TabStrip::chip_id(strip_id(), 20)).expect("arranged");
    assert_eq!(
        selected.size.h, plain.size.h,
        "the cap grew the selected chip: {selected:?} against {plain:?}",
    );
}

/// The close button sits inside the chip, so one press reaches both. The
/// close has to win, or closing a background tab would activate it on
/// the way out.
#[test]
fn a_close_click_reports_a_close_and_not_a_click() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);

    h.click_on(TabStrip::close_id(strip_id(), 20));
    let hit = h.frame_value(|ui| {
        let items = items(ui, TabBadge::None);
        let r = TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
        (r.clicked, r.activated(), r.closed)
    });
    assert_eq!(
        hit,
        (None, None, Some(1)),
        "the close won over the activation"
    );
}

/// A plain chip click reports its slot, and nothing else.
#[test]
fn a_chip_click_reports_its_slot() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);

    h.click_on(TabStrip::chip_id(strip_id(), 30));
    let hit = h.frame_value(|ui| {
        let items = items(ui, TabBadge::None);
        let r = TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
        (r.clicked, r.keyed, r.activated(), r.closed)
    });
    assert_eq!(hit, (Some(2), None, Some(2), None));
}

/// Keyboard travel on the WAI-ARIA tab pattern. Reported apart from a
/// pointer click, because a caller that polls the chips itself one phase
/// earlier already holds the click and would otherwise act on it twice.
#[test]
fn arrows_home_and_end_travel_and_wrap() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);
    h.set_focus(strip_id());
    strip_frame(&mut h, 0, TabBadge::None);

    let travel = |h: &mut UiHarness, key: Key, mods: Modifiers, selected: usize| {
        h.set_modifiers(mods);
        h.key(key);
        let hit = h.frame_value(|ui| {
            let items = items(ui, TabBadge::None);
            let r = TabStrip::new(&items)
                .id(strip_id())
                .selected(selected)
                .show(ui);
            assert_eq!(r.activated(), r.keyed, "a keyboard move is an activation");
            (r.keyed, r.clicked)
        });
        h.set_modifiers(Modifiers::default());
        hit
    };

    assert_eq!(
        travel(&mut h, Key::ArrowRight, Modifiers::default(), 0),
        (Some(1), None),
        "right steps forward, and reports as a keyboard move"
    );
    assert_eq!(
        travel(&mut h, Key::ArrowLeft, Modifiers::default(), 0),
        (Some(2), None),
        "left wraps around the near end"
    );
    assert_eq!(
        travel(&mut h, Key::End, Modifiers::default(), 0).0,
        Some(2),
        "End jumps to the last chip"
    );
    assert_eq!(
        travel(&mut h, Key::Home, Modifiers::default(), 2).0,
        Some(0),
        "Home jumps to the first"
    );
    assert_eq!(
        travel(&mut h, Key::ArrowRight, Modifiers::default(), 2),
        (Some(0), None),
        "right wraps around the far end"
    );

    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    assert_eq!(
        travel(&mut h, Key::Tab, ctrl, 0).0,
        Some(1),
        "Ctrl+Tab cycles forward"
    );
    // The two Tab chords are told apart by an exact modifier match, so
    // the shift variant must not fall into the plain one's arm.
    let ctrl_shift = Modifiers {
        ctrl: true,
        shift: true,
        ..Modifiers::default()
    };
    assert_eq!(
        travel(&mut h, Key::Tab, ctrl_shift, 0).0,
        Some(2),
        "Ctrl+Shift+Tab cycles back"
    );

    // With nothing selected, a step lands on the end it moves from.
    for (key, want) in [(Key::ArrowRight, 0), (Key::ArrowLeft, 2)] {
        h.key(key);
        let keyed = h.frame_value(|ui| {
            let items = items(ui, TabBadge::None);
            TabStrip::new(&items).id(strip_id()).show(ui).keyed
        });
        assert_eq!(keyed, Some(want), "{key:?} with no selection");
    }
}

/// A keyboard move to a chip out of sight pans the band to it on the next
/// frame: End in a strip too narrow for its last chip leaves that chip
/// inside the band's clip — the band's rect deflated by its padding.
#[test]
fn a_keyboard_move_pans_the_band_to_the_chip() {
    use crate::primitives::math::approx::EPS;

    let record = |h: &mut UiHarness, selected: usize| {
        h.frame_value(|ui| {
            let items = items(ui, TabBadge::None);
            TabStrip::new(&items)
                .id(strip_id())
                .selected(selected)
                .size((Sizing::fixed(90.0), Sizing::HUG))
                .show(ui)
                .keyed
        })
    };
    let mut h = UiHarness::new(SURFACE);
    record(&mut h, 0);
    record(&mut h, 0);
    let in_clip = |h: &mut UiHarness| {
        let padding = h.ui().theme().tabs.strip_padding;
        let clip = h
            .rect(strip_id().with("band"))
            .unwrap()
            .deflated_by(padding);
        let chip = TabStrip::chip_id(strip_id(), 30);
        let full = h.transform(chip).apply_rect(h.arranged(chip));
        full.min.x >= clip.min.x - EPS && full.max().x <= clip.max().x + EPS
    };
    assert!(!in_clip(&mut h), "premise: the last chip is out of sight");
    h.set_focus(strip_id());
    record(&mut h, 0);
    h.key(Key::End);
    assert_eq!(record(&mut h, 0), Some(2));
    record(&mut h, 2);
    record(&mut h, 2);
    assert!(in_clip(&mut h), "the band panned to the chip End selected");
}

/// Travel is scoped to focus: the same press with the strip unfocused
/// moves nothing, so an application's own arrow handling keeps working.
#[test]
fn travel_needs_focus_inside_the_strip() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);
    h.clear_focus();
    h.key(Key::ArrowRight);
    let keyed = h.frame_value(|ui| {
        let items = items(ui, TabBadge::None);
        TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .show(ui)
            .keyed
    });
    assert_eq!(keyed, None);
}

/// A focused strip takes Ctrl+Tab, which it cycles on, and lets bare Tab
/// walk past to an app root that declares a scope of its own — the strip
/// has no use for traversal, so claiming it would cut the app's traversal
/// off at the chips.
#[test]
fn a_focused_strip_cycles_on_ctrl_tab_and_yields_bare_tab() {
    use crate::KeyFilter;
    use crate::input::shortcut::{Shortcut, ShortcutMods};

    let scene = |ui: &mut Ui, probe: Shortcut| {
        Panel::vstack()
            .id(WidgetId::from_hash("app-root"))
            .input_scope(KeyFilter::ACCEL)
            .show(ui, |ui| {
                let at_root = ui.key_pressed(probe);
                let items = items(ui, TabBadge::None);
                let keyed = TabStrip::new(&items)
                    .id(strip_id())
                    .selected(0)
                    .show(ui)
                    .keyed;
                (at_root, keyed)
            })
            .inner
    };
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::default()
    };
    for (mods, probe_mods, want) in [
        (Modifiers::default(), ShortcutMods::NONE, (true, None)),
        (ctrl, ShortcutMods::CTRL, (false, Some(1))),
    ] {
        let probe = Shortcut::new(probe_mods, Key::Tab);
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            scene(ui, probe);
        });
        h.set_focus(strip_id());
        h.frame(|ui| {
            scene(ui, probe);
        });
        h.set_modifiers(mods);
        h.key(Key::Tab);
        assert_eq!(h.frame_value(|ui| scene(ui, probe)), want, "{mods:?}");
    }
}

/// The insertion rule is a pure count of the chip centres the pointer
/// has passed, so it is checked against hand-placed rects.
#[test]
fn the_insertion_slot_counts_the_centres_passed() {
    let chips = [
        Rect::new(0.0, 0.0, 40.0, 20.0),
        Rect::new(50.0, 0.0, 40.0, 20.0),
        Rect::new(100.0, 0.0, 40.0, 20.0),
    ];
    // Centres sit at 20, 70 and 120.
    let slot = |x: f32| TabStrip::insertion_slot(chips.iter().copied(), x);
    assert_eq!(slot(-5.0), 0, "before the first chip");
    assert_eq!(slot(19.0), 0, "the leading half of the first chip");
    assert_eq!(slot(21.0), 1, "past the first centre");
    assert_eq!(slot(71.0), 2);
    assert_eq!(slot(500.0), 3, "past every centre appends");
    assert_eq!(
        TabStrip::insertion_slot(iter::empty(), 0.0),
        0,
        "an empty strip has one slot"
    );
}

const PAGES: [&str; 3] = ["Colour", "Geometry", "Metadata"];

/// Clicking a chip writes the bound index and records the new page on
/// the same frame — the view owns its selection, so nothing lags.
#[test]
fn a_tabbed_view_writes_its_binding_and_shows_the_new_page() {
    let mut h = UiHarness::new(SURFACE);
    let view = WidgetId::from_hash("test.view");
    let mut page = 0usize;
    let mut drawn = 0usize;
    let record = |ui: &mut Ui, page: &mut usize, drawn: &mut usize| {
        TabbedView::new(page, &PAGES)
            .id(view)
            .closable(false)
            .show(ui, |ui, index| {
                *drawn = index;
                Text::new(PAGES[index]).id_salt("body").show(ui);
            })
            .action
    };
    h.prime(2, |ui| {
        record(ui, &mut page, &mut drawn);
    });
    assert_eq!((page, drawn), (0, 0));

    let strip = view.with("strip");
    h.click_on(TabStrip::chip_id(strip, 2));
    let action = h.frame_value(|ui| record(ui, &mut page, &mut drawn));
    assert_eq!(action, Some(TabsAction::Activated { index: 2 }));
    assert_eq!(
        (page, drawn),
        (2, 2),
        "the binding moved and the body drew the new page on the same frame"
    );
    assert!(
        h.rect(TabStrip::close_id(strip, 0)).is_none(),
        "closable(false) records no close button",
    );
}

/// A page index that does not address the option slice is a caller bug,
/// exactly as it is for `ComboBox` — there is no empty state to fall
/// back to.
#[test]
#[should_panic(expected = "out of range")]
fn a_tabbed_view_panics_on_an_index_it_cannot_show() {
    let mut h = UiHarness::new(SURFACE);
    let mut page = 7usize;
    h.frame(|ui| {
        TabbedView::new(&mut page, &PAGES).show(ui, |_, _| {});
    });
}

/// A drag that releases over another slot reports the move rather than
/// making it — the view holds a shared slice and cannot reorder it — and
/// moves the bound index with the page it named. A release in either gap
/// beside the chip is no move, and a release off the strip is no drop.
#[test]
fn a_reorderable_view_reports_the_slot_a_drag_released_over() {
    type Release = fn(&UiHarness, WidgetId) -> Vec2;

    let view = WidgetId::from_hash("test.reorder");
    let strip = view.with("strip");
    let record = |ui: &mut Ui, page: &mut usize| {
        TabbedView::new(page, &PAGES)
            .id(view)
            .reorderable(true)
            .show(ui, |ui, _| {
                Panel::vstack()
                    .id_salt("page")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |_| {});
            })
            .action
    };
    let rows: [(&str, Release, Option<TabsAction>, usize); 3] = [
        (
            "past the last centre: the append",
            |h, strip| {
                let onto = h.center_of(TabStrip::chip_id(strip, 2));
                Vec2::new(onto.x + 4.0, onto.y)
            },
            Some(TabsAction::Reordered { from: 0, to: 3 }),
            2,
        ),
        (
            "just past its own centre: the gap beside it",
            |h, strip| {
                let own = h.center_of(TabStrip::chip_id(strip, 0));
                Vec2::new(own.x + 4.0, own.y)
            },
            None,
            0,
        ),
        (
            "deep in the page: off the strip",
            |h, strip| {
                let onto = h.center_of(TabStrip::chip_id(strip, 2));
                Vec2::new(onto.x + 4.0, onto.y + 120.0)
            },
            None,
            0,
        ),
    ];
    for (label, release, want, page_after) in rows {
        let mut h = UiHarness::new(SURFACE);
        let mut page = 0usize;
        h.prime(2, |ui| {
            record(ui, &mut page);
        });
        h.press_on(TabStrip::chip_id(strip, 0));
        h.drag_to(release(&h, strip));
        h.frame(|ui| {
            record(ui, &mut page);
        });
        h.release();
        let action = h.frame_value(|ui| record(ui, &mut page));
        assert_eq!(action, want, "{label}");
        assert_eq!(page, page_after, "{label}: the bound page");
    }
}

/// Where an index lands when `from` moves into the gap `to`, for pages
/// [A, B, C, D]. Moving A to the end (gap 4) puts it at 3 and shifts the
/// others down; moving D to the front (gap 0) shifts the others up.
#[test]
fn a_move_carries_every_index_with_its_page() {
    use crate::widgets::tabs::tabbed_view::internals::moved_index;
    for (from, to, before, after) in [
        (0, 4, [0, 1, 2, 3], [3, 0, 1, 2]),
        (3, 0, [0, 1, 2, 3], [1, 2, 3, 0]),
        (1, 3, [0, 1, 2, 3], [0, 2, 1, 3]),
    ] {
        for (index, want) in before.into_iter().zip(after) {
            assert_eq!(
                moved_index(index, from, to),
                want,
                "{from} into {to}: {index}"
            );
        }
    }
}

/// The chevron has to appear while a chip is *partly* out of sight, not
/// only once one is wholly gone — the half a reader cannot see is the
/// half the menu exists to reach.
///
/// Self-calibrating, because a chip hugs its label and the theme's
/// padding: measure the strip's chips at a width that fits them all,
/// then cut the band to halfway through the last one. That chip then
/// starts inside the band and ends outside it, which is the case a
/// clipped rect cannot tell from a chip wholly inside.
#[test]
fn a_partly_clipped_chip_raises_the_overflow_chevron() {
    let build = |width: f32| {
        move |ui: &mut Ui| {
            let items = items(ui, TabBadge::None);
            TabStrip::new(&items)
                .id(strip_id())
                .selected(0)
                .overflow(TabOverflow::Menu)
                .size((Sizing::fixed(width), Sizing::HUG))
                .show(ui);
        }
    };
    let chevron = strip_id().with("overflow");
    let last = TabStrip::chip_id(strip_id(), 30);

    // Wide enough for all three: no chip is cut, and no chevron.
    let mut h = UiHarness::new(SURFACE);
    h.prime(2, build(SURFACE.x as f32));
    let whole = h.arranged(last);
    let strip_left = h.arranged(strip_id()).min.x;
    assert!(
        h.rect(chevron).is_none(),
        "premise: nothing is hidden, so nothing offers a menu",
    );

    // Half of that chip, and no more: it starts inside the band and ends
    // past it. Measured from the strip's own left edge, since that is
    // what the width below is a width of.
    let half_way = whole.min.x + whole.size.w * 0.5 - strip_left;
    let mut h = UiHarness::new(SURFACE);
    h.prime(2, build(half_way));
    assert!(
        h.rect(chevron).is_some(),
        "a chip cut in half is a chip the strip cannot show whole",
    );

    // The hidden chip is one pick away: the chevron opens the menu, and
    // its row reports the chip as a menu pick, which `activated` merges
    // with a click and a keyboard move.
    h.click_on(chevron);
    h.frame(build(half_way));
    h.click_on(strip_id().with("overflow_menu").with(30u64));
    let picked = h.frame_value(|ui| {
        let items = items(ui, TabBadge::None);
        let r = TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .overflow(TabOverflow::Menu)
            .size((Sizing::fixed(half_way), Sizing::HUG))
            .show(ui);
        (r.menu_picked, r.clicked, r.activated())
    });
    assert_eq!(picked, (Some(2), None, Some(2)));
}
