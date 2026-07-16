//! Pixel-level shadow fixtures.

use aperture::{Color, Configure, Corners, Panel, Rect, Shadow, Shape, Sizing};
use glam::{IVec2, UVec2, Vec2};
use image::RgbaImage;

use crate::diff::{Tolerance, diff};
use crate::harness::Harness;

const VIEWPORT: UVec2 = UVec2::new(220, 180);
const CLEAR: Color = Color::WHITE;

fn render_shadow(
    source: Rect,
    corners: f32,
    offset: Vec2,
    blur: f32,
    spread: f32,
    inset: bool,
) -> RgbaImage {
    let mut harness = Harness::new();
    harness.render(VIEWPORT, 1.0, CLEAR, |ui| {
        Panel::canvas()
            .auto_id()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::Shadow {
                    local_rect: Some(source),
                    corners: Corners::all(corners),
                    shadow: Shadow {
                        color: Color::rgba(0.0, 0.0, 0.0, 0.85),
                        offset,
                        blur,
                        spread,
                        inset,
                    },
                });
            });
    })
}

fn assert_same_pixels_in_rect(label: &str, actual: &RgbaImage, expected: &RgbaImage, rect: Rect) {
    let mut differing_pixels = 0;
    let mut max_channel_delta = 0;
    for y in rect.min.y as u32..rect.max().y as u32 {
        for x in rect.min.x as u32..rect.max().x as u32 {
            let actual = actual.get_pixel(x, y);
            let expected = expected.get_pixel(x, y);
            let delta = actual
                .0
                .iter()
                .zip(expected.0)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap();
            max_channel_delta = max_channel_delta.max(delta);
            differing_pixels += u32::from(delta != 0);
        }
    }
    assert_eq!(
        differing_pixels, 0,
        "{label}: channel delta reached {max_channel_delta}",
    );
}

#[test]
fn shifted_drop_bbox_preserves_positive_and_negative_offset_pixels() {
    let source = Rect::new(64.0, 60.0, 72.0, 54.0);
    let tolerance = Tolerance {
        per_channel: 0,
        max_ratio: 0.0,
    };

    for offset in [Vec2::new(17.0, 13.0), Vec2::new(-19.0, -11.0)] {
        let shifted = render_shadow(source, 11.0, offset, 6.0, 4.0, false);
        let reference = render_shadow(
            Rect {
                min: source.min + offset,
                size: source.size,
            },
            11.0,
            Vec2::ZERO,
            6.0,
            4.0,
            false,
        );
        let report = diff(&shifted, &reference, tolerance);
        assert_eq!(
            report.differing_pixels, 0,
            "offset {offset:?}: max channel delta {}, differing ratio {}",
            report.max_channel_delta, report.differing_ratio,
        );
    }
}

#[test]
fn inset_offset_matches_translated_zero_offset_pixels_inside_source() {
    let source = Rect::new(40.0, 35.0, 120.0, 100.0);
    let pixel_offset = IVec2::new(9, -7);
    let shifted = render_shadow(source, 11.0, pixel_offset.as_vec2(), 6.0, 8.0, true);
    let reference = render_shadow(source, 11.0, Vec2::ZERO, 6.0, 8.0, true);

    let mut differing_pixels = 0;
    let mut max_channel_delta = 0;
    for y in 50..120 {
        for x in 60..145 {
            let actual = shifted.get_pixel(x, y);
            let expected = reference.get_pixel(
                (x as i32 - pixel_offset.x) as u32,
                (y as i32 - pixel_offset.y) as u32,
            );
            let delta = actual
                .0
                .iter()
                .zip(expected.0)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap();
            max_channel_delta = max_channel_delta.max(delta);
            differing_pixels += u32::from(delta != 0);
        }
    }

    assert_eq!(
        differing_pixels, 0,
        "inset translated-pixel comparison reached channel delta {max_channel_delta}",
    );
}

#[test]
fn negative_spread_deflates_drop_and_inset_shadow_geometry() {
    let blur = 6.0;
    let spread = -4.0;

    let drop_source = Rect::new(64.0, 60.0, 72.0, 54.0);
    let drop = render_shadow(drop_source, 11.0, Vec2::ZERO, blur, spread, false);
    let deflated_source = drop_source.inflated(spread);
    let drop_reference = render_shadow(deflated_source, 11.0, Vec2::ZERO, blur, 0.0, false);
    assert_same_pixels_in_rect(
        "drop negative spread",
        &drop,
        &drop_reference,
        deflated_source.inflated(3.0 * blur),
    );

    let inset_source = Rect::new(40.0, 35.0, 120.0, 100.0);
    let inset = render_shadow(inset_source, 11.0, Vec2::ZERO, blur, spread, true);
    let inset_reference = render_shadow(
        inset_source.inflated(-spread),
        11.0 - spread,
        Vec2::ZERO,
        blur,
        0.0,
        true,
    );
    assert_same_pixels_in_rect(
        "inset negative spread",
        &inset,
        &inset_reference,
        inset_source.inflated(-12.0),
    );
}
