//! An overlay opened inside another overlay's body records above it and works there: the
//! dropdown in a dialog, in a popup and in a context menu, and a dialog in a dialog.

use crate::internals::ui_harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::modal::Modal;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use glam::{UVec2, Vec2};

const OPTIONS: [&str; 3] = ["One", "Two", "Three"];

fn combo() -> WidgetId {
    WidgetId::from_hash("nested-combo")
}

fn menu_trigger() -> WidgetId {
    WidgetId::from_hash("menu-trigger")
}

#[derive(Clone, Copy, Debug)]
enum Opener {
    Modal,
    Popup,
    ContextMenu,
    ModalInModal,
}

fn dropdown(ui: &mut Ui, selected: &mut usize) {
    ComboBox::new(selected, &OPTIONS).id(combo()).show(ui);
}

fn record(ui: &mut Ui, opener: Opener, selected: &mut usize) {
    Panel::vstack()
        .id(menu_trigger())
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| match opener {
            Opener::Modal => {
                Modal::new()
                    .auto_id()
                    .show(ui, |ui, _| dropdown(ui, selected));
            }
            Opener::Popup => {
                Popup::new(Anchor::at_point(Vec2::new(20.0, 20.0)))
                    .auto_id()
                    .show(ui, |ui, _| dropdown(ui, selected));
            }
            Opener::ContextMenu => {
                ContextMenu::for_id(menu_trigger()).show(ui, |ui, _| dropdown(ui, selected));
            }
            Opener::ModalInModal => {
                Modal::new().auto_id().show(ui, |ui, _| {
                    Modal::new()
                        .auto_id()
                        .show(ui, |ui, _| dropdown(ui, selected));
                });
            }
        });
}

/// Each opener holds a dropdown: a click opens its list on a plane above the opener's, the list
/// takes the pointer where it covers the opener, and a click on its last row picks it and closes
/// it, with the opener still open.
#[test]
fn a_dropdown_opens_and_picks_inside_every_overlay() {
    for opener in [
        Opener::Modal,
        Opener::Popup,
        Opener::ContextMenu,
        Opener::ModalInModal,
    ] {
        let mut selected = 0;
        let mut h = UiHarness::new(UVec2::new(400, 300));
        h.frame(|ui| {
            if matches!(opener, Opener::ContextMenu) {
                ContextMenu::open(ui, menu_trigger(), Vec2::new(20.0, 20.0));
            }
            record(ui, opener, &mut selected);
        });
        h.frame(|ui| record(ui, opener, &mut selected));
        h.click_on(combo());
        h.frame(|ui| record(ui, opener, &mut selected));

        let list = combo().with("list");
        let rows = h
            .rect(list)
            .unwrap_or_else(|| panic!("{opener:?}: the list opened"));
        let cascade = h.ui.cascade();
        let (trigger_at, list_at) = (
            cascade.endpoint(combo()).expect("the trigger recorded"),
            cascade.endpoint(list).expect("the list recorded"),
        );
        assert!(
            list_at.plane > trigger_at.plane,
            "{opener:?}: the list {:?} paints above its opener {:?}",
            list_at.plane,
            trigger_at.plane,
        );
        // Four fifths down the list is inside its last of three rows, past the bottom padding.
        let last_row = Vec2::new(
            rows.min.x + rows.size.w * 0.5,
            rows.min.y + rows.size.h * 0.8,
        );
        h.click_at(last_row);
        h.frame(|ui| record(ui, opener, &mut selected));
        assert_eq!(selected, 2, "{opener:?}: the last row was picked");
        assert!(
            h.rect(list).is_none(),
            "{opener:?}: the pick closed the list"
        );
        assert!(
            h.rect(combo()).is_some(),
            "{opener:?}: the opener stays open"
        );
    }
}
