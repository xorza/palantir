//! What opens the menu, what dismisses it, and what an item reports.

use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
use crate::primitives::rect::Rect;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use crate::widgets::configure::Configure;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::ContextMenuState;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::context_menu::tests::support::{SURFACE, menu_rows, trigger_id};
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::{Sense, Ui};
use glam::Vec2;

#[test]
fn close_before_open_does_not_create_state() {
    let mut h = UiHarness::arena();
    ContextMenu::close(h.ui(), trigger_id());
    assert!(h.ui.state::<ContextMenuState>(trigger_id()).is_none());
}

#[test]
fn secondary_click_opens_menu_at_pointer() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    assert!(!menu_open(&h.ui), "menu starts closed");

    h.right_click_at(Vec2::new(60.0, 20.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(&h.ui), "secondary click on trigger opens menu");
}

#[test]
fn outside_click_dismisses_menu() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    h.right_click_at(Vec2::new(60.0, 20.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(&h.ui));

    // Click far from both trigger and any plausible menu body location.
    h.click_at(Vec2::new(380.0, 380.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(!menu_open(&h.ui), "outside click closes the menu");
}

#[test]
fn item_click_dismisses_and_reports_clicked() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    // Open the menu at a known anchor.
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(&h.ui));

    let copy_row = menu_rows(&h, trigger_id())[0].id;
    h.click_on(copy_row);
    let copied = h.frame_value(build);
    assert!(copied, "clicking the Copy row reports clicked()");
    assert!(!menu_open(&h.ui), "item click auto-closes the menu");
}

/// Pressing a `MenuItem`'s shortcut while the menu is open fires
/// the item (its `Response::clicked` is `true`) AND closes the menu,
/// mirroring native menu behaviour. A hint shows the same chord on the
/// row and leaves the press alone: the menu stays open and the row does
/// not click.
#[test]
fn shortcut_press_fires_item_and_dismisses_unless_only_hinted() {
    for hint in [false, true] {
        let mut h = UiHarness::new(SURFACE);
        h.frame(|ui| {
            build_copy(ui, hint);
        });
        ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
        let row = h.frame_value(|ui| build_copy(ui, hint));
        assert!(menu_open(&h.ui));
        assert!(
            h.node_of(row.id.with("shortcut")).is_some(),
            "hint {hint}: the chord shows on the row",
        );

        // The primary command modifier + 'C' — `Modifiers::ctrl` is
        // platform-normalized (Cmd on macOS, Ctrl elsewhere).
        h.set_modifiers(Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        });
        h.key(Key::Char('C'));
        let row = h.frame_value(|ui| build_copy(ui, hint));
        assert_eq!(row.clicked, !hint, "hint {hint}: the press clicks the row");
        assert_eq!(menu_open(&h.ui), hint, "hint {hint}: the menu stays open");
    }
}

#[test]
fn escape_dismisses_menu() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(&h.ui));

    // Inject an Escape press.
    h.key(Key::Escape);
    h.frame(|ui| {
        build(ui);
    });
    assert!(!menu_open(&h.ui), "Esc closes the menu");
}

/// Menu body must hug to its content width (theme.min_width floor),
/// not blow up to the surface width. Regresses an issue where `Fill`
/// cross-axis on inner cells leaked `INF` up through the Hug menu
/// container.
#[test]
fn menu_body_width_does_not_span_surface() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        build(ui);
    });

    let body_id = trigger_id().with("body");
    let rect =
        h.ui.cascade()
            .locate(body_id)
            .map(|l| l.entry_idx)
            .map(|i| h.ui.cascade().entries[i as usize].rect)
            .expect("menu body recorded");
    // The rows are narrower than the theme's floor, so the body sits at
    // it: hugging, not spanning the 400 px surface.
    assert_eq!(rect.size.w, h.ui.theme().context_menu.min_width);
    assert_eq!(rect.size.w, 160.0);
}

/// The scene, returning whether the Copy row clicked this pass.
fn build(ui: &mut Ui) -> bool {
    build_copy(ui, false).clicked
}

/// What the Copy row reported this pass.
#[derive(Debug)]
struct CopyRow {
    clicked: bool,
    id: WidgetId,
}

/// The scene, with Copy's Ctrl+C bound or, under `hint`, only shown.
fn build_copy(ui: &mut Ui, hint: bool) -> CopyRow {
    let mut copy = CopyRow {
        clicked: false,
        id: WidgetId::from_hash("unrecorded"),
    };
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .size((Sizing::FILL, Sizing::FILL))
        .sense(Sense::CLICK)
        .show(ui, |ui| {
            let trigger = Button::new()
                .id(WidgetId::from_hash("trigger"))
                .label("right click me")
                .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                .show(ui)
                .snapshot();
            ContextMenu::attach(ui, &trigger).show(ui, |ui, popup| {
                let item = MenuItem::new("Copy");
                let item = if hint {
                    item.shortcut_hint(Shortcut::ctrl('C'))
                } else {
                    item.shortcut(Shortcut::ctrl('C'))
                };
                let row = item.show(ui, popup);
                copy = CopyRow {
                    clicked: row.left.clicked(),
                    id: row.id,
                };
                MenuItem::separator().show(ui);
                MenuItem::new("Paste").show(ui, popup);
            });
        });
    copy
}

fn menu_open(ui: &Ui) -> bool {
    ContextMenu::is_open(ui, trigger_id())
}

/// **A menu raised from inside a popup records above it rather than panicking.**
///
/// The composition every text field in an overlay is: a `TextEdit` in a popup,
/// right-clicked. A menu that shared [`Layer::Popup`] with the popup that
/// raised it asked the forest to push a layer onto itself — a debug assertion
/// in debug, and in release a menu recorded *underneath* its own parent, drawn
/// occluded and un-hittable.
///
/// Asserted on the tree the body lands in rather than on the absence of a
/// panic: a menu that opened on the popup's own layer would still be open, and
/// only the layer says which side of its parent it is on.
#[test]
fn a_menu_raised_inside_a_popup_lands_above_it() {
    let mut h = UiHarness::new(SURFACE);
    h.frame(nested);
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(nested);

    assert!(menu_open(&h.ui), "the menu did not open inside the popup");
    let body = trigger_id().with("body");
    let held = |layer| h.ui.tree(layer).records.widget_id().contains(&body);
    assert!(held(Layer::Menu), "the menu body is not on the menu layer");
    assert!(
        !held(Layer::Popup),
        "the menu body is on the layer of the popup that raised it",
    );
}

/// A popup holding the trigger a menu is attached to.
fn nested(ui: &mut Ui) {
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Popup::below(Rect::new(10.0, 10.0, 100.0, 20.0))
                .id(WidgetId::from_hash("host"))
                .show(ui, |ui, _| {
                    let trigger = Button::new()
                        .id(trigger_id())
                        .label("right click me")
                        .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                        .show(ui)
                        .snapshot();
                    ContextMenu::attach(ui, &trigger).show(ui, |ui, popup| {
                        MenuItem::new("Copy").show(ui, popup);
                    });
                });
        });
}
