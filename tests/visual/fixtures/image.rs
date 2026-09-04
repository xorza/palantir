//! Image sampling fixtures. Exact-pixel assertions (no goldens): the
//! expected values are hand-derived from the source texels, so the test
//! is machine-independent and pins the *sampling semantics*, not a
//! rendered snapshot.

use glam::{UVec2, Vec2};
use palantir::{
    Configure, ImageDownsample, ImageFilter, ImageFit, Panel, RgbaF32, Shape, Sizing, Ui,
};

use crate::fixtures::close;
use crate::harness::Harness;

/// Source texels for the filter fixture: a 2×1 red|blue strip. Upscaled
/// 64× horizontally, the two filters must diverge only around the seam.
const RED: [u8; 4] = [230, 60, 60, 255];
const BLUE: [u8; 4] = [60, 120, 230, 255];

/// One exactly placed pane painting a strip with independent filters.
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
    let mut mag_strip: Option<palantir::ImageHandle> = None;
    let magnified = h.render(UVec2::new(256, 64), 1.0, RgbaF32::BLACK, |ui| {
        let handle = mag_strip
            .get_or_insert_with(|| {
                ui.register_image(palantir::Image::from_rgba8(2, 1, [RED, BLUE].concat()))
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

    let mut min_strip: Option<palantir::ImageHandle> = None;
    let minified = h.render(UVec2::new(4, 16), 1.0, RgbaF32::BLACK, |ui| {
        let handle = min_strip
            .get_or_insert_with(|| {
                ui.register_image(palantir::Image::from_rgba8(
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
    let mut strip: Option<palantir::ImageHandle> = None;
    let strips = h.render(UVec2::new(200, 32), 1.0, RgbaF32::BLACK, |ui| {
        let handle = strip
            .get_or_insert_with(|| {
                ui.register_image(palantir::Image::from_rgba8(3, 1, [RED, BLUE, RED].concat()))
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

    let mut tile: Option<palantir::ImageHandle> = None;
    let tiled = h.render(UVec2::new(200, 16), 1.0, RgbaF32::BLACK, |ui| {
        let handle = tile
            .get_or_insert_with(|| {
                ui.register_image(palantir::Image::from_rgba8(2, 1, [RED, BLUE].concat()))
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

/// Downsample fixture source: one lit texel per three, on black. White and
/// black because they are linear 1.0 and 0.0 exactly, so every expected value
/// below is a plain fraction of full scale.
const STAR: [u8; 4] = [255, 255, 255, 255];
const SKY: [u8; 4] = [0, 0, 0, 255];

/// Each [`ImageDownsample`] mode's answer for a lit texel that the single
/// bilinear tap misses entirely — the aliasing this feature exists to fix,
/// reduced to three hand-computable pixels.
///
/// **Geometry.** A 24×1 source of `STAR SKY SKY` repeated 8× painted into an
/// 8×16 pane: 3 texels per pixel, so `uv_dx = 1/8` and `texel_dx = 3`, giving
/// a footprint of exactly 3 texels. Taps per axis is
/// `clamp(ceil(3 · 0.5), 1, 4) = 2` — `ceil(1.5)` sits mid-bucket, so no
/// float wobble can change the count. Pixel `x` covers texels
/// `T0 T1 T2 = 3x, 3x+1, 3x+2`, and its centre is texel coord `3x + 1.5`.
///
/// **Taps.** The 2×2 grid offsets by `±0.25` of the derivative span, i.e.
/// `±0.75` texels, landing at `3x + 0.75` and `3x + 2.25` (the two rows
/// duplicate them — the source is one texel tall, so the vertical offset
/// samples the same row). Each is a bilinear tap:
/// - `3x + 0.75` is 0.25 of the way from T0's centre to T1's → `0.75·T0 + 0.25·T1`
/// - `3x + 2.25` is 0.75 of the way from T1's centre to T2's → `0.25·T1 + 0.75·T2`
///
/// **Results**, with `T0 = STAR` (linear 1.0) and `T1 = T2 = SKY` (0.0), all
/// weights being exact binary fractions:
/// - `Single` samples once at the pixel centre, `3x + 1.5`, which is *exactly*
///   T1's centre → `0.0`. The star is in the footprint and contributes
///   nothing: sub-pixel motion is what swaps which texel that centre lands on,
///   and that swap is the blinking.
/// - `Mean` = `(0.75 + 0.25·0 + 0.25·0 + 0.75·0) / 2` → `0.375` linear
///   → sRGB `1.055 · 0.375^(1/2.4) − 0.055` = 0.6461 → **165**.
/// - `Peak` keeps the brighter tap, `0.75` linear
///   → `1.055 · 0.75^(1/2.4) − 0.055` = 0.8808 → **225**.
#[test]
fn downsample_modes_recover_a_texel_the_single_tap_misses() {
    const PANE: Vec2 = Vec2::new(8.0, 16.0);
    // (mode, expected grey, label)
    let cases = [
        (ImageDownsample::Single, 0u8, "Single"),
        (ImageDownsample::Mean, 165, "Mean"),
        (ImageDownsample::Peak, 225, "Peak"),
    ];

    let mut h = Harness::new();
    let mut source: Option<palantir::ImageHandle> = None;
    let out = h.render(UVec2::new(24, 16), 1.0, RgbaF32::BLACK, |ui| {
        let handle = source
            .get_or_insert_with(|| {
                let texels: Vec<u8> = std::iter::repeat_n([STAR, SKY, SKY], 8)
                    .flatten()
                    .flatten()
                    .collect();
                ui.register_image(palantir::Image::from_rgba8(24, 1, texels))
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
    });

    let mut measured = Vec::with_capacity(cases.len());
    for (i, (_, expected, label)) in cases.iter().enumerate() {
        // Mid-pane, away from the pane seams; every pixel in a pane covers an
        // identical `STAR SKY SKY` group, so the column choice is arbitrary.
        let pixel = out.get_pixel(i as u32 * PANE.x as u32 + 4, 8).0;
        assert!(
            close(pixel, [*expected, *expected, *expected, 255]),
            "{label} must read {expected} grey, got {pixel:?}",
        );
        assert_eq!(
            [pixel[0], pixel[1], pixel[2]],
            [pixel[0]; 3],
            "{label} must stay neutral — a white star cannot gain a hue",
        );
        measured.push(pixel[0]);
    }

    // The ordering is the semantic claim, and it holds independently of the
    // sRGB round-trip the tolerance above absorbs: one tap loses the star
    // outright, the area average keeps a fraction of it, and the peak keeps
    // the most.
    assert!(
        measured[0] < measured[1] && measured[1] < measured[2],
        "Single < Mean < Peak must hold, got {measured:?}",
    );
}

/// Taps are combined *premultiplied*, which is what makes both modes correct
/// over alpha. Two panes, one claim each, on the same 3-texels-per-pixel
/// geometry as the fixture above (taps read `0.75·T0 + 0.25·T1` and
/// `0.25·T1 + 0.75·T2`; `T1` is fully clear in both sources, so each tap is
/// just three-quarters of an outer texel).
///
/// **Mean — averaging.** Source `WHITE(α=128) CLEAR CLEAR`. Premultiplied, the
/// lit tap is `0.75·0.75α` over `0.75α`, which un-premultiplies back to
/// `rgb = 0.75` — the white survives at full strength and only the *coverage*
/// halves. Straight-alpha averaging would instead have dragged rgb to
/// `(0.75 + 0)/2 = 0.375`, halving the colour a second time. Composited over
/// black the output is `rgb · a = 0.75 · 0.375α = 0.28125α`, and with
/// `α = 128/255` that is 0.14118 linear → **105** sRGB (the straight-alpha
/// answer would read 75).
///
/// **Peak — ranking.** Source `WHITE(α=26) CLEAR GREY(α=255)`, chosen so the
/// two orderings disagree: by straight luma the near-invisible white wins
/// (0.75 vs 0.162), by premultiplied luma the solid grey does (0.121 vs
/// 0.057). Grey is sRGB 128 = 0.21586 linear, so the winning tap
/// un-premultiplies to `rgb = 0.75 · 0.21586`, `a = 0.75`, and the composite
/// is `0.12138` linear → **98** sRGB (picking the white would read 68).
#[test]
fn downsample_combines_taps_in_premultiplied_space() {
    const PANE: Vec2 = Vec2::new(8.0, 16.0);
    const CLEAR: [u8; 4] = [0, 0, 0, 0];
    const DIM_WHITE: [u8; 4] = [255, 255, 255, 128];
    const FAINT_WHITE: [u8; 4] = [255, 255, 255, 26];
    const SOLID_GREY: [u8; 4] = [128, 128, 128, 255];

    // (texel triple, mode, expected grey, label)
    let cases = [
        (
            [DIM_WHITE, CLEAR, CLEAR],
            ImageDownsample::Mean,
            105u8,
            "Mean over alpha",
        ),
        (
            [FAINT_WHITE, CLEAR, SOLID_GREY],
            ImageDownsample::Peak,
            98,
            "Peak ranking over alpha",
        ),
    ];

    let mut h = Harness::new();
    let mut sources: Option<Vec<palantir::ImageHandle>> = None;
    let out = h.render(UVec2::new(16, 16), 1.0, RgbaF32::BLACK, |ui| {
        let handles = sources.get_or_insert_with(|| {
            cases
                .iter()
                .map(|(triple, _, _, _)| {
                    let texels: Vec<u8> = std::iter::repeat_n(*triple, 8)
                        .flatten()
                        .flatten()
                        .collect();
                    ui.register_image(palantir::Image::from_rgba8(24, 1, texels))
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
    });

    for (i, (_, _, expected, label)) in cases.iter().enumerate() {
        let pixel = out.get_pixel(i as u32 * PANE.x as u32 + 4, 8).0;
        assert!(
            close(pixel, [*expected, *expected, *expected, 255]),
            "{label} must read {expected} grey, got {pixel:?}",
        );
    }
}

/// Taps wrap with the tile rather than clamping at its edge. `fs` wraps the
/// *base* UV for a tiled draw, but the taps step off that UV by up to half the
/// footprint and leave `[0,1)` on their own — where `ClampToEdge` would smear
/// the edge texel across every seam instead of continuing into the next
/// repeat.
///
/// **Geometry.** A 4×1 `STAR SKY SKY SKY` tile repeated 24× across an 8×16
/// pane: 3 whole tiles per pixel, so `uv_dx = 3` and the footprint is 12
/// texels — past the cap, so `n = 4` and no float wobble can move it. Every
/// pixel's base UV wraps to exactly 0.5 (`fract(3k + 1.5)`), which is why one
/// expected value covers the whole pane.
///
/// **Taps.** `n = 4` offsets by `±0.125` and `±0.375` of the derivative span,
/// i.e. `±0.375` and `±1.125` tiles, so the four positions are
/// `-0.625, 0.125, 0.875, 1.625` — two of them outside the tile.
/// - Wrapped: `0.375, 0.125, 0.875, 0.625` → texel coords `1.5, 0.5, 3.5, 2.5`,
///   which are the centres of texels 1, 0, 3, 2 → `SKY STAR SKY SKY`. Mean is
///   `0.25` linear → **137** sRGB.
/// - Clamped: `-0.625` and `1.625` pin to the outer texels → `STAR STAR SKY
///   SKY`, mean `0.5` linear → 188. So the wrap is worth exactly one star in
///   four here, and the assertion below separates the two by 51 sRGB steps.
#[test]
fn downsample_taps_wrap_with_the_tile_instead_of_clamping() {
    let mut h = Harness::new();
    let mut source: Option<palantir::ImageHandle> = None;
    let out = h.render(UVec2::new(8, 16), 1.0, RgbaF32::BLACK, |ui| {
        let handle = source
            .get_or_insert_with(|| {
                ui.register_image(palantir::Image::from_rgba8(
                    4,
                    1,
                    [STAR, SKY, SKY, SKY].concat(),
                ))
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
    });

    // Every pixel is tile-aligned identically, so a single expected value
    // covers the pane — and a seam that clamped would break exactly that.
    for x in 0..8 {
        let pixel = out.get_pixel(x, 8).0;
        assert!(
            close(pixel, [137, 137, 137, 255]),
            "tiled tap column {x} must read 137 grey, got {pixel:?}",
        );
    }
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
    let mut sources: Option<[palantir::ImageHandle; 3]> = None;
    let out = h.render(UVec2::new(192, 32), 1.0, RgbaF32::BLACK, |ui| {
        let handles = sources.get_or_insert_with(|| {
            SOURCES.map(|texel| {
                ui.register_image(palantir::Image::from_rgba8(1, 1, texel.to_vec()))
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
