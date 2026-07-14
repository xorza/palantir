//! End-to-end tests for `ContextMenu` + `MenuItem`.

use crate::forest::element::Configure;
use crate::input::InputEvent;
use crate::input::keyboard::{Key, Modifiers};
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::button::Button;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::panel::Panel;
use crate::{Sense, Ui};
use glam::{UVec2, Vec2};

const SURFACE: UVec2 = UVec2::new(400, 400);

fn trigger_id() -> WidgetId {
    WidgetId::from_hash("trigger")
}

fn build(ui: &mut Ui, clicked_copy: &mut bool, _unused: &mut bool) {
    Panel::vstack()
        .id(WidgetId::from_hash("root"))
        .size((Sizing::FILL, Sizing::FILL))
        .sense(Sense::CLICK)
        .show(ui, |ui| {
            let trigger = Button::new()
                .id(WidgetId::from_hash("trigger"))
                .label("right click me")
                .size((Sizing::Fixed(120.0), Sizing::Fixed(40.0)))
                .show(ui)
                .snapshot();
            ContextMenu::attach(ui, &trigger).show(ui, |ui, popup| {
                if MenuItem::new("Copy")
                    .shortcut(Shortcut::ctrl('C'))
                    .show(ui, popup)
                    .left.clicked()
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
    let mut ui = Ui::for_test();
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&ui), "menu starts closed");

    ui.secondary_click_at(Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&ui), "secondary click on trigger opens menu");
}

#[test]
fn outside_click_dismisses_menu() {
    let mut ui = Ui::for_test();
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    ui.secondary_click_at(Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&ui));

    // Click far from both trigger and any plausible menu body location.
    ui.click_at(Vec2::new(380.0, 380.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&ui), "outside click closes the menu");
}

#[test]
fn item_click_dismisses_and_reports_clicked() {
    let mut ui = Ui::for_test();
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    // Open the menu at a known anchor.
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&ui));

    // The menu's container starts at anchor (60, 60). With theme
    // padding (~4) plus row padding, the first item (Copy) sits a
    // few px inside that. Click a couple px past the top-left
    // corner — well inside any plausible row layout.
    ui.click_at(Vec2::new(90.0, 80.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(copied, "clicking the Copy row reports clicked()");
    assert!(!menu_open(&ui), "item click auto-closes the menu");
}

/// Pressing a `MenuItem`'s shortcut while the menu is open fires
/// the item (its `Response::clicked` is `true`) AND closes the menu,
/// mirroring native menu behaviour. Disabled items don't intercept.
#[test]
fn shortcut_press_fires_item_and_dismisses() {
    let mut ui = Ui::for_test();
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&ui));

    // Inject the primary command modifier + 'C' — matches
    // `Shortcut::ctrl('C')` on the Copy item. `Modifiers::ctrl` is
    // platform-normalized (Cmd on macOS, Ctrl elsewhere).
    let primary_mods = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    ui.on_input(InputEvent::ModifiersChanged(primary_mods));
    ui.on_input(InputEvent::KeyDown {
        key: Key::Char('C'),
        repeat: false,
        physical: Key::Other,
    });
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(copied, "shortcut press synthesizes a click on the Copy row");
    assert!(!menu_open(&ui), "shortcut press auto-closes the menu");
}

#[test]
fn escape_dismisses_menu() {
    let mut ui = Ui::for_test();
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&ui));

    // Inject an Escape press.
    ui.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
        physical: Key::Other,
    });
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&ui), "Esc closes the menu");
}

/// Menu body must hug to its content width (theme.min_width floor),
/// not blow up to the surface width. Regresses an issue where `Fill`
/// cross-axis on inner cells leaked `INF` up through the Hug menu
/// container.
#[test]
fn menu_body_width_does_not_span_surface() {
    let mut ui = Ui::for_test();
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at_acked(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    ui.run_at(SURFACE, |ui| build(ui, &mut copied, &mut dismissed));

    let body_id = trigger_id().with("ctx_menu_body");
    let rect = ui
        .cascades
        .entry_idx_of(body_id)
        .map(|i| ui.cascades.entries.rect[i as usize])
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

/// Pure-function pin for `place_anchor`. Off-edge raw anchors flip to
/// the opposite side of themselves when there's room there; surface-
/// origin raw anchors are unchanged; a body taller/wider than the
/// surface clamps inward.
#[test]
fn place_anchor_flips_then_clamps() {
    use crate::primitives::rect::Rect;
    use crate::primitives::size::Size;
    use crate::widgets::popup::place_anchor;
    let surface = Rect::new(0.0, 0.0, 400.0, 400.0);
    let size = Size::new(160.0, 120.0);

    // Bottom-right overflow with room above/left → flip both axes.
    // Anchor at (395, 395): anchor.y+h = 515 > 400 and anchor.y-h =
    // 275 ≥ 0, so y flips to 275; same logic gives x = 235.
    let p = place_anchor(Vec2::new(395.0, 395.0), Some(size), surface);
    assert_eq!(p, Vec2::new(235.0, 275.0));

    // Inside → unchanged on both axes.
    let p = place_anchor(Vec2::new(50.0, 50.0), Some(size), surface);
    assert_eq!(p, Vec2::new(50.0, 50.0));

    // Unknown size (first frame before measure) → pass-through;
    // `Popup::show` pairs this with a one-shot relayout so the
    // next pass places against measured size.
    let p = place_anchor(Vec2::new(395.0, 395.0), None, surface);
    assert_eq!(p, Vec2::new(395.0, 395.0));

    // Body taller than the surface — neither side fits the flip
    // constraint, so the safety clamp shifts inward as far as it
    // can without leaving the top-left off-surface.
    let too_tall = Size::new(50.0, 500.0);
    let p = place_anchor(Vec2::new(50.0, 200.0), Some(too_tall), surface);
    assert_eq!(p, Vec2::new(50.0, 0.0));
}
