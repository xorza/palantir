//! Chip identity, chip content, the close cross, close-over-activate, keyboard travel, page binding.

use glam::{UVec2, Vec2};

use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::sense::Sense;
use crate::internals::ui_harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::ShapeRecord;
use crate::shape::style::LineCap;
use crate::theme::close_shown::CloseShown;
use crate::theme::tabs::TabsTheme;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use crate::widgets::panel::Panel;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::scroll::Scroll;
use crate::widgets::scroll::scroll_state::ScrollState;
use crate::widgets::tabs::reindex;
use crate::widgets::tabs::tab_item::TabItem;
use crate::widgets::tabs::tab_strip::internals::insertion_slot;
use crate::widgets::tabs::tab_strip::{TabOverflow, TabStrip};
use crate::widgets::tabs::tabbed_view::{TabbedView, TabsAction};
use crate::widgets::text::Text;
use std::iter;

const SURFACE: UVec2 = UVec2::new(600, 200);

fn strip_id() -> WidgetId {
    WidgetId::from_hash("test.strip")
}

/// Three chips keyed 10 / 20 / 30 so a key can't be mistaken for a slot.
fn items(ui: &mut Ui) -> Vec<TabItem> {
    [(10u64, "alpha"), (20, "beta"), (30, "gamma")]
        .into_iter()
        .map(|(key, label)| TabItem::new(key, ui.intern(label)))
        .collect()
}

/// One frame of a three-chip strip, with `selected` capped.
fn strip_frame(h: &mut UiHarness, selected: usize) {
    h.frame(|ui| {
        let items = items(ui);
        TabStrip::new(&items)
            .id(strip_id())
            .selected(selected)
            .show(ui, |ui, chip| chip.show_label(ui));
    });
}

/// Chip ids come from the item key, not the slot, across reorders.
#[test]
fn chip_ids_follow_the_key_and_not_the_slot() {
    let mut h = UiHarness::new(SURFACE);
    h.prime(2, |ui| {
        let items = items(ui);
        TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .show(ui, |ui, chip| chip.show_label(ui));
    });
    let alpha = TabStrip::chip_id(strip_id(), 10);
    let before = h.rect(alpha).expect("alpha arranged");

    // Reversed; alpha keeps its id.
    h.prime(2, |ui| {
        let mut items = items(ui);
        items.reverse();
        TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .show(ui, |ui, chip| chip.show_label(ui));
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

/// The content records between the chip's padding and its close button, handed the chip it
/// stands for in the ink the chip's look wears.
#[test]
fn the_content_records_inside_the_chip_before_its_close_button() {
    let mut h = UiHarness::new(SURFACE);
    let alpha = TabStrip::chip_id(strip_id(), 10);
    let dot = alpha.with("dot");
    let mut handed = Vec::new();
    h.prime(2, |ui| {
        handed.clear();
        let items = items(ui);
        TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .show(ui, |ui, chip| {
                handed.push((chip.slot, chip.key, chip.selected, chip.id));
                chip.show_label(ui);
                if chip.key == 10 {
                    Block::new()
                        .id(chip.id.with("dot"))
                        .size((Sizing::fixed(6.0), Sizing::fixed(6.0)))
                        .show(ui);
                }
            });
    });
    assert_eq!(
        handed,
        [
            (0, 10, true, alpha),
            (1, 20, false, TabStrip::chip_id(strip_id(), 20)),
            (2, 30, false, TabStrip::chip_id(strip_id(), 30)),
        ],
    );
    let label = h.rect(alpha.with("label")).expect("the label recorded");
    let dot = h.rect(dot).expect("the content's own widget recorded");
    let close = h
        .rect(TabStrip::close_id(strip_id(), 10))
        .expect("the close button recorded");
    let chip = h.rect(alpha).expect("arranged");
    let gap = h.ui.theme().tabs.label_gap;
    assert_eq!(
        (dot.min.x - label.max().x, close.min.x - dot.max().x),
        (gap, gap),
        "the content's parts and the close button sit `label_gap` apart: {label:?} {dot:?} {close:?}",
    );
    assert!(
        chip.min.x < label.min.x && close.max().x <= chip.max().x,
        "the content and the button are inside the chip: {chip:?}",
    );
}

/// The close cross is two round-capped lines in the close look's ink, crossing at the button's
/// center, and no text: on the stock 16 px button with an 8 px cross, from (4, 4) to (12, 12) and
/// from (12, 4) to (4, 12).
#[test]
fn the_close_cross_is_two_drawn_lines_in_the_close_ink() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);
    let theme = h.ui.theme();
    let t = &theme.tabs;
    assert_eq!((t.close_size, t.close_glyph), (16.0, 8.0), "premise");
    let stroke = t.close_stroke;
    let ink = t.close.normal.text.apply(&theme.text).color.to_f16();
    let node = h.node_for_widget_id(TabStrip::close_id(strip_id(), 20));
    let tree = h.ui.tree(Layer::Main);
    let mut lines = Vec::new();
    for shape in tree.shapes_of(node) {
        match shape {
            ShapeRecord::Curve {
                cap,
                basis: CurveBasis::Cubic(line),
                stroke: drawn,
                ..
            } => {
                assert_eq!(
                    (*cap, drawn.width, drawn.color),
                    (LineCap::Round, stroke, ink)
                );
                lines.push([line.p0, line.p3]);
            }
            ShapeRecord::Text { .. } => panic!("the cross is drawn, not a glyph"),
            _ => {}
        }
    }
    assert_eq!(
        lines,
        [
            [Vec2::new(4.0, 4.0), Vec2::new(12.0, 12.0)],
            [Vec2::new(12.0, 4.0), Vec2::new(4.0, 12.0)],
        ],
    );
}

/// Under `SelectedOrHovered`, an unselected chip at rest keeps its button's box unsensed, so it is
/// as wide as with the button shown and the pointer there reaches the chip; the pointer on the
/// chip shows the button in that box.
#[test]
fn a_hidden_close_keeps_its_box_until_the_pointer_shows_it() {
    let beta = TabStrip::chip_id(strip_id(), 20);
    let close = TabStrip::close_id(strip_id(), 20);
    let slot = close.with("slot");
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);
    let always = h.rect(beta).expect("arranged");
    assert!(
        h.rect(close).is_some(),
        "premise: `Always` shows every button"
    );

    h.ui.theme_mut().tabs.close_shown = CloseShown::SelectedOrHovered;
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);
    let rest = h.rect(beta).expect("arranged");
    assert_eq!(
        h.rect(close),
        None,
        "no button on an unselected chip at rest"
    );
    let close_size = h.ui.theme().tabs.close_size;
    let kept = h.rect(slot).expect("the button's box is kept");
    assert_eq!((kept.size.w, kept.size.h), (close_size, close_size));
    assert_eq!(
        rest.size, always.size,
        "the hidden button keeps the chip's width"
    );
    assert!(
        h.rect(TabStrip::close_id(strip_id(), 10)).is_some(),
        "the selected chip shows its button",
    );

    assert_eq!(
        h.hit_at(h.center_of(slot)),
        Some(beta),
        "the kept box senses nothing, so the pointer there reaches the chip",
    );

    h.move_to(h.center_of(beta.with("label")));
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);
    assert_eq!(
        h.rect(close),
        Some(kept),
        "the pointer on the chip shows its button where the box was kept",
    );
    assert_eq!(h.rect(slot), None);
    assert_eq!(h.rect(beta).expect("arranged").size, always.size);

    // A widget of the content's that senses the pointer is within the chip, so the pointer on it
    // keeps the button.
    let pin = beta.with("pin");
    let pinned = |h: &mut UiHarness| {
        h.frame(|ui| {
            let items = items(ui);
            TabStrip::new(&items)
                .id(strip_id())
                .selected(0)
                .show(ui, |ui, chip| {
                    chip.show_label(ui);
                    Block::new()
                        .id(chip.id.with("pin"))
                        .size((Sizing::fixed(8.0), Sizing::fixed(8.0)))
                        .sense(Sense::HOVER)
                        .show(ui);
                });
        });
    };
    h.move_to(Vec2::ZERO);
    pinned(&mut h);
    pinned(&mut h);
    assert_eq!(h.rect(close), None, "premise: the pointer left the chip");
    h.move_to(h.center_of(pin));
    pinned(&mut h);
    pinned(&mut h);
    assert_eq!(
        h.hit_at(h.center_of(pin)),
        Some(pin),
        "premise: the pin takes the pointer"
    );
    assert!(
        h.rect(close).is_some(),
        "the pointer on the content's own widget shows the button",
    );
}

/// The selection cap adds no height.
#[test]
fn the_selection_cap_adds_no_height() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);
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
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);

    h.click_on(TabStrip::close_id(strip_id(), 20));
    let hit = h.frame_value(|ui| {
        let items = items(ui);
        let r = TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .show(ui, |ui, chip| chip.show_label(ui));
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
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);

    h.click_on(TabStrip::chip_id(strip_id(), 30));
    let hit = h.frame_value(|ui| {
        let items = items(ui);
        let r = TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .show(ui, |ui, chip| chip.show_label(ui));
        (r.clicked, r.keyed, r.activated(), r.closed)
    });
    assert_eq!(hit, (Some(2), None, Some(2), None));
}

/// Keyboard travel (WAI-ARIA tabs), reported apart from a pointer click.
#[test]
fn arrows_home_and_end_travel_and_wrap() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);
    h.set_focus(strip_id());
    strip_frame(&mut h, 0);

    let travel = |h: &mut UiHarness, key: Key, mods: Modifiers, selected: usize| {
        h.set_modifiers(mods);
        h.key(key);
        let hit = h.frame_value(|ui| {
            let items = items(ui);
            let r = TabStrip::new(&items)
                .id(strip_id())
                .selected(selected)
                .show(ui, |ui, chip| chip.show_label(ui));
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
            let items = items(ui);
            TabStrip::new(&items)
                .id(strip_id())
                .show(ui, |ui, chip| chip.show_label(ui))
                .keyed
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
            let items = items(ui);
            TabStrip::new(&items)
                .id(strip_id())
                .selected(selected)
                .size((Sizing::fixed(120.0), Sizing::HUG))
                .show(ui, |ui, chip| chip.show_label(ui))
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
            let items = items(ui);
            Scroll::vertical()
                .id(page)
                .size((Sizing::fixed(300.0), Sizing::fixed(150.0)))
                .show(ui, |ui| {
                    TabStrip::new(&items)
                        .id(strip_id())
                        .selected(0)
                        .size((Sizing::fixed(90.0), Sizing::HUG))
                        .show(ui, |ui, chip| chip.show_label(ui));
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
            h.state::<ScrollState>(band).offset().x,
            20.0,
            "{label}: wheel x"
        );
        // What a Linux host makes of Shift+wheel y.
        h.scroll_pixels(Vec2::new(10.0, 0.0));
        h.frame(build);
        assert_eq!(
            h.state::<ScrollState>(band).offset().x,
            30.0,
            "{label}: Shift+wheel"
        );

        h.scroll_pixels(Vec2::new(0.0, 25.0));
        h.frame(build);
        assert_eq!(
            h.state::<ScrollState>(page).offset().y,
            page_y,
            "{label}: page"
        );
        assert_eq!(
            h.state::<ScrollState>(band).offset().x,
            band_x,
            "{label}: band"
        );
    }
}

/// Travel is scoped to focus.
#[test]
fn travel_needs_focus_inside_the_strip() {
    let mut h = UiHarness::new(SURFACE);
    strip_frame(&mut h, 0);
    strip_frame(&mut h, 0);
    h.clear_focus();
    h.key(Key::ArrowRight);
    let keyed = h.frame_value(|ui| {
        let items = items(ui);
        TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .show(ui, |ui, chip| chip.show_label(ui))
            .keyed
    });
    assert_eq!(keyed, None);
}

/// A focused strip takes Ctrl+Tab and lets bare Tab pass to the app root.
#[test]
fn a_focused_strip_cycles_on_ctrl_tab_and_yields_bare_tab() {
    use crate::input::key_class::KeyFilter;
    use crate::input::shortcut::{Shortcut, ShortcutMods};

    let scene = |ui: &mut Ui, probe: Shortcut| {
        Panel::vstack()
            .id(WidgetId::from_hash("app-root"))
            .input_scope(KeyFilter::ACCEL | KeyFilter::FOCUS)
            .show(ui, |ui| {
                let at_root = ui.shortcut_pressed(probe);
                let items = items(ui);
                let keyed = TabStrip::new(&items)
                    .id(strip_id())
                    .selected(0)
                    .show(ui, |ui, chip| chip.show_label(ui))
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

/// Insertion is a count of chip centers passed; checked on hand-placed rects.
#[test]
fn the_insertion_slot_counts_the_centers_passed() {
    let chips = [
        Rect::new(0.0, 0.0, 40.0, 20.0),
        Rect::new(50.0, 0.0, 40.0, 20.0),
        Rect::new(100.0, 0.0, 40.0, 20.0),
    ];
    // Centers sit at 20, 70 and 120.
    let slot = |x: f32| insertion_slot(chips.iter().copied(), x);
    assert_eq!(slot(-5.0), 0, "before the first chip");
    assert_eq!(slot(19.0), 0, "the leading half of the first chip");
    assert_eq!(slot(21.0), 1, "past the first center");
    assert_eq!(slot(71.0), 2);
    assert_eq!(slot(500.0), 3, "past every center appends");
    assert_eq!(
        insertion_slot(iter::empty(), 0.0),
        0,
        "an empty strip has one slot"
    );
}

/// A drop reads chips at their uncut extents: with the band panned so `alpha` is cut on the
/// left, a pointer between its true center and its visible one has passed it. The caret sits in
/// the gutter before its slot's chip, and inside the band where that gutter is out of sight.
#[test]
fn a_drop_slot_and_its_caret_read_uncut_chip_extents() {
    let theme = TabsTheme::default();
    let (width, half_gap) = (theme.caret_width, theme.gap * 0.5);
    let caret = strip_id().with("caret");
    let build = |slot: usize| {
        move |ui: &mut Ui| {
            let items = items(ui);
            TabStrip::new(&items)
                .id(strip_id())
                .selected(0)
                .drop_caret(Some(slot))
                .size((Sizing::fixed(120.0), Sizing::HUG))
                .show(ui, |ui, chip| chip.show_label(ui));
        }
    };
    let extent = |h: &UiHarness, key: u64| {
        let chip = TabStrip::chip_id(strip_id(), key);
        h.transform(chip).apply_rect(h.arranged(chip))
    };
    let clip = |h: &UiHarness| {
        h.rect(strip_id().with("band"))
            .expect("the band arranged")
            .deflated_by(theme.strip_padding)
    };
    let mark = |h: &UiHarness, x: f32| {
        let clip = clip(h);
        Rect::new(x - width * 0.5, clip.min.y, width, clip.size.h)
    };

    let mut h = UiHarness::new(SURFACE);
    h.prime(2, build(1));
    let gutter = extent(&h, 20).min.x - half_gap;
    assert!(
        clip(&h).min.x < gutter && gutter < clip(&h).max().x,
        "premise: in sight"
    );
    assert_eq!(
        h.rect(caret),
        Some(mark(&h, gutter)),
        "the gutter before beta"
    );
    h.prime(2, build(3));
    assert!(
        extent(&h, 30).max().x > clip(&h).max().x,
        "premise: gamma is cut"
    );
    assert_eq!(
        h.rect(caret),
        Some(mark(&h, clip(&h).max().x)),
        "the append, held at the band's right edge"
    );

    h.move_to(Vec2::new(40.0, 10.0));
    h.scroll_pixels(Vec2::new(30.0, 0.0));
    h.prime(2, build(0));
    let uncut = extent(&h, 10);
    let seen = h
        .rect(TabStrip::chip_id(strip_id(), 10))
        .expect("alpha shows in part");
    assert!(
        uncut.center().x < seen.center().x,
        "premise: alpha is cut on the left"
    );
    let x = f32::midpoint(uncut.center().x, seen.center().x);
    assert_eq!(
        insertion_slot([seen], x),
        0,
        "the cut rect has not been passed"
    );
    assert_eq!(TabStrip::drop_slot(&h.ui, strip_id(), [10, 20, 30], x), 1);
    assert_eq!(
        h.rect(caret),
        Some(mark(&h, clip(&h).min.x)),
        "the gutter before alpha, held at the band's left edge"
    );
}

const PAGES: [&str; 3] = ["Color", "Geometry", "Metadata"];

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
            .show(ui, |ui, idx| {
                *drawn = idx;
                Text::new(PAGES[idx]).id_salt("body").show(ui);
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
    assert_eq!(action, Some(TabsAction::Activated { idx: 2 }));
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
            .show(ui, |_, idx| drawn = Some(idx));
    });
    assert_eq!((page, drawn), (7, Some(2)));

    let mut drawn = None;
    let no_tabs: [&str; 0] = [];
    h.frame(|ui| {
        TabbedView::new(&mut page, &no_tabs)
            .id(view)
            .show(ui, |_, idx| drawn = Some(idx));
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
    strip_frame(&mut h, 9);
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
            "past the last center: the append",
            |h, strip| {
                let onto = h.center_of(TabStrip::chip_id(strip, 2));
                Vec2::new(onto.x + 4.0, onto.y)
            },
            Some(TabsAction::Reordered { from: 0, to: 3 }),
            2,
        ),
        (
            "just past its own center: the gap beside it",
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

/// A drag shows the strip's caret at the slot under the pointer, and Escape cancels it: the
/// caret goes and the release reorders nothing.
#[test]
fn an_escape_cancels_a_view_drag_and_its_caret() {
    let view = WidgetId::from_hash("test.cancel");
    let strip = view.with("strip");
    let caret = strip.with("caret");
    let record = |ui: &mut Ui, page: &mut usize| {
        TabbedView::new(page, &PAGES)
            .id(view)
            .reorderable(true)
            .show(ui, |_, _| {})
            .action
    };
    let mut h = UiHarness::new(SURFACE);
    let mut page = 0usize;
    h.prime(2, |ui| {
        record(ui, &mut page);
    });
    h.press_on(TabStrip::chip_id(strip, 0));
    let onto = h.center_of(TabStrip::chip_id(strip, 2));
    h.drag_to(Vec2::new(onto.x + 4.0, onto.y));
    h.prime(2, |ui| {
        record(ui, &mut page);
    });
    assert!(h.rect(caret).is_some(), "the carried chip marks its slot");

    h.key(Key::Escape);
    h.frame(|ui| {
        record(ui, &mut page);
    });
    assert!(h.rect(caret).is_none(), "Escape takes the caret away");
    h.release();
    assert_eq!(h.frame_value(|ui| record(ui, &mut page)), None);
    assert_eq!(page, 0, "the cancelled drag moved nothing");
}

/// A close rewrites the binding for the collection after the removal, so a caller that removes
/// the page keeps showing the right one: [Color, Geometry, Metadata] showing `page`, closing
/// `closed`.
#[test]
fn a_close_moves_the_binding_with_the_shown_page() {
    let view = WidgetId::from_hash("test.close");
    let strip = view.with("strip");
    let record = |ui: &mut Ui,
                  bound: &mut usize,
                  pages: &mut Vec<&'static str>,
                  drawn: &mut &'static str| {
        let action = TabbedView::new(bound, pages.as_slice())
            .id(view)
            .show(ui, |_, idx| *drawn = pages[idx])
            .action;
        if let Some(TabsAction::Closed { idx }) = action {
            pages.remove(idx);
        }
        action
    };
    for (page, closed, want, shows) in [
        (2, 0, 1, "Metadata"),
        (2, 2, 1, "Geometry"),
        (0, 0, 0, "Geometry"),
        (1, 2, 1, "Geometry"),
    ] {
        let mut bound = page;
        let mut pages = PAGES.to_vec();
        let mut drawn = "";
        let mut h = UiHarness::new(SURFACE);
        h.prime(2, |ui| {
            record(ui, &mut bound, &mut pages, &mut drawn);
        });
        h.click_on(TabStrip::close_id(strip, u64::try_from(closed).unwrap()));
        let action = h.frame_value(|ui| record(ui, &mut bound, &mut pages, &mut drawn));
        assert_eq!(action, Some(TabsAction::Closed { idx: closed }));
        assert_eq!(bound, want, "showing {page}, closing {closed}");
        h.frame(|ui| {
            record(ui, &mut bound, &mut pages, &mut drawn);
        });
        assert_eq!(drawn, shows, "showing {page}, closing {closed}");
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
    let color = h.rect(chip("Color")).expect("Color's chip");
    assert!(
        h.rect(TabStrip::chip_id(strip, 1)).is_none(),
        "no index keys"
    );
    h.frame(|ui| {
        record(ui, &mut page, &PAGES[1..]);
    });
    let geometry_after = h.rect(chip("Geometry")).expect("Geometry's chip survives");
    assert_eq!(
        geometry_after.min.x, color.min.x,
        "it moved to the left edge"
    );
    assert!(geometry_after.min.x < geometry_before.min.x);

    let mut h = UiHarness::new(SURFACE);
    let mut page = 0usize;
    h.prime(2, |ui| {
        record(ui, &mut page, &PAGES);
    });
    h.press_on(chip("Color"));
    let onto = h.center_of(chip("Metadata"));
    h.drag_to(Vec2::new(onto.x + 4.0, onto.y));
    h.frame(|ui| {
        record(ui, &mut page, &PAGES);
    });
    h.release();
    let action = h.frame_value(|ui| record(ui, &mut page, &PAGES));
    assert_eq!(action, Some(TabsAction::Reordered { from: 0, to: 3 }));
    assert_eq!(page, 2, "the bound page followed Color to the end");
}

/// Where an index lands when `from` moves into gap `to`, for pages [A, B, C, D]; and where
/// the visible index goes when one page closes.
#[test]
fn a_move_and_a_close_carry_every_index_with_its_page() {
    for (from, to, after) in [
        (0, 4, [3, 0, 1, 2]),
        (3, 0, [1, 2, 3, 0]),
        (1, 3, [0, 2, 1, 3]),
        (1, 1, [0, 1, 2, 3]),
        (1, 2, [0, 1, 2, 3]),
    ] {
        for (idx, want) in after.into_iter().enumerate() {
            assert_eq!(
                reindex::after_move(idx, from, to),
                want,
                "{from} into {to}: {idx}"
            );
        }
        assert_eq!(reindex::landing(from, to), after[from], "{from} into {to}");
    }

    // [A, B, C, D] showing `idx`, closing `removed`: an earlier close slides the
    // index down, a later one leaves it, closing the shown page shows the one
    // that took its slot, or the one before where it was last.
    for (idx, removed, want) in [(2, 0, 1), (2, 3, 2), (2, 2, 2), (3, 3, 2), (0, 0, 0)] {
        assert_eq!(
            reindex::after_removal(idx, removed, 4),
            want,
            "showing {idx}, closing {removed}"
        );
    }
    assert_eq!(reindex::after_removal(0, 0, 1), 0, "an emptied strip");
}

/// The chevron appears while a chip is *partly* out of sight. Self-calibrating:
/// measure the chips, then cut the band halfway through the last one.
#[test]
fn a_partly_clipped_chip_raises_the_overflow_chevron() {
    let build = |width: f32| {
        move |ui: &mut Ui| {
            let items = items(ui);
            TabStrip::new(&items)
                .id(strip_id())
                .selected(0)
                .overflow(TabOverflow::Menu)
                .size((Sizing::fixed(width), Sizing::HUG))
                .show(ui, |ui, chip| chip.show_label(ui));
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
    // Three dots in the cross's proportions: the stock 8 px cross at a 1.5 px stroke spaces them
    // 8 · 7 / 12 ≈ 4.67 apart about the 16 px button's center, each 2 · 1.5 = 3 across.
    let node = h.node_for_widget_id(chevron);
    let dots: Vec<Rect> =
        h.ui.tree(Layer::Main)
            .shapes_of(node)
            .filter_map(|shape| match shape {
                ShapeRecord::Quad(QuadShape::Rect {
                    local_rect: Some(rect),
                    ..
                }) => Some(*rect),
                _ => None,
            })
            .collect();
    let pitch = 8.0 * 7.0 / 12.0;
    assert_eq!(
        dots,
        [-pitch, 0.0, pitch].map(|dx| Rect::new(8.0 + dx - 1.5, 6.5, 3.0, 3.0)),
    );

    // The chevron opens the menu; its row reports a menu pick, which `activated` merges.
    h.click_on(chevron);
    h.frame(build(half_way));
    h.click_on(strip_id().with("overflow").with(30u64));
    let picked = h.frame_value(|ui| {
        let items = items(ui);
        let r = TabStrip::new(&items)
            .id(strip_id())
            .selected(0)
            .overflow(TabOverflow::Menu)
            .size((Sizing::fixed(half_way), Sizing::HUG))
            .show(ui, |ui, chip| chip.show_label(ui));
        (r.menu_picked, r.clicked, r.activated())
    });
    assert_eq!(picked, (Some(2), None, Some(2)));

    // Menu state is the chevron's: it comes back closed.
    h.click_on(chevron);
    h.frame(build(half_way));
    assert!(
        PopupTrigger::is_open(&h.ui, chevron),
        "premise: the menu is open"
    );
    h.frame(|_| {});
    h.frame(build(half_way));
    assert!(
        !PopupTrigger::is_open(&h.ui, chevron),
        "the strip came back with its menu closed"
    );
}
