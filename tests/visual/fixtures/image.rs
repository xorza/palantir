//! Image sampling fixtures. Exact-pixel assertions (no goldens): the
//! expected values are hand-derived from the source texels, so the test
//! is machine-independent and pins the *sampling semantics*, not a
//! rendered snapshot.

use aperture::{Color, Configure, ImageFilter, ImageFit, Panel, Shape, Sizing, Ui};
use glam::{UVec2, Vec2};

use crate::harness::Harness;

/// Source texels for the filter fixture: a 2×1 red|blue strip. Upscaled
/// 64× horizontally, the two filters must diverge only around the seam.
const RED: [u8; 4] = [230, 60, 60, 255];
const BLUE: [u8; 4] = [60, 120, 230, 255];

/// One exactly placed pane painting a strip with independent filters.
fn strip_pane(
    ui: &mut Ui,
    handle: &aperture::ImageHandle,
    x: f32,
    size: Vec2,
    min_filter: ImageFilter,
    mag_filter: ImageFilter,
) {
    Panel::zstack()
        .id_salt(("filter_pane", x as i32))
        .position(glam::Vec2::new(x, 0.0))
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .show(ui, |ui| {
            ui.add_shape(Shape::Image {
                handle: handle.clone(),
                local_rect: None,
                fit: ImageFit::Fill,
                min_filter,
                mag_filter,
                tint: Color::WHITE,
            });
        });
}

/// Minification and magnification choose their own filters. Sampled
/// per-pixel against hand-derived expectations:
/// - Both filters: x=16 / x=112 sit inside the sampler's texel-center
///   clamp region → exactly RED / BLUE (±2 sRGB round-trip).
/// - Nearest: the seam is a hard edge — x=63 is RED, x=64 is BLUE
///   (texel index = floor(uv · 2): 63.5/128·2 = 0.99 vs 64.5/128·2 = 1.01).
/// - Linear: x=64 is mid-ramp — far from both endpoints.
/// - Downscaling RED|BLUE|RED|BLUE from 4px to 2px samples each
///   red/blue boundary: nearest picks BLUE while linear blends.
#[test]
fn minification_and_magnification_filters_are_independent() {
    let mut h = Harness::new();
    let mut mag_strip: Option<aperture::ImageHandle> = None;
    let magnified = h.render(UVec2::new(256, 64), 1.0, Color::BLACK, |ui| {
        let handle = mag_strip
            .get_or_insert_with(|| {
                ui.register_image(aperture::Image::from_rgba8(2, 1, [RED, BLUE].concat()))
                    .expect("fixture image fits every supported GPU")
            })
            .clone();
        Panel::canvas()
            .id_salt("filter_fixture")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                strip_pane(
                    ui,
                    &handle,
                    0.0,
                    Vec2::new(128.0, 64.0),
                    ImageFilter::Nearest,
                    ImageFilter::Linear,
                );
                strip_pane(
                    ui,
                    &handle,
                    128.0,
                    Vec2::new(128.0, 64.0),
                    ImageFilter::Linear,
                    ImageFilter::Nearest,
                );
            });
    });

    let px = |x: u32| magnified.get_pixel(x, 32).0;
    let close = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(l, r)| l.abs_diff(r) <= 2);
    let assert_blend = |pixel: [u8; 4], label: &str| {
        for c in [0, 2] {
            let (lo, hi) = (RED[c].min(BLUE[c]), RED[c].max(BLUE[c]));
            assert!(
                pixel[c] > lo + 20 && pixel[c] < hi - 20,
                "{label} channel {c} = {} must ramp between {lo} and {hi}",
                pixel[c],
            );
        }
    };

    for (base, name) in [(0, "linear magnification"), (128, "nearest magnification")] {
        assert!(close(px(base + 16), RED), "{name} left half must be RED");
        assert!(
            close(px(base + 112), BLUE),
            "{name} right half must be BLUE"
        );
    }

    assert!(close(px(128 + 63), RED), "nearest seam-left must be RED");
    assert!(close(px(128 + 64), BLUE), "nearest seam-right must be BLUE");
    assert_blend(px(64), "linear magnification seam");

    let mut min_strip: Option<aperture::ImageHandle> = None;
    let minified = h.render(UVec2::new(4, 16), 1.0, Color::BLACK, |ui| {
        let handle = min_strip
            .get_or_insert_with(|| {
                ui.register_image(aperture::Image::from_rgba8(
                    4,
                    1,
                    [RED, BLUE, RED, BLUE].concat(),
                ))
                .expect("fixture image fits every supported GPU")
            })
            .clone();
        Panel::canvas()
            .id_salt("min_filter_fixture")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                strip_pane(
                    ui,
                    &handle,
                    0.0,
                    Vec2::new(2.0, 16.0),
                    ImageFilter::Nearest,
                    ImageFilter::Linear,
                );
                strip_pane(
                    ui,
                    &handle,
                    2.0,
                    Vec2::new(2.0, 16.0),
                    ImageFilter::Linear,
                    ImageFilter::Nearest,
                );
            });
    });

    for x in 0..2 {
        assert!(
            close(minified.get_pixel(x, 8).0, BLUE),
            "nearest minification pixel {x} must select BLUE",
        );
    }
    for x in 2..4 {
        assert_blend(
            minified.get_pixel(x, 8).0,
            &format!("linear minification pixel {x}"),
        );
    }
}
