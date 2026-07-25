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
    fit: ImageFit,
    min_filter: ImageFilter,
    mag_filter: ImageFilter,
) {
    Panel::zstack()
        .id_salt(("filter_pane", x as i32))
        .position(glam::Vec2::new(x, 0.0))
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .show(ui, |ui| {
            ui.add_shape(
                Shape::image(handle.clone())
                    .fit(fit)
                    .min_filter(min_filter)
                    .mag_filter(mag_filter),
            );
        });
}

/// Pixel comparison shared by the fixtures: sRGB round-trip through the
/// f16 tint and the render target moves a channel by at most one step.
fn close(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(l, r)| l.abs_diff(r) <= 2)
}

/// Assert the pixel sits strictly between the two source texels on both
/// the red and blue channels — i.e. the sampler blended rather than
/// picked one texel.
fn assert_blend(pixel: [u8; 4], label: &str) {
    for c in [0, 2] {
        let (lo, hi) = (RED[c].min(BLUE[c]), RED[c].max(BLUE[c]));
        assert!(
            pixel[c] > lo + 20 && pixel[c] < hi - 20,
            "{label} channel {c} = {} must ramp between {lo} and {hi}",
            pixel[c],
        );
    }
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
                    ImageFit::Fill,
                    ImageFilter::Nearest,
                    ImageFilter::Linear,
                );
                strip_pane(
                    ui,
                    &handle,
                    128.0,
                    Vec2::new(128.0, 64.0),
                    ImageFit::Fill,
                    ImageFilter::Linear,
                    ImageFilter::Nearest,
                );
            });
    });

    let px = |x: u32| magnified.get_pixel(x, 32).0;

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
                    ImageFit::Fill,
                    ImageFilter::Nearest,
                    ImageFilter::Linear,
                );
                strip_pane(
                    ui,
                    &handle,
                    2.0,
                    Vec2::new(2.0, 16.0),
                    ImageFit::Fill,
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

/// The shader keeps its footprint measurement behind the nearest-flag
/// branch, so the zero-flag (bilinear), both-nearest, and tiled
/// combinations each need their own pin at a fractional texel-per-pixel
/// ratio — where the two filters land on different texels.
///
/// Strip fixture: RED|BLUE|RED across 100px → 33.33px per texel, so the
/// per-fragment texel coordinate is `t = (x + 0.5) · 3 / 100`.
/// - Bilinear samples the texel *centers* at 0.5 / 1.5 / 2.5:
///   `t(16) = 0.495` and `t(83) = 2.505` sit outside the outermost
///   centers → clamped to pure RED; `t(32) = 0.975` is 47.5% of the way
///   to texel 1 → a ramp.
/// - Both-nearest snaps to `floor(t)`: `t(32) = 0.975` → RED and
///   `t(33) = 1.005` → BLUE, a hard seam exactly where bilinear ramps;
///   `t(66) = 1.995` → BLUE and `t(67) = 2.025` → RED.
///
/// Tile fixture: RED|BLUE with `scale = 2.5` across 100px → 2.5 repeats,
/// so `uv = (x + 0.5) / 40` and the shader's `fract` wrap gives
/// `t = fract(uv) · 2`.
/// - Wrap is filter-independent: `t(39)` lands past the last texel
///   center (BLUE) and `t(40)` restarts the tile (RED) under both.
/// - Inside a repeat the filters diverge: `t(20) = 1.025` blends under
///   bilinear but snaps to BLUE under nearest.
/// - `x = 81` is inside the truncated third repeat
///   (`fract(2.0375) = 0.0375` → `t = 0.075`, below the first texel
///   center) → RED under both filters, which pins that `fract` runs per
///   fragment rather than the sampler clamping the last partial tile.
#[test]
fn bilinear_both_nearest_and_tiled_sampling_paths_are_pinned() {
    let mut h = Harness::new();
    let mut strip: Option<aperture::ImageHandle> = None;
    let strips = h.render(UVec2::new(200, 32), 1.0, Color::BLACK, |ui| {
        let handle = strip
            .get_or_insert_with(|| {
                ui.register_image(aperture::Image::from_rgba8(3, 1, [RED, BLUE, RED].concat()))
                    .expect("fixture image fits every supported GPU")
            })
            .clone();
        Panel::canvas()
            .id_salt("branch_fixture")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                strip_pane(
                    ui,
                    &handle,
                    0.0,
                    Vec2::new(100.0, 32.0),
                    ImageFit::Fill,
                    ImageFilter::Linear,
                    ImageFilter::Linear,
                );
                strip_pane(
                    ui,
                    &handle,
                    100.0,
                    Vec2::new(100.0, 32.0),
                    ImageFit::Fill,
                    ImageFilter::Nearest,
                    ImageFilter::Nearest,
                );
            });
    });

    let px = |x: u32| strips.get_pixel(x, 16).0;
    assert!(close(px(16), RED), "bilinear left clamp must be RED");
    assert!(close(px(83), RED), "bilinear right clamp must be RED");
    assert_blend(px(32), "bilinear seam");
    for (x, expected, name) in [
        (32, RED, "both-nearest first seam-left"),
        (33, BLUE, "both-nearest first seam-right"),
        (66, BLUE, "both-nearest second seam-left"),
        (67, RED, "both-nearest second seam-right"),
    ] {
        assert!(close(px(100 + x), expected), "{name} must be {expected:?}");
    }

    let mut tile: Option<aperture::ImageHandle> = None;
    let tiled = h.render(UVec2::new(200, 16), 1.0, Color::BLACK, |ui| {
        let handle = tile
            .get_or_insert_with(|| {
                ui.register_image(aperture::Image::from_rgba8(2, 1, [RED, BLUE].concat()))
                    .expect("fixture image fits every supported GPU")
            })
            .clone();
        let fit = ImageFit::Tile {
            offset: Vec2::ZERO,
            scale: Vec2::new(2.5, 1.0),
        };
        Panel::canvas()
            .id_salt("tile_fixture")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                strip_pane(
                    ui,
                    &handle,
                    0.0,
                    Vec2::new(100.0, 16.0),
                    fit,
                    ImageFilter::Linear,
                    ImageFilter::Linear,
                );
                strip_pane(
                    ui,
                    &handle,
                    100.0,
                    Vec2::new(100.0, 16.0),
                    fit,
                    ImageFilter::Nearest,
                    ImageFilter::Nearest,
                );
            });
    });

    let tpx = |x: u32| tiled.get_pixel(x, 8).0;
    for (base, name) in [(0, "tiled bilinear"), (100, "tiled both-nearest")] {
        assert!(close(tpx(base), RED), "{name} tile start must be RED");
        assert!(close(tpx(base + 39), BLUE), "{name} tile end must be BLUE");
        assert!(close(tpx(base + 40), RED), "{name} must wrap back to RED");
        assert!(
            close(tpx(base + 81), RED),
            "{name} partial third repeat must be RED"
        );
    }
    assert_blend(tpx(20), "tiled bilinear intra-tile seam");
    assert!(
        close(tpx(100 + 19), RED),
        "tiled nearest intra-tile seam-left must be RED"
    );
    assert!(
        close(tpx(100 + 20), BLUE),
        "tiled nearest intra-tile seam-right must be BLUE"
    );
}

/// Third solid source for the run-coalescing fixture, distinct from
/// [`RED`] / [`BLUE`] so every run boundary is a visible colour change.
const GREEN: [u8; 4] = [60, 200, 90, 255];

/// Adjacent draws sharing a texture collapse into one instanced draw
/// (`image_runs`). Composited output must be byte-identical to the
/// one-draw-per-image walk that preceded it, so this paints a pattern
/// carrying every case that distinguishes them and reads back the pane
/// each instance landed in:
///
/// - `A A` — a leading run, the case that actually coalesces.
/// - `B` then `A` — singletons, and `A`'s second appearance is a
///   *non-adjacent* repeat that must stay its own run. Merging it would
///   paint it before `B`.
/// - `C C` — a trailing run, which pins that the last span closes at the
///   batch end rather than one short.
///
/// A drifting instance range shows up as a pane painting its neighbour's
/// colour, and a dropped run as a pane left at the clear colour — both
/// caught by the per-pane centre-pixel assertion.
#[test]
fn adjacent_same_texture_runs_composite_identically_to_per_draw() {
    const PATTERN: [usize; 6] = [0, 0, 1, 0, 2, 2];
    const SOURCES: [[u8; 4]; 3] = [RED, BLUE, GREEN];
    const PANE: f32 = 32.0;

    let mut h = Harness::new();
    let mut sources: Option<[aperture::ImageHandle; 3]> = None;
    let out = h.render(UVec2::new(192, 32), 1.0, Color::BLACK, |ui| {
        let handles = sources.get_or_insert_with(|| {
            SOURCES.map(|texel| {
                ui.register_image(aperture::Image::from_rgba8(1, 1, texel.to_vec()))
                    .expect("fixture image fits every supported GPU")
            })
        });
        Panel::canvas()
            .id_salt("coalesce_fixture")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (pane, &source) in PATTERN.iter().enumerate() {
                    strip_pane(
                        ui,
                        &handles[source],
                        pane as f32 * PANE,
                        Vec2::new(PANE, 32.0),
                        ImageFit::Fill,
                        ImageFilter::Linear,
                        ImageFilter::Linear,
                    );
                }
            });
    });

    for (pane, &source) in PATTERN.iter().enumerate() {
        let expected = SOURCES[source];
        let x = pane as u32 * PANE as u32 + PANE as u32 / 2;
        let pixel = out.get_pixel(x, 16).0;
        assert!(
            close(pixel, expected),
            "pane {pane} draws source {source}: expected {expected:?}, got {pixel:?}"
        );
    }
}
