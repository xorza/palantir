//! Image sampling fixtures: exact-pixel assertions (no goldens) with values hand-derived from the source texels, pinning sampling semantics.

#![expect(
    clippy::cast_sign_loss,
    reason = "test fixtures cast non-negative sizes, coordinates, indices and colour channels"
)]

use glam::{UVec2, Vec2};
use palantir::golden::image::RgbaImage;
use palantir::widget::{ImageShape, Shape};
use palantir::{
    Configure, Image, ImageDownsample, ImageFilter, ImageFit, ImageHandle, Panel, RgbaF32, Sizing,
    SrgbaU8, Ui,
};

use crate::fixtures::{SRGB_ROUND_TRIP, assert_px, canvas};
use crate::harness::Harness;

const RED: [u8; 4] = [230, 60, 60, 255];
const BLUE: [u8; 4] = [60, 120, 230, 255];
const GREEN: [u8; 4] = [60, 200, 90, 255];

/// Source: one lit texel per three, on black; white and black are linear 1.0 and 0.0, so expectations are plain fractions.
const STAR: [u8; 4] = [255, 255, 255, 255];
const SKY: [u8; 4] = [0, 0, 0, 255];

/// `texels` as one row, loaded into `h`'s host.
fn load(h: &mut Harness, texels: &[[u8; 4]]) -> ImageHandle {
    let image = Image::from_srgba8(UVec2::new(texels.len() as u32, 1), texels.concat()).unwrap();
    h.host
        .ui()
        .load_image(&image)
        .expect("fixture image fits every supported GPU")
}

/// One frame on black, with `scene` on a full-surface canvas.
fn render(h: &mut Harness, size: UVec2, mut scene: impl FnMut(&mut Ui)) -> RgbaImage {
    h.size(size)
        .clear(RgbaF32::BLACK)
        .frame(|ui| canvas(ui, &mut scene))
        .image
}

/// `image` in a pane `size` at `(x, 0)`.
fn pane(ui: &mut Ui, x: f32, size: Vec2, image: ImageShape) {
    Panel::zstack()
        .id_salt(("pane", x as i32))
        .position(Vec2::new(x, 0.0))
        .size((Sizing::fixed(size.x), Sizing::fixed(size.y)))
        .show(ui, |ui| ui.add_shape(image));
}

/// `handle` under `fit`, minified with `min` and magnified with `mag`.
fn filtered(handle: &ImageHandle, fit: ImageFit, min: ImageFilter, mag: ImageFilter) -> ImageShape {
    Shape::image(handle.clone())
        .fit(fit)
        .min_filter(min)
        .mag_filter(mag)
}

/// Each `(x, colour, what)` probe on row `y` reads its colour.
#[track_caller]
fn assert_probes(img: &RgbaImage, y: u32, probes: &[(u32, [u8; 4], &str)]) {
    for &(x, want, what) in probes {
        assert_px(
            img.get_pixel(x, y).0,
            want,
            SRGB_ROUND_TRIP,
            format_args!("{what} at ({x}, {y})"),
        );
    }
}

#[track_caller]
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

#[test]
fn image_updates_copy_pixels_and_repaint_every_clone() {
    let mut h = Harness::new();
    let mut image = Image::from_srgba8(UVec2::new(2, 2), [RED, BLUE, BLUE, RED].concat()).unwrap();
    let handle = h.host.ui().load_image(&image).unwrap();
    let clone = handle.clone();
    drop(handle);
    assert_eq!(h.host.gpu_image_cache_len(), 1);

    image.texels_mut().rotate_left(1);
    let nearest = filtered(
        &clone,
        ImageFit::Fill,
        ImageFilter::Nearest,
        ImageFilter::Nearest,
    );
    for expected in [[RED, BLUE, BLUE, RED], [BLUE, BLUE, RED, RED]] {
        let out = render(&mut h, UVec2::new(4, 2), |ui| {
            for x in [0.0, 2.0] {
                pane(ui, x, Vec2::splat(2.0), nearest.clone());
            }
        });
        for (x, y, pixel) in out.enumerate_pixels() {
            assert_px(
                pixel.0,
                expected[(y * 2 + x % 2) as usize],
                SRGB_ROUND_TRIP,
                format_args!("pixel ({x}, {y})"),
            );
        }
        clone.update(&image);
        image.texels_mut().fill(SrgbaU8::default());
        assert_eq!(h.host.gpu_image_cache_len(), 1);
    }
    drop(nearest);
    drop(clone);
    assert_eq!(h.host.gpu_image_cache_len(), 0);
}

/// Minification and magnification choose their own filters:
/// - Both: x=16 / x=112 sit in the texel-center clamp region: RED / BLUE (±2 sRGB).
/// - Nearest: hard seam, x=63 RED, x=64 BLUE (floor(uv · 2): 63.5/128·2 = 0.99 vs 64.5/128·2 = 1.01).
/// - Linear: x=64 is mid-ramp.
/// - Downscaling RED|BLUE|RED|BLUE 4px to 2px: nearest picks BLUE at each boundary, linear blends.
#[test]
fn minification_and_magnification_filters_are_independent() {
    use ImageFilter::{Linear, Nearest};
    let mut h = Harness::new();
    let two = load(&mut h, &[RED, BLUE]);
    let magnified = render(&mut h, UVec2::new(256, 64), |ui| {
        let size = Vec2::new(128.0, 64.0);
        pane(
            ui,
            0.0,
            size,
            filtered(&two, ImageFit::Fill, Nearest, Linear),
        );
        pane(
            ui,
            128.0,
            size,
            filtered(&two, ImageFit::Fill, Linear, Nearest),
        );
    });
    assert_probes(
        &magnified,
        32,
        &[
            (16, RED, "linear magnification, left half"),
            (112, BLUE, "linear magnification, right half"),
            (128 + 16, RED, "nearest magnification, left half"),
            (128 + 112, BLUE, "nearest magnification, right half"),
            (128 + 63, RED, "nearest seam-left"),
            (128 + 64, BLUE, "nearest seam-right"),
        ],
    );
    assert_blend(magnified.get_pixel(64, 32).0, "linear magnification seam");

    let four = load(&mut h, &[RED, BLUE, RED, BLUE]);
    let minified = render(&mut h, UVec2::new(4, 16), |ui| {
        let size = Vec2::new(2.0, 16.0);
        pane(
            ui,
            0.0,
            size,
            filtered(&four, ImageFit::Fill, Nearest, Linear),
        );
        pane(
            ui,
            2.0,
            size,
            filtered(&four, ImageFit::Fill, Linear, Nearest),
        );
    });
    assert_probes(
        &minified,
        8,
        &[
            (0, BLUE, "nearest minification"),
            (1, BLUE, "nearest minification"),
        ],
    );
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
    use ImageFilter::{Linear, Nearest};
    let mut h = Harness::new();
    let three = load(&mut h, &[RED, BLUE, RED]);
    let strips = render(&mut h, UVec2::new(200, 32), |ui| {
        let size = Vec2::new(100.0, 32.0);
        pane(
            ui,
            0.0,
            size,
            filtered(&three, ImageFit::Fill, Linear, Linear),
        );
        pane(
            ui,
            100.0,
            size,
            filtered(&three, ImageFit::Fill, Nearest, Nearest),
        );
    });
    assert_probes(
        &strips,
        16,
        &[
            (16, RED, "bilinear left clamp"),
            (83, RED, "bilinear right clamp"),
            (100 + 32, RED, "both-nearest first seam-left"),
            (100 + 33, BLUE, "both-nearest first seam-right"),
            (100 + 66, BLUE, "both-nearest second seam-left"),
            (100 + 67, RED, "both-nearest second seam-right"),
        ],
    );
    assert_blend(strips.get_pixel(32, 16).0, "bilinear seam");

    let two = load(&mut h, &[RED, BLUE]);
    let tile = ImageFit::Tile {
        offset: Vec2::ZERO,
        scale: Vec2::new(2.5, 1.0),
    };
    let tiled = render(&mut h, UVec2::new(200, 16), |ui| {
        let size = Vec2::new(100.0, 16.0);
        pane(ui, 0.0, size, filtered(&two, tile, Linear, Linear));
        pane(ui, 100.0, size, filtered(&two, tile, Nearest, Nearest));
    });
    let tpx = |x: u32| tiled.get_pixel(x, 8).0;
    // Bilinear over a repeat blends across every seam, edges included: at 20 px per texel, x = 0 and x = 39 read a blend, not the pure texel a clamp would give.
    assert_probes(
        &tiled,
        8,
        &[
            (
                0,
                [176, 94, 170, 255],
                "tiled bilinear opens on the seam blend",
            ),
            (
                39,
                [170, 97, 176, 255],
                "tiled bilinear closes on the seam blend",
            ),
            (40, tpx(0), "the next repeat opens the same way"),
            (79, tpx(39), "the next repeat closes the same way"),
            (100, RED, "tiled both-nearest tile start"),
            (100 + 19, RED, "tiled both-nearest intra-tile seam-left"),
            (100 + 20, BLUE, "tiled both-nearest intra-tile seam-right"),
            (100 + 39, BLUE, "tiled both-nearest tile end"),
            (100 + 40, RED, "tiled both-nearest wrap back"),
            (100 + 81, RED, "tiled both-nearest partial third repeat"),
        ],
    );
    assert_blend(tpx(20), "tiled bilinear intra-tile seam");
}

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
    let source = load(&mut h, &[[STAR, SKY, SKY]; 8].concat());
    let out = render(&mut h, UVec2::new(24, 16), |ui| {
        for (i, (mode, _, _)) in cases.iter().enumerate() {
            let image = Shape::image(source.clone())
                .fit(ImageFit::Fill)
                .downsample(*mode);
            pane(ui, i as f32 * PANE.x, PANE, image);
        }
    });

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
    let sources = cases.map(|(triple, _, _, _)| load(&mut h, &[triple; 8].concat()));
    let out = render(&mut h, UVec2::new(16, 16), |ui| {
        for (i, (_, mode, _, _)) in cases.iter().enumerate() {
            let image = Shape::image(sources[i].clone())
                .fit(ImageFit::Fill)
                .downsample(*mode);
            pane(ui, i as f32 * PANE.x, PANE, image);
        }
    });

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
    const PURE_RED: [u8; 4] = [255, 0, 0, 255];

    let mut h = Harness::new();
    let source = load(&mut h, &[PURE_RED, CLEAR]);
    let out = render(&mut h, UVec2::new(16, 16), |ui| {
        pane(
            ui,
            0.0,
            Vec2::splat(16.0),
            Shape::image(source.clone()).fit(ImageFit::Fill),
        );
    });

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
    let source = load(&mut h, &[STAR, SKY, SKY, SKY]);
    let out = render(&mut h, UVec2::new(8, 16), |ui| {
        let image = Shape::image(source.clone())
            .fit(ImageFit::Tile {
                offset: Vec2::ZERO,
                scale: Vec2::new(24.0, 1.0),
            })
            .downsample(ImageDownsample::Mean);
        pane(ui, 0.0, Vec2::new(8.0, 16.0), image);
    });

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
    let handles = SOURCES.map(|texel| load(&mut h, &[texel]));
    let out = render(&mut h, UVec2::new(192, 32), |ui| {
        for (pane_index, &source) in PATTERN.iter().enumerate() {
            let image = filtered(
                &handles[source],
                ImageFit::Fill,
                ImageFilter::Linear,
                ImageFilter::Linear,
            );
            pane(ui, pane_index as f32 * PANE, Vec2::splat(PANE), image);
        }
    });

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
