//! Image sampling fixtures: exact-pixel assertions (no goldens) with values hand-derived from the source texels, pinning sampling semantics.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use glam::{UVec2, Vec2};
use palantir::widget::Shape;
use palantir::{
    Configure, Image, ImageDownsample, ImageFilter, ImageFit, Panel, RgbaF32, Sizing, Ui,
};

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px};
use crate::harness::Harness;
use std::iter;

#[test]
fn image_updates_copy_pixels_and_repaint_every_clone() {
    let mut h = Harness::new();
    let size = UVec2::new(4, 2);
    let mut image = Image::from_srgba8(UVec2::new(2, 2), [RED, BLUE, BLUE, RED].concat()).unwrap();
    let handle = h.host.ui().load_image(&image).unwrap();
    let clone = handle.clone();
    drop(handle);
    assert_eq!(h.host.gpu_image_cache_len(), 1);

    image.texels_mut().rotate_left(1);
    let scene = |ui: &mut Ui| {
        Panel::canvas()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for x in [0.0, 2.0] {
                    strip_pane(
                        ui,
                        &clone,
                        x,
                        Vec2::splat(2.0),
                        ImageFit::Fill,
                        ImageFilter::Nearest,
                        ImageFilter::Nearest,
                    );
                }
            });
    };
    for expected in [[RED, BLUE, BLUE, RED], [BLUE, BLUE, RED, RED]] {
        let out = h.size(size).clear(RgbaF32::BLACK).frame(scene).image;
        for y in 0..2 {
            for x in 0..4 {
                let want = expected[(y * 2 + x % 2) as usize];
                assert_px(
                    out.get_pixel(x, y).0,
                    want,
                    SRGB_ROUND_TRIP,
                    format_args!("pixel ({x}, {y})"),
                );
            }
        }
        clone.update(&image);
        image.texels_mut().fill(palantir::SrgbaU8::default());
        assert_eq!(h.host.gpu_image_cache_len(), 1);
    }
    drop(clone);
    assert_eq!(h.host.gpu_image_cache_len(), 0);
}

const RED: [u8; 4] = [230, 60, 60, 255];
const BLUE: [u8; 4] = [60, 120, 230, 255];

fn strip_pane(
    ui: &mut Ui,
    handle: &palantir::ImageHandle,
    x: f32,
    size: Vec2,
    fit: ImageFit,
    min_filter: ImageFilter,
    mag_filter: ImageFilter,
) {
    Panel::zstack()
        .id_salt(("filter_pane", x as i32))
        .position(Vec2::new(x, 0.0))
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

/// Minification and magnification choose their own filters:
/// - Both: x=16 / x=112 sit in the texel-center clamp region: RED / BLUE (±2 sRGB).
/// - Nearest: hard seam, x=63 RED, x=64 BLUE (floor(uv · 2): 63.5/128·2 = 0.99 vs 64.5/128·2 = 1.01).
/// - Linear: x=64 is mid-ramp.
/// - Downscaling RED|BLUE|RED|BLUE 4px to 2px: nearest picks BLUE at each boundary, linear blends.
#[test]
fn minification_and_magnification_filters_are_independent() {
    let mut h = Harness::new();
    let mut mag_strip: Option<palantir::ImageHandle> = None;
    let magnified = h
        .size(UVec2::new(256, 64))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handle = mag_strip
                .get_or_insert_with(|| {
                    ui.load_image(
                        &Image::from_srgba8(UVec2::new(2, 1), [RED, BLUE].concat()).unwrap(),
                    )
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
        })
        .image;

    let px = |x: u32| magnified.get_pixel(x, 32).0;

    for (base, name) in [(0, "linear magnification"), (128, "nearest magnification")] {
        assert_px(
            px(base + 16),
            RED,
            SRGB_ROUND_TRIP,
            format_args!("{name} left half must be RED"),
        );
        assert_px(
            px(base + 112),
            BLUE,
            SRGB_ROUND_TRIP,
            format_args!("{name} right half must be BLUE"),
        );
    }

    assert_px(
        px(128 + 63),
        RED,
        SRGB_ROUND_TRIP,
        format_args!("nearest seam-left must be RED"),
    );
    assert_px(
        px(128 + 64),
        BLUE,
        SRGB_ROUND_TRIP,
        format_args!("nearest seam-right must be BLUE"),
    );
    assert_blend(px(64), "linear magnification seam");

    let mut min_strip: Option<palantir::ImageHandle> = None;
    let minified = h
        .size(UVec2::new(4, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handle = min_strip
                .get_or_insert_with(|| {
                    ui.load_image(
                        &Image::from_srgba8(UVec2::new(4, 1), [RED, BLUE, RED, BLUE].concat())
                            .unwrap(),
                    )
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
        })
        .image;

    for x in 0..2 {
        assert_px(
            minified.get_pixel(x, 8).0,
            BLUE,
            SRGB_ROUND_TRIP,
            format_args!("nearest minification pixel {x} must select BLUE"),
        );
    }
    for x in 2..4 {
        assert_blend(
            minified.get_pixel(x, 8).0,
            &format!("linear minification pixel {x}"),
        );
    }
}

/// The shader keeps footprint measurement behind the nearest-flag branch, so bilinear, both-nearest and tiled each need a pin at a fractional texel-per-pixel ratio.
///
/// Strip: RED|BLUE|RED across 100px, `t = (x + 0.5) · 3 / 100`.
/// - Bilinear: `t(16) = 0.495` and `t(83) = 2.505` clamp to RED; `t(32) = 0.975` is a ramp.
/// - Both-nearest: `floor(t)`, so `t(32)` RED, `t(33) = 1.005` BLUE; `t(66) = 1.995` BLUE, `t(67) = 2.025` RED.
///
/// Tile: RED|BLUE, `scale = 2.5` across 100px, `t = fract((x + 0.5) / 40) · 2`.
/// - At every seam nearest steps while bilinear blends; `t(20) = 1.025` blends or snaps to BLUE.
/// - `x = 81` (`fract(2.0375) = 0.0375`, `t = 0.075`) is RED under nearest: the wrap runs per fragment.
#[test]
fn bilinear_both_nearest_and_tiled_sampling_paths_are_pinned() {
    let mut h = Harness::new();
    let mut strip: Option<palantir::ImageHandle> = None;
    let strips = h
        .size(UVec2::new(200, 32))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handle = strip
                .get_or_insert_with(|| {
                    ui.load_image(
                        &Image::from_srgba8(UVec2::new(3, 1), [RED, BLUE, RED].concat()).unwrap(),
                    )
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
        })
        .image;

    let px = |x: u32| strips.get_pixel(x, 16).0;
    assert_px(
        px(16),
        RED,
        SRGB_ROUND_TRIP,
        format_args!("bilinear left clamp must be RED"),
    );
    assert_px(
        px(83),
        RED,
        SRGB_ROUND_TRIP,
        format_args!("bilinear right clamp must be RED"),
    );
    assert_blend(px(32), "bilinear seam");
    for (x, expected, name) in [
        (32, RED, "both-nearest first seam-left"),
        (33, BLUE, "both-nearest first seam-right"),
        (66, BLUE, "both-nearest second seam-left"),
        (67, RED, "both-nearest second seam-right"),
    ] {
        assert_px(
            px(100 + x),
            expected,
            SRGB_ROUND_TRIP,
            format_args!("{name} must be {expected:?}"),
        );
    }

    let mut tile: Option<palantir::ImageHandle> = None;
    let tiled = h
        .size(UVec2::new(200, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handle = tile
                .get_or_insert_with(|| {
                    ui.load_image(
                        &Image::from_srgba8(UVec2::new(2, 1), [RED, BLUE].concat()).unwrap(),
                    )
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
        })
        .image;

    let tpx = |x: u32| tiled.get_pixel(x, 8).0;
    // Bilinear over a repeat blends across every seam, edges included: at 20 px per texel, x = 0 and x = 39 read a blend, not the pure texel a clamp would give.
    assert_px(
        tpx(0),
        [176, 94, 170, 255],
        SRGB_ROUND_TRIP,
        format_args!(
            "tiled bilinear must open on the seam blend, got {:?}",
            tpx(0)
        ),
    );
    assert_px(
        tpx(39),
        [170, 97, 176, 255],
        SRGB_ROUND_TRIP,
        format_args!("and close on it, got {:?}", tpx(39)),
    );
    assert_px(
        tpx(40),
        tpx(0),
        SRGB_ROUND_TRIP,
        format_args!("the next repeat opens the same way"),
    );
    assert_px(
        tpx(79),
        tpx(39),
        SRGB_ROUND_TRIP,
        format_args!("and closes the same way"),
    );
    assert_blend(tpx(20), "tiled bilinear intra-tile seam");

    for (x, expected, name) in [
        (0, RED, "tile start"),
        (39, BLUE, "tile end"),
        (40, RED, "wrap back"),
        (81, RED, "partial third repeat"),
    ] {
        assert_px(
            tpx(100 + x),
            expected,
            SRGB_ROUND_TRIP,
            format_args!(
                "tiled both-nearest {name} must be {expected:?}, got {:?}",
                tpx(100 + x)
            ),
        );
    }
    assert_px(
        tpx(100 + 19),
        RED,
        SRGB_ROUND_TRIP,
        format_args!("tiled nearest intra-tile seam-left must be RED"),
    );
    assert_px(
        tpx(100 + 20),
        BLUE,
        SRGB_ROUND_TRIP,
        format_args!("tiled nearest intra-tile seam-right must be BLUE"),
    );
}

/// Source: one lit texel per three, on black; white and black are linear 1.0 and 0.0, so expectations are plain fractions.
const STAR: [u8; 4] = [255, 255, 255, 255];
const SKY: [u8; 4] = [0, 0, 0, 255];

/// Each [`ImageDownsample`] mode's answer for a lit texel the single bilinear tap misses.
///
/// **Geometry.** A 24x1 `STAR SKY SKY` source in an 8x16 pane: 3 texels per pixel, taps per axis `clamp(ceil(3 · 0.5), 1, 4) = 2`. Pixel `x` covers texels `3x..3x+2`, centre `3x + 1.5`.
///
/// **Taps.** The 2x2 grid offsets `±0.75` texels: `3x + 0.75` is `0.75·T0 + 0.25·T1`, `3x + 2.25` is `0.25·T1 + 0.75·T2`.
///
/// **Results**, `T0 = STAR` (1.0), `T1 = T2 = SKY` (0.0):
/// - `Single` samples T1's exact centre: `0.0`.
/// - `Mean` = `(0.75 + 0 + 0 + 0) / 2` = `0.375` linear, sRGB `1.055 · 0.375^(1/2.4) − 0.055` = 0.6461, **165**.
/// - `Peak` = `0.75` linear, sRGB 0.8808, **225**.
#[test]
fn downsample_modes_recover_a_texel_the_single_tap_misses() {
    const PANE: Vec2 = Vec2::new(8.0, 16.0);
    let cases = [
        (ImageDownsample::Single, 0u8, "Single"),
        (ImageDownsample::Mean, 165, "Mean"),
        (ImageDownsample::Peak, 225, "Peak"),
    ];

    let mut h = Harness::new();
    let mut source: Option<palantir::ImageHandle> = None;
    let out = h
        .size(UVec2::new(24, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handle = source
                .get_or_insert_with(|| {
                    let texels: Vec<u8> = iter::repeat_n([STAR, SKY, SKY], 8)
                        .flatten()
                        .flatten()
                        .collect();
                    ui.load_image(&Image::from_srgba8(UVec2::new(24, 1), texels).unwrap())
                        .expect("fixture image fits every supported GPU")
                })
                .clone();
            Panel::canvas()
                .id_salt("downsample_fixture")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for (i, (mode, _, _)) in cases.iter().enumerate() {
                        let x = i as f32 * PANE.x;
                        Panel::zstack()
                            .id_salt(("downsample_pane", i))
                            .position(Vec2::new(x, 0.0))
                            .size((Sizing::fixed(PANE.x), Sizing::fixed(PANE.y)))
                            .show(ui, |ui| {
                                ui.add_shape(
                                    Shape::image(handle.clone())
                                        .fit(ImageFit::Fill)
                                        .downsample(*mode),
                                );
                            });
                    }
                });
        })
        .image;

    let mut measured = Vec::with_capacity(cases.len());
    for (i, (_, expected, label)) in cases.iter().enumerate() {
        let pixel = out.get_pixel(i as u32 * PANE.x as u32 + 4, 8).0;
        assert_px(
            pixel,
            [*expected, *expected, *expected, 255],
            SRGB_ROUND_TRIP,
            format_args!("{label} must read {expected} grey, got {pixel:?}"),
        );
        assert_eq!(
            [pixel[0], pixel[1], pixel[2]],
            [pixel[0]; 3],
            "{label} must stay neutral — a white star cannot gain a hue",
        );
        measured.push(pixel[0]);
    }

    assert!(
        measured[0] < measured[1] && measured[1] < measured[2],
        "Single < Mean < Peak must hold, got {measured:?}",
    );
}

/// Taps combine *premultiplied*, so both modes are correct over alpha (geometry as above; `T1` clear, so each tap is three quarters of an outer texel).
///
/// **Mean.** `WHITE(α=128) CLEAR CLEAR`: the lit tap is full-strength white at 0.75 coverage; the mean is `0.75 · α / 2 = 0.1882` linear, **120** sRGB. Straight colour applies coverage twice: 105.
///
/// **Peak.** `WHITE(α=26) CLEAR GREY(α=255)`, where orderings disagree: straight luma picks the faint white (0.75 vs 0.162), premultiplied the grey (0.121 vs 0.057). Grey sRGB 128 = 0.21586 linear, tap `0.75 · 0.21586 = 0.1619`, **113** sRGB (white 68, double coverage 98).
#[test]
fn downsample_combines_taps_in_premultiplied_space() {
    const PANE: Vec2 = Vec2::new(8.0, 16.0);
    const CLEAR: [u8; 4] = [0, 0, 0, 0];
    const DIM_WHITE: [u8; 4] = [255, 255, 255, 128];
    const FAINT_WHITE: [u8; 4] = [255, 255, 255, 26];
    const SOLID_GREY: [u8; 4] = [128, 128, 128, 255];

    let cases = [
        (
            [DIM_WHITE, CLEAR, CLEAR],
            ImageDownsample::Mean,
            120u8,
            "Mean over alpha",
        ),
        (
            [FAINT_WHITE, CLEAR, SOLID_GREY],
            ImageDownsample::Peak,
            113,
            "Peak ranking over alpha",
        ),
    ];

    let mut h = Harness::new();
    let mut sources: Option<Vec<palantir::ImageHandle>> = None;
    let out = h
        .size(UVec2::new(16, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handles = sources.get_or_insert_with(|| {
                cases
                    .iter()
                    .map(|(triple, _, _, _)| {
                        let texels: Vec<u8> =
                            iter::repeat_n(*triple, 8).flatten().flatten().collect();
                        ui.load_image(&Image::from_srgba8(UVec2::new(24, 1), texels).unwrap())
                            .expect("fixture image fits every supported GPU")
                    })
                    .collect()
            });
            Panel::canvas()
                .id_salt("premultiplied_fixture")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for (i, (_, mode, _, _)) in cases.iter().enumerate() {
                        Panel::zstack()
                            .id_salt(("premultiplied_pane", i))
                            .position(Vec2::new(i as f32 * PANE.x, 0.0))
                            .size((Sizing::fixed(PANE.x), Sizing::fixed(PANE.y)))
                            .show(ui, |ui| {
                                ui.add_shape(
                                    Shape::image(handles[i].clone())
                                        .fit(ImageFit::Fill)
                                        .downsample(*mode),
                                );
                            });
                    }
                });
        })
        .image;

    for (i, (_, _, expected, label)) in cases.iter().enumerate() {
        let pixel = out.get_pixel(i as u32 * PANE.x as u32 + 4, 8).0;
        assert_px(
            pixel,
            [*expected, *expected, *expected, 255],
            SRGB_ROUND_TRIP,
            format_args!("{label} must read {expected} grey, got {pixel:?}"),
        );
    }
}

/// A magnified edge keeps its colour: bilinear between opaque and transparent texels needs premultiplied storage, else soft edges darken.
///
/// `RED CLEAR` across 16 px; pixel 7 samples uv `7.5/16 = 0.46875`, `t = 0.4375` between centres 0.25 and 0.75: `0.5625` red at `0.5625` coverage, `0.5625` linear over black, **199** sRGB. Straight colour multiplies twice: 153.
#[test]
fn a_magnified_transparent_edge_keeps_its_colour() {
    const CLEAR: [u8; 4] = [0, 0, 0, 0];
    const RED: [u8; 4] = [255, 0, 0, 255];

    let mut h = Harness::new();
    let mut source: Option<palantir::ImageHandle> = None;
    let out = h
        .size(UVec2::new(16, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handle = source
                .get_or_insert_with(|| {
                    let texels: Vec<u8> = [RED, CLEAR].into_iter().flatten().collect();
                    ui.load_image(&Image::from_srgba8(UVec2::new(2, 1), texels).unwrap())
                        .expect("fixture image fits every supported GPU")
                })
                .clone();
            Panel::canvas()
                .id_salt("fringe_fixture")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id_salt("fringe_pane")
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            ui.add_shape(Shape::image(handle.clone()).fit(ImageFit::Fill));
                        });
                });
        })
        .image;

    let pixel = out.get_pixel(7, 8).0;
    assert_px(
        pixel,
        [199, 0, 0, 255],
        SRGB_ROUND_TRIP,
        format_args!("half-covered red must stay red at half coverage, got {pixel:?}"),
    );
}

/// Taps wrap with the tile instead of clamping: a tap steps off the base UV by up to half the footprint, and `tap` wraps each fetch into the neighbouring repeat.
///
/// **Geometry.** A 4x1 `STAR SKY SKY SKY` tile across an 8x16 pane: 3 tiles per pixel, `uv_dx = 3`, footprint 12 texels, `n = 4`. Every pixel's base UV wraps to exactly 0.5 (`fract(3k + 1.5)`).
///
/// **Taps.** `n = 4` offsets `±0.375` and `±1.125` tiles: `-0.625, 0.125, 0.875, 1.625`.
/// - Wrapped: `0.375, 0.125, 0.875, 0.625`, texel coords `1.5, 0.5, 3.5, 2.5`: `SKY STAR SKY SKY`, mean `0.25` linear, **137** sRGB.
/// - Clamped: `STAR STAR SKY SKY`, mean `0.5`, 188; 51 steps apart.
#[test]
fn downsample_taps_wrap_with_the_tile_instead_of_clamping() {
    let mut h = Harness::new();
    let mut source: Option<palantir::ImageHandle> = None;
    let out = h
        .size(UVec2::new(8, 16))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handle = source
                .get_or_insert_with(|| {
                    ui.load_image(
                        &Image::from_srgba8(UVec2::new(4, 1), [STAR, SKY, SKY, SKY].concat())
                            .unwrap(),
                    )
                    .expect("fixture image fits every supported GPU")
                })
                .clone();
            Panel::zstack()
                .id_salt("tiled_downsample_fixture")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::image(handle.clone())
                            .fit(ImageFit::Tile {
                                offset: Vec2::ZERO,
                                scale: Vec2::new(24.0, 1.0),
                            })
                            .downsample(ImageDownsample::Mean),
                    );
                });
        })
        .image;

    for x in 0..8 {
        let pixel = out.get_pixel(x, 8).0;
        assert_px(
            pixel,
            [137, 137, 137, 255],
            SRGB_ROUND_TRIP,
            format_args!("tiled tap column {x} must read 137 grey, got {pixel:?}"),
        );
    }
}

const GREEN: [u8; 4] = [60, 200, 90, 255];

/// Adjacent draws sharing a texture collapse into one instanced draw (`image_runs`), byte-identical to one draw per image:
///
/// - `A A`: a leading run, the case that coalesces.
/// - `B` then `A`: singletons; `A`'s second appearance is non-adjacent and must stay its own run, else it paints before `B`.
/// - `C C`: a trailing run, pinning that the last span closes at the batch end.
///
/// Read back per pane: a drifting range paints a neighbour's colour, a dropped run the clear colour.
#[test]
fn adjacent_same_texture_runs_composite_identically_to_per_draw() {
    const PATTERN: [usize; 6] = [0, 0, 1, 0, 2, 2];
    const SOURCES: [[u8; 4]; 3] = [RED, BLUE, GREEN];
    const PANE: f32 = 32.0;

    let mut h = Harness::new();
    let mut sources: Option<[palantir::ImageHandle; 3]> = None;
    let out = h
        .size(UVec2::new(192, 32))
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            let handles = sources.get_or_insert_with(|| {
                SOURCES.map(|texel| {
                    ui.load_image(&Image::from_srgba8(UVec2::new(1, 1), texel.to_vec()).unwrap())
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
        })
        .image;

    for (pane, &source) in PATTERN.iter().enumerate() {
        let expected = SOURCES[source];
        let x = pane as u32 * PANE as u32 + PANE as u32 / 2;
        let pixel = out.get_pixel(x, 16).0;
        assert_px(
            pixel,
            expected,
            SRGB_ROUND_TRIP,
            format_args!("pane {pane} draws source {source}: expected {expected:?}, got {pixel:?}"),
        );
    }
}
