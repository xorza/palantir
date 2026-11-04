//! What opens the menu, what dismisses it, and what an item reports.

use crate::host::harness::Harness;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::scene::layer::Layer;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::widgets::menu::Menu;
use crate::widgets::menu::menu_item::MenuItem;
use crate::widgets::menu::menu_separator::MenuSeparator;
use crate::widgets::menu::tests::support::{SURFACE, menu_body, menu_rows, trigger_id};
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::popup::trigger_state::TriggerState;
use glam::UVec2;
use glam::Vec2;

#[test]
fn close_before_open_does_not_create_state() {
    let mut h = Harness::mono(UVec2::ONE);
    Menu::close(h.ui_mut(), trigger_id());
    assert!(
        h.ui()
            .state::<TriggerState<Menu<'static>>>(trigger_id())
            .is_none()
    );
}

#[test]
fn secondary_click_opens_menu_at_pointer() {
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    assert!(!menu_open(h.ui()), "menu starts closed");

    h.right_click(Vec2::new(60.0, 20.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(h.ui()), "secondary click on trigger opens menu");
}

#[test]
fn outside_click_dismisses_menu() {
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    h.right_click(Vec2::new(60.0, 20.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(h.ui()));

    // Click far from both trigger and any plausible menu body location.
    h.click(Vec2::new(380.0, 380.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(!menu_open(h.ui()), "outside click closes the menu");
}

#[test]
fn item_click_dismisses_and_reports_clicked() {
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    // Open the menu at a known anchor.
    Menu::open(h.ui_mut(), trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(h.ui()));

    let copy_row = menu_rows(&h, trigger_id())[0].id;
    h.click(copy_row);
    let copied = h.frame_value(build);
    assert!(copied, "clicking the Copy row reports clicked()");
    assert!(!menu_open(h.ui()), "item click auto-closes the menu");
}

/// Pressing a `MenuItem`'s shortcut while the menu is open fires the item (`Response::clicked` is
/// `true`) and closes the menu, as native menus do. A hint shows the same chord on the row and
/// leaves the press alone: the menu stays open and the row does not click.
#[test]
fn shortcut_press_fires_item_and_dismisses_unless_only_hinted() {
    for hint in [false, true] {
        let mut h = Harness::mono(SURFACE);
        h.frame(|ui| {
            build_copy(ui, hint);
        });
        Menu::open(h.ui_mut(), trigger_id(), Vec2::new(60.0, 60.0));
        let row = h.frame_value(|ui| build_copy(ui, hint));
        assert!(menu_open(h.ui()));
        assert!(
            h.node_of(row.id.with("shortcut")).is_some(),
            "hint {hint}: the chord shows on the row",
        );

        // The primary command modifier + 'C' — `Modifiers::CTRL` is
        // platform-normalized (Cmd on macOS, Ctrl elsewhere).
        h.hold_modifiers(Modifiers::CTRL);
        h.key(Key::Char('C'));
        let row = h.frame_value(|ui| build_copy(ui, hint));
        assert_eq!(row.clicked, !hint, "hint {hint}: the press clicks the row");
        assert_eq!(menu_open(h.ui()), hint, "hint {hint}: the menu stays open");
    }
}

#[test]
fn escape_dismisses_menu() {
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    Menu::open(h.ui_mut(), trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        build(ui);
    });
    assert!(menu_open(h.ui()));

    // Inject an Escape press.
    h.key(Key::Escape);
    h.frame(|ui| {
        build(ui);
    });
    assert!(!menu_open(h.ui()), "Esc closes the menu");
}

/// The menu body hugs its content width (`theme.min_width` floor) instead of spanning the surface;
/// guards `Fill` cross-axis on inner cells leaking `INF` up through the Hug menu container.
#[test]
fn menu_body_width_does_not_span_surface() {
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        build(ui);
    });
    Menu::open(h.ui_mut(), trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        build(ui);
    });

    let body_id = Menu::menu_id(trigger_id());
    let rect = h
        .ui()
        .cascade()
        .locate(body_id)
        .map(|l| h.ui().cascade().entry(l).rect)
        .expect("menu body recorded");
    // The rows are narrower than the theme's floor, so the body sits at
    // it: hugging, not spanning the 400 px surface.
    assert_eq!(rect.size.w, h.ui().theme().menu.min_width);
    assert_eq!(rect.size.w, 208.0);
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
                .show(ui);
            Menu::context(&trigger).show(ui, |ui| {
                let item = MenuItem::new("Copy");
                let item = if hint {
                    item.shortcut_hint(Shortcut::ctrl('C'))
                } else {
                    item.shortcut(Shortcut::ctrl('C'))
                };
                let row = item.show(ui);
                copy = CopyRow {
                    clicked: row.left.clicked(),
                    id: row.id,
                };
                MenuSeparator::new().show(ui);
                MenuItem::new("Paste").show(ui);
            });
        });
    copy
}

fn menu_open(ui: &Ui) -> bool {
    Menu::is_open(ui, trigger_id())
}

/// **A menu raised from inside a popup records above it rather than panicking.**
///
/// The composition every text field in an overlay is: a `TextEdit` in a popup, right-clicked. A menu
/// sharing [`Layer::Popup`] with its popup would push a layer onto itself: a debug assertion in
/// debug, and in release a menu recorded *underneath* its parent, occluded and un-hittable.
///
/// Asserted on the tree the body lands in, not on the absence of a panic: a menu on the popup's own
/// layer would still be open, and only the layer says which side of its parent it is on.
#[test]
fn a_menu_raised_inside_a_popup_lands_above_it() {
    let mut h = Harness::mono(SURFACE);
    h.frame(nested);
    Menu::open(h.ui_mut(), trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(nested);

    assert!(menu_open(h.ui()), "the menu did not open inside the popup");
    let body = Menu::menu_id(trigger_id());
    let held = |layer| h.ui().tree(layer).records.widget_id().contains(&body);
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
            Popup::new()
                .anchor(Anchor::below(Rect::new(10.0, 10.0, 100.0, 20.0)))
                .id(WidgetId::from_hash("host"))
                .show(ui, |ui| {
                    let trigger = Button::new()
                        .id(trigger_id())
                        .label("right click me")
                        .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                        .show(ui);
                    Menu::context(&trigger).show(ui, |ui| {
                        MenuItem::new("Copy").show(ui);
                    });
                });
        });
}

/// A menu opened while focus shows the ring takes focus on its first row, and Escape gives it back
/// to the trigger; a disabled trigger closes its menu without running the body, and the menu stays
/// closed once enabled again.
#[test]
fn a_keyboard_open_focuses_the_first_row_and_a_disabled_trigger_closes() {
    let copy = WidgetId::from_hash("copy");
    let scene = |ui: &mut Ui, disabled: bool| {
        let trigger = Button::new()
            .id(trigger_id())
            .label("menu")
            .disabled(disabled)
            .show(ui);
        Menu::context(&trigger)
            .show(ui, |ui| {
                MenuItem::new("Copy").id(copy).show(ui);
                MenuItem::new("Paste").show(ui);
            })
            .is_some()
    };
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| {
        scene(ui, false);
    });
    h.key(Key::Tab);
    h.frame(|ui| {
        scene(ui, false);
    });
    assert_eq!(
        h.ui().focus(),
        Some(trigger_id()),
        "premise: Tab focuses the trigger"
    );

    Menu::open(h.ui_mut(), trigger_id(), Vec2::new(60.0, 60.0));
    assert!(h.frame_value(|ui| scene(ui, false)));
    assert_eq!(
        h.ui().focus(),
        Some(copy),
        "the keyboard's open focuses the first row"
    );
    h.key(Key::Escape);
    h.frame(|ui| {
        scene(ui, false);
    });
    h.frame(|ui| {
        scene(ui, false);
    });
    assert!(!menu_open(h.ui()));
    assert_eq!(
        h.ui().focus(),
        Some(trigger_id()),
        "closing gives focus back"
    );

    Menu::open(h.ui_mut(), trigger_id(), Vec2::new(60.0, 60.0));
    assert!(
        !h.frame_value(|ui| scene(ui, true)),
        "the body does not run under a disabled trigger"
    );
    assert!(!menu_open(h.ui()), "a disabled trigger closes its menu");
    assert!(
        !h.frame_value(|ui| scene(ui, false)),
        "and it stays closed once enabled again"
    );
}

/// A trigger 120 × 40 at `(20, top)` with a menu below it; whether the menu is open.
fn below_scene(top: f32) -> impl FnMut(&mut Ui) -> bool {
    move |ui| {
        let mut open = false;
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .padding(Spacing::new(20.0, top, 0.0, 0.0))
            .show(ui, |ui| {
                let trigger = Button::new()
                    .id(trigger_id())
                    .label("menu")
                    .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                    .show(ui);
                open = Menu::below(&trigger)
                    .show(ui, |ui| {
                        MenuItem::new("Copy").show(ui);
                    })
                    .is_some();
            });
        open
    }
}

/// A menu below its trigger opens on a left click, on the menu layer, flush with the trigger's
/// left edge and under its foot at y = 40 + 40; under a trigger at the window's foot it flips
/// above, its foot on the trigger's top at y = 340. A second click closes it, and a right-click
/// opens nothing.
#[test]
fn a_menu_below_toggles_on_a_click_and_hangs_from_its_trigger() {
    let body = |h: &Harness| h.ui().layout(Layer::Menu).rect[menu_body(h, trigger_id()).idx()];
    for (top, flips) in [(40.0, false), (340.0, true)] {
        let mut scene = below_scene(top);
        let mut h = Harness::mono(SURFACE);
        h.frame_value(&mut scene);
        let at = Vec2::new(80.0, top + 20.0);
        h.right_click(at);
        assert!(
            !h.frame_value(&mut scene),
            "{top}: a right-click opens nothing"
        );
        h.click(at);
        assert!(h.frame_value(&mut scene), "{top}: a click opens it");
        let rect = body(&h);
        assert_eq!(rect.min.x, 20.0, "{top}");
        if flips {
            assert_eq!(rect.max().y, top, "{top}: flipped above");
        } else {
            assert_eq!(rect.min.y, top + 40.0, "{top}: below the foot");
        }
        h.click(at);
        h.frame_value(&mut scene);
        assert!(!menu_open(h.ui()), "{top}: a second click closes it");
    }
}

/// A menu at a point shows while its caller keeps it open: on the menu layer at the point, its
/// first row focused when the keyboard holds focus, and a clicked row closes it, which the
/// caller reads to stop showing it.
#[test]
fn a_menu_at_a_point_shows_while_its_caller_keeps_it_open() {
    let id = WidgetId::from_hash("canvas-menu");
    let row = WidgetId::from_hash("row");
    let mut open = false;
    let scene = |ui: &mut Ui, open: &mut bool| {
        Button::new().id(trigger_id()).label("focus").show(ui);
        if *open {
            let resp = Menu::at(Vec2::new(60.0, 70.0)).id(id).show(ui, |ui| {
                MenuItem::new("Copy").id(row).show(ui);
            });
            *open = !resp.is_some_and(|resp| resp.closed());
        }
    };
    let mut h = Harness::mono(SURFACE);
    h.frame(|ui| scene(ui, &mut open));
    h.key(Key::Tab);
    h.frame(|ui| scene(ui, &mut open));
    assert_eq!(
        h.ui().focus(),
        Some(trigger_id()),
        "premise: the keyboard holds focus"
    );
    open = true;
    h.frame(|ui| scene(ui, &mut open));
    h.frame(|ui| scene(ui, &mut open));
    assert_eq!(h.node_of(id).map(|at| at.plane.layer), Some(Layer::Menu));
    assert_eq!(h.rect(id).map(|rect| rect.min), Some(Vec2::new(60.0, 70.0)));
    assert_eq!(
        h.ui().focus(),
        Some(row),
        "the keyboard focus lands on the first row"
    );
    let at = h.center_of(row);
    h.click(at);
    h.frame(|ui| scene(ui, &mut open));
    assert!(!open, "a clicked row closes it");
}
