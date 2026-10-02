//! The anchored body a popup test records, and the main-panel probe under
//! it.

use crate::layout::types::anchor::Anchor;

use crate::layout::types::sizing::Sizing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::popup::{ClickOutside, Popup};
use crate::{Sense, Ui};
use glam::{UVec2, Vec2};

pub(super) const SURFACE: UVec2 = UVec2::new(400, 400);

pub(super) const ANCHOR: Vec2 = Vec2::new(50.0, 50.0);

pub(super) const BODY_W: f32 = 100.0;

pub(super) const BODY_H: f32 = 60.0;

/// What one record pass of [`record_body`] observed.
#[derive(Clone, Copy, Debug)]
pub(super) struct BodyPass {
    /// The popup reported dismissal.
    pub(super) dismissed: bool,
    /// The `Main` panel under the popup saw a click.
    pub(super) main_clicked: bool,
}

/// A clickable `Main` panel with a popup over it. Read inside the record
/// pass, the only place a one-frame edge is live.
pub(super) fn record_body(ui: &mut Ui, config: ClickOutside) -> BodyPass {
    let main_id = WidgetId::from_hash("main-bg");
    let mut dismissed = false;
    Panel::vstack()
        .id(main_id)
        .size((Sizing::FILL, Sizing::FILL))
        .sense(Sense::CLICK)
        .show(ui, |ui| {
            dismissed = Popup::new(Anchor::at_point(ANCHOR))
                .id(WidgetId::from_hash("test-popup"))
                .click_outside(config)
                .padding(4.0)
                .show(ui, |ui, _popup| {
                    Panel::vstack()
                        .id(WidgetId::from_hash("popup-content"))
                        .size((Sizing::fixed(BODY_W), Sizing::fixed(BODY_H)))
                        .show(ui, |_| {});
                })
                .dismissed;
        });
    BodyPass {
        dismissed,
        main_clicked: ui.response_for(main_id).left.clicked(),
    }
}

/// One frame of [`record_body`], pass A's observation.
pub(super) fn frame_body(h: &mut UiHarness, config: ClickOutside) -> BodyPass {
    *h.frame_passes(|ui| record_body(ui, config)).a()
}
