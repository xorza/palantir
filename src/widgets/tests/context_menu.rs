//! End-to-end tests for `ContextMenu` + `MenuItem`.

use crate::Ui;
use crate::forest::element::Configure;
use crate::forest::widget_id::WidgetId;
use crate::input::InputEvent;
use crate::input::keyboard::Key;
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
use crate::support::testing::{click_at, run_at, run_at_acked, secondary_click_at};
use crate::widgets::button::Button;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 400);

fn trigger_id() -> WidgetId {
    WidgetId::from_hash("trigger")
}

fn build(ui: &mut Ui, clicked_copy: &mut bool, _unused: &mut bool) {
    Panel::vstack()
        .id_salt("root")
        .size((Sizing::FILL, Sizing::FILL))
        .sense(crate::Sense::CLICK)
        .show(ui, |ui| {
            let trigger = Button::new()
                .id_salt("trigger")
                .label("right click me")
                .size((Sizing::Fixed(120.0), Sizing::Fixed(40.0)))
                .show(ui);
            ContextMenu::attach(ui, &trigger).show(ui, |ui, popup| {
                if MenuItem::new("Copy")
                    .shortcut(Shortcut::cmd('C'))
                    .show(ui, popup)
                    .clicked()
                {
                    *clicked_copy = true;
                }
                MenuItem::separator(ui);
                MenuItem::new("Paste").show(ui, popup);
            });
        });
}

fn menu_open(ui: &Ui) -> bool {
    ContextMenu::is_open(ui, trigger_id())
}

#[test]
fn secondary_click_opens_menu_at_pointer() {
    let mut ui = Ui::new();
    let mut copied = false;
    let mut dismissed = false;
    run_at_acked(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(!menu_open(&ui), "menu starts closed");

    secondary_click_at(&mut ui, Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(menu_open(&ui), "secondary click on trigger opens menu");
}

#[test]
fn outside_click_dismisses_menu() {
    let mut ui = Ui::new();
    let mut copied = false;
    let mut dismissed = false;
    run_at_acked(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    secondary_click_at(&mut ui, Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(menu_open(&ui));

    // Click far from both trigger and any plausible menu body location.
    click_at(&mut ui, Vec2::new(380.0, 380.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(!menu_open(&ui), "outside click closes the menu");
}

#[test]
fn item_click_dismisses_and_reports_clicked() {
    let mut ui = Ui::new();
    let mut copied = false;
    let mut dismissed = false;
    run_at_acked(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    // Open the menu at a known anchor.
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(menu_open(&ui));

    // The menu's container starts at anchor (60, 60). With theme
    // padding (~4) plus row padding, the first item (Copy) sits a
    // few px inside that. Click a couple px past the top-left
    // corner — well inside any plausible row layout.
    click_at(&mut ui, Vec2::new(90.0, 80.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(copied, "clicking the Copy row reports clicked()");
    assert!(!menu_open(&ui), "item click auto-closes the menu");
}

/// Pressing a `MenuItem`'s shortcut while the menu is open fires
/// the item (its `Response::clicked` is `true`) AND closes the menu,
/// mirroring native menu behaviour. Disabled items don't intercept.
#[test]
fn shortcut_press_fires_item_and_dismisses() {
    let mut ui = Ui::new();
    let mut copied = false;
    let mut dismissed = false;
    run_at_acked(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(menu_open(&ui));

    // Inject the platform-primary modifier + 'C' — matches
    // `Shortcut::cmd('C')` on the Copy item.
    let primary_mods = if cfg!(target_os = "macos") {
        crate::input::keyboard::Modifiers {
            meta: true,
            ..crate::input::keyboard::Modifiers::NONE
        }
    } else {
        crate::input::keyboard::Modifiers {
            ctrl: true,
            ..crate::input::keyboard::Modifiers::NONE
        }
    };
    ui.on_input(InputEvent::ModifiersChanged(primary_mods));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('C'),
        repeat: false,
    });
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(copied, "shortcut press synthesizes a click on the Copy row");
    assert!(!menu_open(&ui), "shortcut press auto-closes the menu");
}

#[test]
fn escape_dismisses_menu() {
    let mut ui = Ui::new();
    let mut copied = false;
    let mut dismissed = false;
    run_at_acked(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(menu_open(&ui));

    // Inject an Escape press.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
    });
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    assert!(!menu_open(&ui), "Esc closes the menu");
}

/// Menu body must hug to its content width (theme.min_width floor),
/// not blow up to the surface width. Regresses an issue where `Fill`
/// cross-axis on inner cells leaked `INF` up through the Hug menu
/// container.
#[test]
fn menu_body_width_does_not_span_surface() {
    let mut ui = Ui::new();
    let mut copied = false;
    let mut dismissed = false;
    run_at_acked(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    run_at(&mut ui, SURFACE, |ui| {
        build(ui, &mut copied, &mut dismissed)
    });

    let body_id = trigger_id().with("ctx_menu_body");
    let rect = ui
        .layout
        .cascades
        .by_id
        .get(&body_id)
        .map(|&i| ui.layout.cascades.entries[i as usize].rect)
        .expect("menu body recorded");
    // Theme min_width is 160; sample labels are short so we expect
    // ≤ 200 px wide. SURFACE.w = 400, so a "spans surface" regression
    // would land ≥ 380.
    assert!(
        rect.size.w < 240.0,
        "menu body w={} — expected hug to content, not surface width ({})",
        rect.size.w,
        SURFACE.x,
    );
}

/// Pure-function pin for `clamp_anchor`. Off-edge raw anchors get
/// pulled back inside surface so `anchor + size <= surface_max`;
/// surface-origin raw anchors are unchanged.
#[test]
fn clamp_anchor_pins_to_surface() {
    use crate::primitives::rect::Rect;
    use crate::primitives::size::Size;
    use crate::widgets::context_menu::clamp_anchor;
    let surface = Rect::new(0.0, 0.0, 400.0, 400.0);
    let size = Size::new(160.0, 120.0);

    // Bottom-right overflow → clamp to (240, 280).
    let p = clamp_anchor(Vec2::new(395.0, 395.0), Some(size), surface);
    assert_eq!(p, Vec2::new(240.0, 280.0));

    // Inside → unchanged.
    let p = clamp_anchor(Vec2::new(50.0, 50.0), Some(size), surface);
    assert_eq!(p, Vec2::new(50.0, 50.0));

    // Unknown size → pass-through.
    let p = clamp_anchor(Vec2::new(395.0, 395.0), None, surface);
    assert_eq!(p, Vec2::new(395.0, 395.0));
}
