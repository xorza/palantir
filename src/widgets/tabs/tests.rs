//! Chip identity, badge box, close-over-activate, keyboard travel, page binding.

use glam::{UVec2, Vec2};

use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::state::ScrollState;
use crate::widgets::tabs::tab_item::{TabBadge, TabItem};
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use crate::widgets::tabs::tabbed_view::{TabbedView, TabsAction};
use crate::widgets::text::Text;
use std::iter;

const SURFACE: UVec2 = UVec2::new(600, 200);

fn strip_id() -> WidgetId {
    WidgetId::from_hash("test.strip")
}

/// Three chips keyed 10 / 20 / 30 so a key can't be mistaken for a slot.
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

/// Chip ids come from the item key, not the slot, across reorders.
#[test]
fn chip_ids_follow_the_key_and_not_the_slot() {
    let mut h = UiHarness::new(SURFACE);
    h.prime(2, |ui| {
        let items = items(ui, TabBadge::None);
        TabStrip::new(&items).id(strip_id()).selected(0).show(ui);
    });
    let alpha = TabStrip::chip_id(strip_id(), 10);
    let before = h.rect(alpha).expect("alpha arranged");

    // Reversed; alpha keeps its id.
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

/// The badge changes visibility, never layout.
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

/// The selection cap adds no height.
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

/// Close wins over activate when one press reaches both.
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

/// Keyboard travel (WAI-ARIA tabs), reported apart from a pointer click.
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
    // Exact modifier match: the shift variant must not hit the plain arm.
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

/// A keyboard move to an out-of-sight chip pans the band to it next frame.
#[test]
fn a_keyboard_move_pans_the_band_to_the_chip() {
    use crate::primitives::math::domain::EPS;

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

/// An overflowing strip pans on horizontal wheel (and Shift+wheel), passes
/// vertical wheel to a scrollable page, and otherwise turns it horizontal.
#[test]
fn an_overflowing_strip_pans_sideways_and_chains_a_vertical_wheel() {
    let page = WidgetId::from_hash("page");
    let band = strip_id().with("band");
    // (case, page content height, page offset and band offset after a 25 px vertical turn)
    let cases: [(&str, f32, f32, f32); 2] = [
        ("the page scrolls, so it takes y", 600.0, 25.0, 30.0),
        ("the page fits, so the strip takes y", 50.0, 0.0, 55.0),
    ];
    for (label, filler_h, page_y, band_x) in cases {
        let build = |ui: &mut Ui| {
            let items = items(ui, TabBadge::None);
            Scroll::vertical()
                .id(page)
                .size((Sizing::fixed(300.0), Sizing::fixed(150.0)))
                .show(ui, |ui| {
                    TabStrip::new(&items)
                        .id(strip_id())
                        .selected(0)
                        .size((Sizing::fixed(90.0), Sizing::HUG))
                        .show(ui);
                    Block::new()
                        .id_salt("filler")
                        .size((Sizing::fixed(280.0), Sizing::fixed(filler_h)))
                        .show(ui);
                });
        };
        let mut h = UiHarness::new(SURFACE);
        h.prime(2, build);
        h.move_to(Vec2::new(40.0, 10.0));

        h.scroll_pixels(Vec2::new(20.0, 0.0));
        h.frame(build);
        assert_eq!(
            h.state::<ScrollState>(band).offset.x,
            20.0,
            "{label}: wheel x"
        );
        // What a Linux host makes of Shift+wheel y.
        h.scroll_pixels(Vec2::new(10.0, 0.0));
        h.frame(build);
        assert_eq!(
            h.state::<ScrollState>(band).offset.x,
            30.0,
            "{label}: Shift+wheel"
        );

        h.scroll_pixels(Vec2::new(0.0, 25.0));
        h.frame(build);
        assert_eq!(
            h.state::<ScrollState>(page).offset.y,
            page_y,
            "{label}: page"
        );
        assert_eq!(
            h.state::<ScrollState>(band).offset.x,
            band_x,
            "{label}: band"
        );
    }
}

/// Travel is scoped to focus.
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

/// A focused strip takes Ctrl+Tab and lets bare Tab pass to the app root.
#[test]
fn a_focused_strip_cycles_on_ctrl_tab_and_yields_bare_tab() {
    use crate::KeyFilter;
    use crate::input::shortcut::{Shortcut, ShortcutMods};

    let scene = |ui: &mut Ui, probe: Shortcut| {
        Panel::vstack()
            .id(WidgetId::from_hash("app-root"))
            .input_scope(KeyFilter::ACCEL | KeyFilter::FOCUS)
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

/// Insertion is a count of chip centres passed; checked on hand-placed rects.
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

/// Clicking a chip writes the bound index and records the page the same frame.
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

/// An out-of-range page index shows the last page without rewriting the binding;
/// an empty list records a strip with no page.
#[test]
fn a_stale_page_shows_the_last_one_and_no_pages_show_none() {
    let mut h = UiHarness::new(SURFACE);
    let view = WidgetId::from_hash("test.stale");
    let mut page = 7usize;
    let mut drawn = None;
    h.frame(|ui| {
        TabbedView::new(&mut page, &PAGES)
            .id(view)
            .show(ui, |_, index| drawn = Some(index));
    });
    assert_eq!((page, drawn), (7, Some(2)));

    let mut drawn = None;
    h.frame(|ui| {
        TabbedView::new(&mut page, &[] as &[&str])
            .id(view)
            .show(ui, |_, index| drawn = Some(index));
    });
    assert_eq!((page, drawn), (7, None));
    assert!(
        h.rect(view.with("strip")).is_some(),
        "the strip still records"
    );
}

/// A strip's selection is coerced the same way.
#[test]
fn a_stale_strip_selection_caps_the_last_chip() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 9, TabBadge::None);
    let capped = |key: u64| {
        let node = h
            .node_of(TabStrip::chip_id(strip_id(), key))
            .expect("chip")
            .node;
        h.ui.tree(Layer::Main).chrome(node).is_some()
    };
    assert_eq!([10, 20, 30].map(capped), [false, false, true]);
}

/// A drag release over another slot reports the move (the view can't reorder
/// a shared slice) and moves the bound index with it; gaps beside the chip
/// are no move, off-strip is no drop.
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

/// A keyed view names chips by page, not slot, for ids and drags.
#[test]
fn a_keyed_view_keeps_each_chip_with_its_page() {
    let view = WidgetId::from_hash("test.keyed");
    let strip = view.with("strip");
    let chip = |name: &str| TabStrip::chip_id(strip, WidgetId::from_hash(name).0);
    let record = |ui: &mut Ui, page: &mut usize, pages: &[&str]| {
        TabbedView::new(page, pages)
            .id(view)
            .reorderable(true)
            .keyed(|name: &&str| *name)
            .show(ui, |ui, _| {
                Panel::vstack()
                    .id_salt("page")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |_| {});
            })
            .action
    };

    let mut h = UiHarness::new(SURFACE);
    let mut page = 0usize;
    h.prime(2, |ui| {
        record(ui, &mut page, &PAGES);
    });
    let geometry_before = h.rect(chip("Geometry")).expect("Geometry's chip");
    let colour = h.rect(chip("Colour")).expect("Colour's chip");
    assert!(
        h.rect(TabStrip::chip_id(strip, 1)).is_none(),
        "no index keys"
    );
    h.frame(|ui| {
        record(ui, &mut page, &PAGES[1..]);
    });
    let geometry_after = h.rect(chip("Geometry")).expect("Geometry's chip survives");
    assert_eq!(
        geometry_after.min.x, colour.min.x,
        "it moved to the left edge"
    );
    assert!(geometry_after.min.x < geometry_before.min.x);

    let mut h = UiHarness::new(SURFACE);
    let mut page = 0usize;
    h.prime(2, |ui| {
        record(ui, &mut page, &PAGES);
    });
    h.press_on(chip("Colour"));
    let onto = h.center_of(chip("Metadata"));
    h.drag_to(Vec2::new(onto.x + 4.0, onto.y));
    h.frame(|ui| {
        record(ui, &mut page, &PAGES);
    });
    h.release();
    let action = h.frame_value(|ui| record(ui, &mut page, &PAGES));
    assert_eq!(action, Some(TabsAction::Reordered { from: 0, to: 3 }));
    assert_eq!(page, 2, "the bound page followed Colour to the end");
}

/// Where an index lands when `from` moves into gap `to`, for pages [A, B, C, D].
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

/// The chevron appears while a chip is *partly* out of sight. Self-calibrating:
/// measure the chips, then cut the band halfway through the last one.
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

    // Half of the last chip, measured from the strip's own left edge.
    let half_way = whole.min.x + whole.size.w * 0.5 - strip_left;
    let mut h = UiHarness::new(SURFACE);
    h.prime(2, build(half_way));
    assert!(
        h.rect(chevron).is_some(),
        "a chip cut in half is a chip the strip cannot show whole",
    );

    // The chevron opens the menu; its row reports a menu pick, which `activated` merges.
    h.click_on(chevron);
    h.frame(build(half_way));
    h.click_on(strip_id().with("overflow").with(30u64));
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

    // Menu state is the chevron's: it comes back closed.
    h.click_on(chevron);
    h.frame(build(half_way));
    assert!(
        ContextMenu::is_open(&h.ui, chevron),
        "premise: the menu is open"
    );
    h.frame(|_| {});
    h.frame(build(half_way));
    assert!(
        !ContextMenu::is_open(&h.ui, chevron),
        "the strip came back with its menu closed"
    );
}
