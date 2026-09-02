//! Chip identity, the reserved badge box, close-over-activate ordering,
//! keyboard travel, and the page binding a tabbed view writes.

use glam::{UVec2, Vec2};

use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::ui::harness::UiHarness;
use crate::widgets::tabs::tab_item::{TabBadge, TabItem};
use crate::widgets::tabs::tab_strip::TabStrip;
use crate::widgets::tabs::tabbed_view::{TabbedView, TabsAction};
use crate::widgets::text::Text;

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

    let at = h.center_of(TabStrip::close_id(strip_id(), 20));
    h.click_at(at);
    let hit = h.frame_value(|ui| {
        let items = items(ui, TabBadge::None);
        let r = TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
        (r.clicked, r.closed)
    });
    assert_eq!(hit, (None, Some(1)), "the close won over the activation");
}

/// A plain chip click reports its slot, and nothing else.
#[test]
fn a_chip_click_reports_its_slot() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);

    let at = h.center_of(TabStrip::chip_id(strip_id(), 30));
    h.click_at(at);
    let hit = h.frame_value(|ui| {
        let items = items(ui, TabBadge::None);
        let r = TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
        (r.clicked, r.keyed, r.closed)
    });
    assert_eq!(hit, (Some(2), None, None));
}

/// Keyboard travel on the WAI-ARIA tab pattern. Reported apart from a
/// pointer click, because a caller that polls the chips itself one phase
/// earlier already holds the click and would otherwise act on it twice.
#[test]
fn arrows_home_and_end_travel_and_wrap() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);
    h.request_focus(Some(strip_id()));
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
}

/// Travel is scoped to focus: the same press with the strip unfocused
/// moves nothing, so an application's own arrow handling keeps working.
#[test]
fn travel_needs_focus_inside_the_strip() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0, TabBadge::None);
    strip_frame(&mut h, 0, TabBadge::None);
    h.request_focus(None);
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
        TabStrip::insertion_slot(std::iter::empty(), 0.0),
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
    let at = h.center_of(TabStrip::chip_id(strip, 2));
    h.click_at(at);
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
/// making it — the view holds a shared slice and cannot reorder it.
#[test]
fn a_reorderable_view_reports_the_slot_a_drag_released_over() {
    let mut h = UiHarness::new(SURFACE);
    let view = WidgetId::from_hash("test.reorder");
    let mut page = 0usize;
    let record = |ui: &mut Ui, page: &mut usize| {
        TabbedView::new(page, &PAGES)
            .id(view)
            .reorderable(true)
            .show(ui, |_, _| {})
            .action
    };
    h.prime(2, |ui| {
        record(ui, &mut page);
    });

    let strip = view.with("strip");
    let from = h.center_of(TabStrip::chip_id(strip, 0));
    let onto = h.center_of(TabStrip::chip_id(strip, 2));
    h.press_at(from);
    h.drag_to(Vec2::new(onto.x + 4.0, onto.y));
    h.frame(|ui| {
        record(ui, &mut page);
    });
    h.release();
    let action = h.frame_value(|ui| record(ui, &mut page));
    assert_eq!(
        action,
        Some(TabsAction::Reordered { from: 0, to: 3 }),
        "released past the last centre, so the target slot is the append"
    );
}
