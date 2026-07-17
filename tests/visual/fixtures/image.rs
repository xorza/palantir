//! Image sampling fixtures. Exact-pixel assertions (no goldens): the
//! expected values are hand-derived from the source texels, so the test
//! is machine-independent and pins the *sampling semantics*, not a
//! rendered snapshot.

use aperture::{Color, Configure, ImageFilter, ImageFit, Panel, Shape, Sizing, Ui};
use glam::UVec2;

use crate::harness::Harness;

/// Source texels for the filter fixture: a 2×1 red|blue strip. Upscaled
/// 64× horizontally, the two filters must diverge only around the seam.
const RED: [u8; 4] = [230, 60, 60, 255];
const BLUE: [u8; 4] = [60, 120, 230, 255];

/// One 128×64 pane painting the 2×1 strip with `filter`, at `(x, 0)` in
/// a canvas parent (exact physical placement, no layout rounding).
fn strip_pane(ui: &mut Ui, handle: &aperture::ImageHandle, x: f32, filter: ImageFilter) {
    Panel::zstack()
        .id_salt(("filter_pane", x as i32))
        .position(glam::Vec2::new(x, 0.0))
        .size((Sizing::fixed(128.0), Sizing::fixed(64.0)))
        .show(ui, |ui| {
            ui.add_shape(Shape::Image {
                handle: handle.clone(),
                local_rect: None,
                fit: ImageFit::Fill,
                filter,
                tint: Color::WHITE,
            });
        });
}

/// Bilinear leaves a ramp across the seam; nearest snaps every fragment
/// to exactly one source texel. Sampled per-pixel against hand-derived
/// expectations:
/// - Both filters: x=16 / x=112 sit inside the sampler's texel-center
///   clamp region → exactly RED / BLUE (±2 sRGB round-trip).
/// - Nearest: the seam is a hard edge — x=63 is RED, x=64 is BLUE
///   (texel index = floor(uv · 2): 63.5/128·2 = 0.99 vs 64.5/128·2 = 1.01).
/// - Linear: x=64 is mid-ramp — far from both endpoints.
#[test]
fn nearest_snaps_texels_linear_ramps() {
    let mut h = Harness::new();
    // Held across the render: the shape record only carries the id, so
    // a handle dropped inside the frame frees its texture before draw.
    let mut strip: Option<aperture::ImageHandle> = None;
    let img = h.render(UVec2::new(256, 64), 1.0, Color::BLACK, |ui| {
        let handle = strip
            .get_or_insert_with(|| {
                ui.register_image(aperture::Image::from_rgba8(2, 1, [RED, BLUE].concat()))
            })
            .clone();
        Panel::canvas()
            .id_salt("filter_fixture")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                strip_pane(ui, &handle, 0.0, ImageFilter::Linear);
                strip_pane(ui, &handle, 128.0, ImageFilter::Nearest);
            });
    });

    let px = |x: u32| img.get_pixel(x, 32).0;
    let close = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(l, r)| l.abs_diff(r) <= 2);

    // Texel-center clamp regions: both filters reproduce the source.
    for (base, name) in [(0, "linear"), (128, "nearest")] {
        assert!(close(px(base + 16), RED), "{name} left half must be RED");
        assert!(
            close(px(base + 112), BLUE),
            "{name} right half must be BLUE"
        );
    }

    // Nearest: hard edge exactly between pixels 63 and 64.
    assert!(close(px(128 + 63), RED), "nearest seam-left must be RED");
    assert!(close(px(128 + 64), BLUE), "nearest seam-right must be BLUE");

    // Linear: the seam pixel is a blend — every channel that differs by
    // >100 in the source sits well away from both endpoints. (sRGB
    // encode is monotonic, so between-ness survives the round-trip.)
    let mid = px(64);
    for c in [0, 2] {
        let (lo, hi) = (RED[c].min(BLUE[c]), RED[c].max(BLUE[c]));
        assert!(
            mid[c] > lo + 20 && mid[c] < hi - 20,
            "linear seam channel {c} = {} must ramp between {lo} and {hi}",
            mid[c],
        );
    }
}
