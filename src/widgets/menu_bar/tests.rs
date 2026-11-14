use super::*;
use crate::host::harness::Harness;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::input::interaction::pointer_button_phase::PointerButtonPhase;
use crate::primitives::input::interaction::pointer_button_state::PointerButtonState;
use crate::primitives::input::interaction::response::Response;
use crate::scene::layer::Layer;
use crate::shape::paint::lowered_brush::LoweredBrush;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::palette::Palette;
use crate::widgets::menu::menu_item::MenuItem;
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
    MenuBar::title_id(bar(), MENUS[usize::from(idx)].0)
}

/// The three menus of [`MENUS`]; the row picked this frame, if any.
fn build(ui: &mut Ui) -> Option<&'static str> {
    build_first(ui, MENUS.len())
}

/// The first `count` menus of [`MENUS`]; the row picked this frame, if any.
fn build_first(ui: &mut Ui, count: usize) -> Option<&'static str> {
    build_some(ui, &[0, 1, 2][..count])
}

/// The menus of [`MENUS`] at `idxs`; the row picked this frame, if any.
fn build_some(ui: &mut Ui, idxs: &[usize]) -> Option<&'static str> {
    let mut picked = None;
    MenuBar::new().id(bar()).show(ui, |menus| {
        for (label, rows) in idxs.iter().map(|&idx| MENUS[idx]) {
            menus.menu(label, |ui| {
                for row in rows {
                    if MenuItem::new(row).id_salt(row).show(ui).clicked() {
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
    let open = h.ui().state::<MenuBarRow>(bar())?.open?;
    (0..MENUS.len() as u16).find(|&idx| title(idx) == open.title)
}

/// Whether menu `idx` recorded this frame.
fn recorded(h: &Harness, idx: u16) -> bool {
    h.ui()
        .tree(Layer::Menu)
        .records
        .widget_id()
        .contains(&Menu::menu_id(title(idx)))
}

/// One click opens a title's menu, one on another title moves to its menu without a click to close
/// the first, and one on the open title closes it.
#[test]
fn a_click_opens_switches_and_closes() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    assert_eq!(open(&h), None);

    h.click(title(0));
    frame(&mut h);
    assert_eq!(open(&h), Some(0), "a click opens File");
    assert!(recorded(&h, 0));

    // The open menu's barrier lies over the titles, so aim by position.
    h.click(h.center_of(title(2)));
    frame(&mut h);
    assert_eq!(open(&h), Some(2), "one click on View moves to it");
    assert!(
        recorded(&h, 2) && !recorded(&h, 0),
        "only View's menu records"
    );

    h.click(h.center_of(title(2)));
    frame(&mut h);
    assert_eq!(open(&h), None, "a click on the open title closes it");
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
    h.ui_mut().set_focus(title(0));
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
            // One frame: its second pass records the next menu, with no later frame asked for it.
            h.key(key);
            frame(&mut h);
            assert_eq!(open(&h), Some(want), "{opened_by} {key:?}");
            assert!(recorded(&h, want), "{opened_by} {key:?}: its menu records");
            if opened_by == "keyboard" {
                assert!(
                    h.ui().is_focus_within(Menu::menu_id(title(want))),
                    "{opened_by} {key:?}: the menu's row takes focus"
                );
            }
        }
    }
}

/// On a focused title, Down opens its menu on the first row and Up on the last, as the APG
/// menubar does: "Quit", where Up lands, sits below "New", where Down lands.
#[test]
fn down_and_up_open_a_title_on_its_first_and_last_row() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    h.key(Key::Tab);
    frame(&mut h);
    assert_eq!(h.ui().focus(), Some(title(0)));
    let mut landed = Vec::new();
    for key in [Key::ArrowDown, Key::ArrowUp] {
        h.key(key);
        frame(&mut h);
        frame(&mut h);
        assert_eq!(open(&h), Some(0), "{key:?} opens the menu");
        let row = h.ui().focus().expect("a row takes focus");
        assert!(
            h.ui().is_focus_within(Menu::menu_id(title(0))),
            "{key:?}: focus is in the menu"
        );
        landed.push(h.rect(row).expect("the row is arranged").min.y);
        h.key(Key::Escape);
        frame(&mut h);
        frame(&mut h);
        assert_eq!(open(&h), None, "{key:?}: Escape closes");
        assert_eq!(h.ui().focus(), Some(title(0)), "{key:?}: focus returns");
    }
    assert!(landed[1] > landed[0], "Up lands below Down: {landed:?}");
}

/// A picked row reports and closes the menu; Esc and a click outside close it too.
#[test]
fn a_pick_escape_or_an_outside_click_closes_the_menu() {
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
    assert_eq!(open(&h), None, "a pick closes the menu");

    for how in ["escape", "outside"] {
        h.click(title(1));
        frame(&mut h);
        assert_eq!(open(&h), Some(1), "{how}");
        match how {
            "escape" => h.key(Key::Escape),
            _ => h.click(Vec2::new(500.0, 350.0)),
        };
        frame(&mut h);
        assert_eq!(open(&h), None, "{how} closes the menu");
    }
}

/// A menu whose title the bar stops recording closes with it, rather than keep the open slot
/// with no title to hang from.
#[test]
fn a_menu_the_bar_drops_closes() {
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

/// A title is keyed by its label, not its place: with File dropped before it, View keeps its open
/// menu, and the arrows walk the titles the bar shows now.
#[test]
fn a_menu_dropped_before_the_open_one_leaves_it_open() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    h.click(title(2));
    frame(&mut h);
    assert_eq!(open(&h), Some(2));
    h.frame(|ui| {
        build_some(ui, &[1, 2]);
    });
    assert_eq!(open(&h), Some(2), "View stays open");
    assert!(recorded(&h, 2));
    h.key(Key::ArrowRight);
    h.frame(|ui| {
        build_some(ui, &[1, 2]);
    });
    assert_eq!(
        open(&h),
        Some(1),
        "Right from View wraps to Edit, past the dropped File"
    );
}

/// The open title wears the hovered look of a closed one whatever the pointer does, a pressed or a
/// disabled one alike; a closed title picks from its unselected pack by state.
#[test]
fn the_open_title_wears_the_open_look() {
    let theme = Theme::stock(ColorScheme::Dark).menu_bar.title;
    let hovered = &theme.unselected.hovered;
    let pressed = Response {
        pointer_over: true,
        left: PointerButtonState {
            phase: PointerButtonPhase::Held,
            ..PointerButtonState::default()
        },
        ..Response::idle(WidgetId::from_hash("probe"))
    };
    assert!(pressed.is_pressed(), "premise");
    let disabled = Response {
        disabled: true,
        ..Response::idle(WidgetId::from_hash("probe"))
    };
    for (label, state) in [
        ("rest", Response::idle(WidgetId::from_hash("probe"))),
        ("pressed", pressed),
        ("disabled", disabled),
    ] {
        assert_eq!(theme.look(&state, Selection::Selected), hovered, "{label}");
    }
    let rest = Response::idle(WidgetId::from_hash("probe"));
    assert_eq!(
        theme.look(&rest, Selection::Unselected),
        &theme.unselected.normal
    );
    assert_ne!(hovered, &theme.unselected.normal);
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
        .ui()
        .tree(Layer::Main)
        .chrome(h.node_for_widget_id(bar()))
        .expect("the bar paints its band");
    let LoweredBrush::Solid(fill) = chrome.fill else {
        panic!("the band is a solid fill");
    };
    assert_eq!(fill, Palette::DARK.surround.to_f16());
}

/// `menu_style` restyles the menus the titles drop: a padding 9 px wider on each edge makes the
/// open menu 2 × 9 taller than the theme's.
#[test]
fn menu_style_restyles_the_menus() {
    let height = |styled: bool| {
        let mut menu = Theme::stock(ColorScheme::Dark).menu;
        menu.padding = menu.padding + Spacing::all(9.0);
        let mut scene = |ui: &mut Ui| {
            MenuBar::new()
                .id(bar())
                .menu_style(styled.then_some(&menu))
                .show(ui, |menus| {
                    menus.menu("File", |ui| {
                        MenuItem::new("New").show(ui);
                    });
                });
        };
        let mut h = Harness::mono(SURFACE);
        h.frame(&mut scene);
        let at = h.center_of(title(0));
        h.click(at);
        h.frame(&mut scene);
        h.rect(Menu::menu_id(title(0)))
            .expect("the menu is open")
            .size
            .h
    };
    assert_eq!(height(true) - height(false), 2.0 * 9.0);
}

/// `open`, `close` and `is_open` act on the bar as it stands when a menu's own body calls them,
/// while the bar records: `is_open` reads the open menu, `close` closes it, and `open` moves the
/// bar's one slot to a title recorded after it in the same frame.
#[test]
fn open_close_and_is_open_act_from_inside_a_menu_body() {
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Drive {
        Nothing,
        Close,
        OpenView,
    }
    /// The bar with `drive` run in the first row of whichever menu is open; what `is_open`
    /// read there.
    fn build_driving(ui: &mut Ui, drive: Drive) -> Option<bool> {
        let mut read = None;
        MenuBar::new().id(bar()).show(ui, |menus| {
            for (label, rows) in MENUS {
                menus.menu(label, |ui| {
                    read = Some(MenuBar::is_open(ui, bar()));
                    match drive {
                        Drive::Nothing => {}
                        Drive::Close => MenuBar::close(ui, bar()),
                        Drive::OpenView => MenuBar::open(ui, bar(), "View"),
                    }
                    for row in rows {
                        MenuItem::new(row).id_salt(row).show(ui);
                    }
                });
            }
        });
        read
    }
    /// A bar with Edit open and settled.
    fn edit_open() -> Harness {
        let mut h = Harness::mono(SURFACE);
        h.prime(2, |ui| {
            build_driving(ui, Drive::Nothing);
        });
        MenuBar::open(h.ui_mut(), bar(), "Edit");
        h.prime(2, |ui| {
            build_driving(ui, Drive::Nothing);
        });
        assert_eq!(open(&h), Some(1));
        h
    }

    let mut h = edit_open();
    let mut read = None;
    h.frame(|ui| read = build_driving(ui, Drive::Nothing));
    assert_eq!(read, Some(true), "the open menu's body reads its bar open");

    let mut h = edit_open();
    h.frame(|ui| {
        build_driving(ui, Drive::Close);
    });
    assert_eq!(open(&h), None, "a close from the menu's body closes it");
    h.frame(|ui| {
        build_driving(ui, Drive::Nothing);
    });
    assert!(!recorded(&h, 1) && !MenuBar::is_open(h.ui(), bar()));

    let mut h = edit_open();
    h.frame(|ui| {
        build_driving(ui, Drive::OpenView);
    });
    assert_eq!(open(&h), Some(2), "an open from Edit's body moves the slot");
    assert!(
        recorded(&h, 2),
        "View records after Edit, so it opens this frame"
    );
    h.frame(|ui| {
        build_driving(ui, Drive::Nothing);
    });
    assert_eq!(open(&h), Some(2));
    assert!(!recorded(&h, 1) && recorded(&h, 2));
}

/// `open`, `close` and `is_open` drive the bar from outside: `open` names the menu by its label
/// and moves the bar's one open menu, a label the bar does not show opens nothing, and `close`
/// closes it.
#[test]
fn open_close_and_is_open_drive_the_bar_from_outside() {
    let mut h = Harness::mono(SURFACE);
    h.prime(2, |ui| {
        build(ui);
    });
    assert!(!MenuBar::is_open(h.ui(), bar()));
    MenuBar::open(h.ui_mut(), bar(), "Edit");
    frame(&mut h);
    assert_eq!(open(&h), Some(1));
    assert!(recorded(&h, 1) && MenuBar::is_open(h.ui(), bar()));
    MenuBar::open(h.ui_mut(), bar(), "View");
    frame(&mut h);
    assert_eq!(open(&h), Some(2));
    assert!(!recorded(&h, 1) && recorded(&h, 2));
    MenuBar::close(h.ui_mut(), bar());
    frame(&mut h);
    assert_eq!(open(&h), None);
    assert!(!recorded(&h, 2) && !MenuBar::is_open(h.ui(), bar()));
    MenuBar::open(h.ui_mut(), bar(), "Help");
    frame(&mut h);
    assert_eq!(open(&h), None, "a label the bar does not show");
}

/// A modal that opens with no press, as on a close the window manager asks for, closes the open
/// menu in its own frame, before the title records: the menu does not record in the frame's last
/// pass, the bar holds no open menu, and the title paints the closed look it had before the menu
/// opened, not the open one. A modal recorded after the bar takes two passes for it, one recorded
/// ahead of the bar takes one.
#[test]
fn an_opening_modal_closes_the_menu_and_its_open_look_in_its_frame() {
    use crate::widgets::modal::Modal;

    let title_fill = |h: &Harness| {
        h.ui()
            .tree(Layer::Main)
            .chrome(h.node_for_widget_id(title(1)))
            .map(|chrome| match chrome.fill {
                LoweredBrush::Solid(fill) => fill,
                other @ LoweredBrush::Gradient { .. } => panic!("a title fills solid: {other:?}"),
            })
    };
    for (modal_first, want_passes) in [(false, 2), (true, 1)] {
        // Reduced motion snaps the title's look, so each frame paints the look its state picks.
        let mut h = Harness::mono(SURFACE);
        h.set_system_motion_reduced(true);
        h.prime(2, |ui| {
            build(ui);
        });
        let closed = title_fill(&h);
        MenuBar::open(h.ui_mut(), bar(), "Edit");
        frame(&mut h);
        assert!(recorded(&h, 1));
        assert_ne!(title_fill(&h), closed, "the open look");

        let mut passes = 0;
        h.frame(|ui| {
            passes += 1;
            if modal_first {
                Modal::new().id_salt("modal").show(ui, |_| {});
            }
            build(ui);
            if !modal_first {
                Modal::new().id_salt("modal").show(ui, |_| {});
            }
        });
        assert_eq!(passes, want_passes, "modal first: {modal_first}");
        assert_eq!(open(&h), None);
        assert!(
            !recorded(&h, 1),
            "the menu is gone from the frame's last pass"
        );
        assert_eq!(title_fill(&h), closed, "the closed look");
    }
}
