//! The anchored body a popup test records, and the main-panel probe under
//! it.

use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::popup::{ClickOutside, Popup};
use crate::{Sense, Ui};
use glam::{UVec2, Vec2};

pub(super) const SURFACE: UVec2 = UVec2::new(400, 400);

pub(super) const ANCHOR: Vec2 = Vec2::new(50.0, 50.0);

pub(super) const BODY_W: f32 = 100.0;

pub(super) const BODY_H: f32 = 60.0;

// `Ui::frame` re-runs the build closure when action input is pending,
// so we OR `dismissed` across passes — pass 1 sees the click, pass 2
// would otherwise overwrite with a fresh false.
pub(super) fn record_body(ui: &mut Ui, config: ClickOutside, dismissed: &mut bool) {
    Panel::vstack()
        .id(WidgetId::from_hash("main-bg"))
        .size((Sizing::FILL, Sizing::FILL))
        .sense(Sense::CLICK)
        .show(ui, |ui| {
            let r = Popup::anchored_to(ANCHOR)
                .id(WidgetId::from_hash("test-popup"))
                .click_outside(config)
                .padding(4.0)
                .show(ui, |ui, _popup| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-content"))
                        .size((Sizing::fixed(100.0), Sizing::fixed(60.0)))
                        .show(ui, |_| {});
                });
            *dismissed |= r.dismissed;
        });
}

pub(super) fn main_panel_clicked(ui: &Ui) -> bool {
    let main_id = WidgetId::from_hash("main-bg");
    ui.response_for(main_id).left.clicked()
}
