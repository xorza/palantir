//! Backdrop filters: the three looks over a page that keeps moving under them, and the same
//! material on a popup's and a context menu's chrome.

use crate::support;
use crate::support::{DemoCell, Section, api};
use palantir::glam::Vec2;
use palantir::widget::{PaintAnimation, PaintRepeat, Shape, curves};
use palantir::{
    Align, BackdropFilter, Button, Chrome, Configure, ContextMenu, ContextMenuTheme, Corners, Lens,
    MenuItem, Panel, PopupTrigger, Rect, RgbaF32, Sizing, Stroke, Text, Ui, WidgetId,
};
use std::time::Duration;

/// The size of a look's cell.
const CELL: f32 = 220.0;
/// The id of the button that opens the frosted popup.
pub(crate) fn popup_button() -> WidgetId {
    WidgetId::from_hash("materials-popup-button")
}

/// The pane inside a look's cell.
const PANE: Rect = Rect::new(30.0, 50.0, 160.0, 100.0);

pub(crate) fn build(ui: &mut Ui) {
    Section::new("Looks")
        .apis(&[
            api!(BackdropFilter::blur),
            api!(BackdropFilter::frosted),
            api!(BackdropFilter::glass),
        ])
        .show(ui, |ui| {
            support::note(
                ui,
                "Each pane shows the pixels painted before it, under its fill, which tints them. \
                 The page under it turns, so each pane blurs again every frame; a pane over a \
                 still page costs nothing.",
            );
            support::tiles(ui, |ui| {
                look(
                    ui,
                    "blur — σ 8",
                    BackdropFilter::blur(8.0),
                    RgbaF32::TRANSPARENT,
                );
                look(
                    ui,
                    "frosted — σ 16, tinted",
                    BackdropFilter::frosted(16.0),
                    RgbaF32::new(1.0, 1.0, 1.0, 0.18),
                );
                look(
                    ui,
                    "glass — a lens, σ 1",
                    BackdropFilter::glass(Lens::default()).with_blur(1.0),
                    RgbaF32::TRANSPARENT,
                );
            });
        });

    Section::new("On chrome")
        .apis(&[api!(Chrome::with_backdrop_filter), api!(PopupTrigger::on)])
        .show(ui, |ui| {
            support::note(
                ui,
                "A popup and a context menu whose chrome carries a frosted filter, over the \
                 turning page. Click the button for the popup; right-click the page for the \
                 menu.",
            );
            chrome_demo(ui);
        });
}

/// A cell of `label` holding the turning page and one pane of `filter` and `tint` over it.
fn look(ui: &mut Ui, label: &'static str, filter: BackdropFilter, tint: RgbaF32) {
    DemoCell::new(label).size(CELL, CELL).show(ui, |ui| {
        Panel::canvas()
            .id_salt(label)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                page(ui, Vec2::splat(CELL));
                let corners = if filter.lens.is_some() { 50.0 } else { 14.0 };
                ui.add_shape(
                    Shape::rect(PANE)
                        .corners(Corners::all(corners))
                        .fill(tint)
                        .border(Stroke::new(RgbaF32::new(1.0, 1.0, 1.0, 0.25), 1.0))
                        .backdrop_filter(filter),
                );
            });
    });
}

/// Colored bars and a turning cross filling `size`, so a pane over it has detail to blur and a
/// reason to blur again each frame.
fn page(ui: &mut Ui, size: Vec2) {
    let colors = [support::A, support::B, support::C, support::D, support::E];
    for (i, color) in (0..12).zip(colors.iter().cycle()) {
        let x = i as f32 * size.x / 12.0;
        ui.add_shape(Shape::rect(Rect::new(x, 0.0, size.x / 24.0, size.y)).fill(*color));
    }
    let spin = PaintAnimation::turn(0.0, 1.0)
        .with_period(Duration::from_secs(8))
        .with_repeat(PaintRepeat::Forever)
        .with_curve(curves::linear);
    let arm = size.min_element() * 0.8;
    let bar = size.min_element() * 0.12;
    for (w, h) in [(arm, bar), (bar, arm)] {
        let rect = Rect::new((size.x - w) / 2.0, (size.y - h) / 2.0, w, h);
        ui.add_shape_animated(Shape::rect(rect).fill(RgbaF32::WHITE), spin);
    }
}

/// The frosted chrome the popup and the menu share: a dark tint, rounded, with a hairline.
fn frosted_chrome() -> Chrome {
    Chrome::rounded(RgbaF32::new(0.08, 0.09, 0.11, 0.45), Corners::all(10.0))
        .with_border(Stroke::new(RgbaF32::new(1.0, 1.0, 1.0, 0.12), 1.0))
        .with_backdrop_filter(BackdropFilter::frosted(18.0))
}

/// A wide turning page with a button that opens a frosted popup, and a frosted context menu on
/// the page.
fn chrome_demo(ui: &mut Ui) {
    let size = Vec2::new(360.0, 220.0);
    let page = Panel::zstack()
        .id(WidgetId::from_hash("materials-chrome-page"))
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .child_align(Align::CENTER)
        .show(ui, |ui| {
            Panel::canvas()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| page(ui, size));
            let trigger = Button::new()
                .id(popup_button())
                .label("open popup")
                .show(ui)
                .snapshot();
            PopupTrigger::on(&trigger)
                .padding((14.0, 10.0))
                .chrome(frosted_chrome())
                .show(ui, |ui, _| {
                    Text::new("a frosted popup")
                        .style(&support::body_style())
                        .show(ui);
                });
        })
        .response
        .snapshot();
    let mut menu: ContextMenuTheme = ui.theme().context_menu.clone();
    menu.panel = frosted_chrome();
    ContextMenu::on(&page).style(&menu).show(ui, |ui, popup| {
        for label in ["Copy", "Paste", "Delete"] {
            MenuItem::new(label).id_salt(label).show(ui, popup);
        }
    });
}
