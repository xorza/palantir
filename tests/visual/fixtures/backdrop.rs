//! Backdrop panes over the pixels painted before them.

use glam::UVec2;
use palantir::widget::{PaintAnimation, Shape};
use palantir::{
    BackdropFilter, Configure, Corners, Lens, Panel, PopupTrigger, Rect, RgbaF32, Sizing, Stroke,
    Text, Ui, WidgetId,
};

use crate::golden_name::GoldenName;
use crate::support;

/// Stripes of three colors and widths, and two lines of text over them, filling `size`.
fn backdrop_scene(ui: &mut Ui, size: UVec2) {
    for i in 0..size.x / 6 {
        let color = match i % 3 {
            0 => RgbaF32::WHITE,
            1 => RgbaF32::srgb(1.0, 0.3, 0.1),
            _ => RgbaF32::srgb(0.1, 0.4, 1.0),
        };
        let width = 1.0 + (i % 3) as f32;
        ui.add_shape(Shape::rect(Rect::new(i as f32 * 6.0, 0.0, width, size.y as f32)).fill(color));
    }
    Text::new("Backdrop panes blur what lies under them")
        .id(WidgetId::from_hash("backdrop-text"))
        .show(ui);
}

/// Pin: panes over stripes and text, left to right: a plain blur, a frosted pane (saturated,
/// grained and tinted), a rounded pane with a border, a windowed pane, which blurs around its
/// hole, a pane over a pane, and a pane cut by the surface's edge, whose blur mirrors there.
#[test]
fn backdrop_panes_match_golden() {
    let size = UVec2::new(640, 120);
    GoldenName::BackdropPanes.assert_scene(size, |ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("backdrop-canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                backdrop_scene(ui, size);
                ui.add_shape(
                    Shape::rect(Rect::new(10.0, 20.0, 90.0, 80.0))
                        .backdrop_filter(BackdropFilter::blur(6.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(110.0, 20.0, 90.0, 80.0))
                        .fill(RgbaF32::new(1.0, 1.0, 1.0, 0.2))
                        .backdrop_filter(BackdropFilter::frosted(12.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(210.0, 20.0, 90.0, 80.0))
                        .corners(Corners::all(16.0))
                        .border(Stroke::new(RgbaF32::WHITE, 2.0))
                        .backdrop_filter(BackdropFilter::blur(8.0)),
                );
                ui.add_shape(
                    Shape::windowed_rect(Rect::new(310.0, 20.0, 90.0, 80.0))
                        .corners(Corners::all(24.0))
                        .backdrop_filter(BackdropFilter::blur(8.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(410.0, 20.0, 90.0, 80.0))
                        .backdrop_filter(BackdropFilter::blur(3.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(440.0, 40.0, 90.0, 40.0))
                        .fill(RgbaF32::new(0.0, 0.0, 0.0, 0.3))
                        .backdrop_filter(BackdropFilter::blur(10.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(560.0, 0.0, 80.0, 60.0))
                        .backdrop_filter(BackdropFilter::blur(16.0)),
                );
            });
    });
}

/// Pin: a rounded, bordered pane turned 30° about its center, which still blurs by screen
/// position, so the stripes under it stay upright.
#[test]
fn a_turned_pane_matches_golden() {
    let size = UVec2::new(200, 160);
    GoldenName::BackdropTurned.assert_scene(size, |ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("backdrop-turned"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                backdrop_scene(ui, size);
                ui.add_shape_animated(
                    Shape::rect(Rect::new(40.0, 40.0, 120.0, 80.0))
                        .corners(Corners::all(12.0))
                        .fill(RgbaF32::new(1.0, 1.0, 1.0, 0.15))
                        .border(Stroke::new(RgbaF32::WHITE, 2.0))
                        .backdrop_filter(BackdropFilter::blur(8.0)),
                    PaintAnimation::turn(1.0 / 12.0, 1.0 / 12.0),
                );
            });
    });
}

/// Pin: glass over stripes and text: a pill, its bezel half its height, bending the stripes in at
/// its rim and splitting red from blue there, and a flat panel with a narrow bezel and a light
/// blur, glinting where its rim faces the top-left lamp.
#[test]
fn glass_matches_golden() {
    let size = UVec2::new(320, 120);
    GoldenName::BackdropGlass.assert_scene(size, |ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("backdrop-glass"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                backdrop_scene(ui, size);
                ui.add_shape(
                    Shape::rect(Rect::new(16.0, 30.0, 130.0, 60.0))
                        .corners(Corners::all(30.0))
                        .backdrop_filter(BackdropFilter::glass(Lens {
                            bezel: 30.0,
                            thickness: 18.0,
                            refractive_index: 1.5,
                            dispersion: 0.06,
                            highlight: 0.6,
                        })),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(170.0, 16.0, 130.0, 90.0))
                        .corners(Corners::all(16.0))
                        .backdrop_filter(
                            BackdropFilter::glass(Lens {
                                bezel: 12.0,
                                thickness: 10.0,
                                refractive_index: 1.4,
                                dispersion: 0.0,
                                highlight: 0.8,
                            })
                            .with_blur(2.0),
                        ),
                );
            });
    });
}

/// The showcase's materials page, compiled from the example.
#[path = "../../../examples/showcase/pages/materials.rs"]
mod showcase_page;

/// The showcase's materials page as one golden, at the turn's start: the three looks, and the
/// frosted popup open over the page.
#[test]
fn showcase_materials_page_matches_golden() {
    GoldenName::ShowcaseMaterialsPage.assert_settled_scene(UVec2::new(760, 760), 2, |ui| {
        PopupTrigger::open(ui, showcase_page::popup_button());
        Panel::vstack()
            .auto_id()
            .gap(support::PAGE_GAP)
            .show(ui, showcase_page::build);
    });
}
