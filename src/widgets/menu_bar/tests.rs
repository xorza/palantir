use super::*;
use crate::host::harness::Harness;
use crate::input::interaction::response_state::ResponseState;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::theme::palette::Palette;
use crate::widgets::context_menu::menu_item::MenuItem;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(600, 400);
const MENUS: [(&str, [&str; 2]); 3] = [
    ("File", ["New", "Quit"]),
    ("Edit", ["Undo", "Redo"]),
    ("View", ["Zoom", "Fit"]),
];

fn bar() -> WidgetId {
    WidgetId::from_hash("bar")
}

fn title(idx: u16) -> WidgetId {
    title_id(bar(), idx)
}

/// The three menus of [`MENUS`]; the row picked this frame, if any.
fn build(ui: &mut Ui) -> Option<&'static str> {
    build_first(ui, MENUS.len())
}

/// The first `count` menus of [`MENUS`]; the row picked this frame, if any.
fn build_first(ui: &mut Ui, count: usize) -> Option<&'static str> {
    let mut picked = None;
    MenuBar::new().id(bar()).show(ui, |menus| {
        for (label, rows) in MENUS.into_iter().take(count) {
            menus.menu(label, |ui, close| {
                for row in rows {
                    if MenuItem::new(row).id_salt(row).show(ui, close).clicked() {
                        picked = Some(row);
                    }
                }
            });
        }
    });
    picked
}

fn frame(h: &mut Harness) {
    h.frame(|ui| {
        build(ui);
    });
}

fn open(h: &Harness) -> Option<u16> {
    h.ui_ref()
        .state::<MenuBarRow>(bar())
        .and_then(|row| row.open)
        .map(|open| open.idx)
}

/// Whether menu `idx` recorded this frame.
fn recorded(h: &Harness, idx: u16) -> bool {
    h.ui_ref()
        .tree(Layer::Menu)
        .records
        .widget_id()
        .contains(&menu_id(title(idx)))
}

/// One click opens a title's menu, one on another title moves to its menu without a click to shut
/// the first, and one on the open title shuts it.
#[test]
fn a_click_opens_switches_and_shuts() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    assert_eq!(open(&h), None);

    h.click(title(0));
    frame(&mut h);
    assert_eq!(open(&h), Some(0), "a click opens File");
    assert!(recorded(&h, 0));

    // The open menu's click-eater lies over the titles, so aim by position.
    h.click(h.center_of(title(2)));
    frame(&mut h);
    assert_eq!(open(&h), Some(2), "one click on View moves to it");
    assert!(
        recorded(&h, 2) && !recorded(&h, 0),
        "only View's menu records"
    );

    h.click(h.center_of(title(2)));
    frame(&mut h);
    assert_eq!(open(&h), None, "a click on the open title shuts it");
}

/// While a menu is open, moving onto another title opens that one's menu in the same frame; with
/// none open, a move opens nothing.
#[test]
fn a_move_onto_a_title_switches_only_while_a_menu_is_open() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    h.hover(title(1));
    frame(&mut h);
    assert_eq!(open(&h), None, "a hover opens nothing");

    h.click(title(0));
    frame(&mut h);
    h.hover(h.center_of(title(1)));
    frame(&mut h);
    assert_eq!(open(&h), Some(1));
    assert!(
        recorded(&h, 1) && !recorded(&h, 0),
        "Edit's menu records at once"
    );
    h.hover(h.center_of(title(2)));
    frame(&mut h);
    assert_eq!(open(&h), Some(2));
}

/// A menu the keyboard opens under a resting pointer stays open: only a move switches it.
#[test]
fn a_resting_pointer_over_another_title_does_not_switch() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    h.hover(title(1));
    frame(&mut h);
    h.ui().set_focus(title(0));
    frame(&mut h);
    h.key(Key::Enter);
    frame(&mut h);
    assert_eq!(open(&h), Some(0), "Enter opens the focused title's menu");
    h.prime(2, |ui| {
        build(ui);
    });
    assert_eq!(open(&h), Some(0), "the pointer did not move");
}

/// Right and Left move to the neighboring menu, wrapping: from File, Left reaches View. They do
/// from a title holding focus, as a click leaves it, and from a row the keyboard focused in the
/// open menu, which the next menu's first row then takes.
#[test]
fn the_arrows_move_between_menus_and_wrap() {
    for opened_by in ["click", "keyboard"] {
        let mut h = Harness::mono(SURFACE);
        h.prime(2, |ui| {
            build(ui);
        });
        if opened_by == "click" {
            h.click(title(0));
        } else {
            // Tab reaches the first title and shows the ring, so the menu takes focus.
            h.key(Key::Tab);
            frame(&mut h);
            assert_eq!(h.ui().focus(), Some(title(0)));
            h.key(Key::Enter);
        }
        frame(&mut h);
        frame(&mut h);
        assert_eq!(open(&h), Some(0), "{opened_by}");
        let in_menu = h.ui().focus().is_some_and(|focus| focus != title(0));
        assert_eq!(
            in_menu,
            opened_by == "keyboard",
            "{opened_by}: where focus is"
        );
        for (key, want) in [
            (Key::ArrowRight, 1),
            (Key::ArrowRight, 2),
            (Key::ArrowRight, 0),
            (Key::ArrowLeft, 2),
        ] {
            h.key(key);
            frame(&mut h);
            frame(&mut h);
            assert_eq!(open(&h), Some(want), "{opened_by} {key:?}");
            assert!(recorded(&h, want), "{opened_by} {key:?}: its menu records");
            if opened_by == "keyboard" {
                assert!(
                    h.ui().is_focus_within(menu_id(title(want))),
                    "{opened_by} {key:?}: the menu's row takes focus"
                );
            }
        }
    }
}

/// A picked row reports and shuts the menu; Esc and a click outside shut it too.
#[test]
fn a_pick_escape_or_an_outside_click_shuts_the_menu() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    h.click(title(1));
    frame(&mut h);
    let mut listed = Vec::new();
    h.ui().inspect(&mut listed);
    let redo = listed
        .iter()
        .find(|widget| widget.text == "Redo")
        .expect("Edit's menu shows Redo")
        .id;
    h.click(redo);
    assert_eq!(h.frame_value(build), Some("Redo"));
    assert_eq!(open(&h), None, "a pick shuts the menu");

    for shut in ["escape", "outside"] {
        h.click(title(1));
        frame(&mut h);
        assert_eq!(open(&h), Some(1), "{shut}");
        match shut {
            "escape" => drop(h.key(Key::Escape)),
            _ => h.click(Vec2::new(500.0, 350.0)),
        }
        frame(&mut h);
        assert_eq!(open(&h), None, "{shut} shuts the menu");
    }
}

/// A menu whose title the bar stops recording shuts with it, rather than keep the open slot
/// with no title to hang from.
#[test]
fn a_menu_the_bar_drops_shuts() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    h.click(title(2));
    frame(&mut h);
    assert_eq!(open(&h), Some(2));
    h.frame(|ui| {
        build_first(ui, 2);
    });
    assert_eq!(open(&h), None, "View's title is gone");
}

/// The open title wears [`MenuBarTheme::open`] whatever the pointer does, the hovered look of a
/// shut one; a shut title picks from [`MenuBarTheme::title`] by state.
#[test]
fn the_open_title_wears_the_open_look() {
    let theme = MenuBarTheme::default();
    let rest = ResponseState::default();
    assert_eq!(theme.look(&rest, MenuState::Open), &theme.open);
    assert_eq!(theme.look(&rest, MenuState::Shut), &theme.title.normal);
    assert_eq!(MenuState::from(true), MenuState::Open);
    assert_eq!(MenuState::from(false), MenuState::Shut);
    assert_eq!(theme.open, theme.title.hovered);
    assert_ne!(theme.open, theme.title.normal);
}

/// Unset, the bar spans its parent on the theme's band, its first title inset by the band's
/// padding: on the 600 px surface, from x = 0 to 600, the first title at `bar_padding.left`.
#[test]
fn the_bar_spans_its_parent_on_the_band() {
    let mut h = Harness::mono(SURFACE);
    frame(&mut h);
    let theme = h.ui().theme().menu_bar.clone();
    let band = h.rect(bar()).expect("the bar arranged");
    assert_eq!((band.min.x, band.size.w), (0.0, 600.0));
    let first = h.rect(title(0)).expect("the first title arranged");
    let [left, top, ..] = theme.bar_padding.to_array();
    assert_eq!((first.min.x, first.min.y), (left, top));
    let chrome = h
        .ui_ref()
        .tree(Layer::Main)
        .chrome(h.node_for_widget_id(bar()))
        .expect("the bar paints its band");
    let ShapeBrush::Solid(fill) = chrome.fill else {
        panic!("the band is a solid fill");
    };
    assert_eq!(fill, Palette::DEFAULT.surround.to_f16());
}
