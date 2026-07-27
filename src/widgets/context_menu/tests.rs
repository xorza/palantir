//! End-to-end tests for `ContextMenu` + `MenuItem`.

use crate::input::InputEvent;
use crate::input::keyboard::{Key, Modifiers};
use crate::input::shortcut::Shortcut;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::scene::tree::node::NodeId;
use crate::ui::harness::UiHarness;
use crate::widgets::button::Button;
use crate::widgets::context_menu::{ContextMenu, ContextMenuState, MenuItem};
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
                .size((Sizing::fixed(120.0), Sizing::fixed(40.0)))
                .show(ui)
                .snapshot();
            ContextMenu::attach(ui, &trigger).show(ui, |ui, popup| {
                if MenuItem::new("Copy")
                    .shortcut(Shortcut::ctrl('C'))
                    .show(ui, popup)
                    .left
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
fn close_before_open_does_not_create_state() {
    let mut h = UiHarness::arena();
    ContextMenu::close(h.ui(), trigger_id());
    assert!(h.ui.try_state::<ContextMenuState>(trigger_id()).is_none());
}

#[test]
fn secondary_click_opens_menu_at_pointer() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&h.ui), "menu starts closed");

    h.right_click_at(Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui), "secondary click on trigger opens menu");
}

#[test]
fn outside_click_dismisses_menu() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    h.right_click_at(Vec2::new(60.0, 20.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // Click far from both trigger and any plausible menu body location.
    h.click_at(Vec2::new(380.0, 380.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&h.ui), "outside click closes the menu");
}

#[test]
fn item_click_dismisses_and_reports_clicked() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    // Open the menu at a known anchor.
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // The menu's container starts at anchor (60, 60). With theme
    // padding (~4) plus row padding, the first item (Copy) sits a
    // few px inside that. Click a couple px past the top-left
    // corner — well inside any plausible row layout.
    h.click_at(Vec2::new(90.0, 80.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(copied, "clicking the Copy row reports clicked()");
    assert!(!menu_open(&h.ui), "item click auto-closes the menu");
}

/// Pressing a `MenuItem`'s shortcut while the menu is open fires
/// the item (its `Response::clicked` is `true`) AND closes the menu,
/// mirroring native menu behaviour. Disabled items don't intercept.
#[test]
fn shortcut_press_fires_item_and_dismisses() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // Inject the primary command modifier + 'C' — matches
    // `Shortcut::ctrl('C')` on the Copy item. `Modifiers::ctrl` is
    // platform-normalized (Cmd on macOS, Ctrl elsewhere).
    let primary_mods = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    h.on_input(InputEvent::ModifiersChanged(primary_mods));
    h.on_input(InputEvent::KeyDown {
        key: Key::Char('C'),
        repeat: false,
        physical: Key::Other,
    });
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(copied, "shortcut press synthesizes a click on the Copy row");
    assert!(!menu_open(&h.ui), "shortcut press auto-closes the menu");
}

#[test]
fn escape_dismisses_menu() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(menu_open(&h.ui));

    // Inject an Escape press.
    h.on_input(InputEvent::KeyDown {
        key: Key::Escape,
        repeat: false,
        physical: Key::Other,
    });
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    assert!(!menu_open(&h.ui), "Esc closes the menu");
}

/// Menu body must hug to its content width (theme.min_width floor),
/// not blow up to the surface width. Regresses an issue where `Fill`
/// cross-axis on inner cells leaked `INF` up through the Hug menu
/// container.
#[test]
fn menu_body_width_does_not_span_surface() {
    let mut h = UiHarness::new(SURFACE);
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    let mut copied = false;
    let mut dismissed = false;
    h.frame(|ui| build(ui, &mut copied, &mut dismissed));

    let body_id = trigger_id().with("ctx_menu_body");
    let rect =
        h.ui.cascades
            .entry_idx_of(body_id)
            .map(|i| h.ui.cascades.entries.rect()[i as usize])
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

#[test]
fn explicit_zero_padding_and_minimum_override_menu_theme() {
    let mut h = UiHarness::new(SURFACE);
    ContextMenu::open(&mut h.ui, trigger_id(), Vec2::new(60.0, 60.0));
    h.frame(|ui| {
        ContextMenu::for_id(trigger_id())
            .background(Background::NONE)
            .padding(Spacing::ZERO)
            .min_size(Size::ZERO)
            .show(ui, |_, _| {});
    });

    let body_id = trigger_id().with("ctx_menu_body");
    let tree = &h.ui.forest.trees[Layer::Popup];
    let index = tree
        .records
        .widget_id()
        .iter()
        .position(|id| *id == body_id)
        .expect("context menu body node");
    let node = NodeId(index as u32);
    assert_eq!(tree.records.layout()[index].padding, Spacing::ZERO);
    assert_eq!(tree.bounds(node).min_size, Size::ZERO);
}
