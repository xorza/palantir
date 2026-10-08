//! Pixel-level shadow fixtures.

use glam::{IVec2, UVec2, Vec2};
use palantir::golden::image::{Rgba, RgbaImage};
use palantir::widget::{ShadowShape, Shape};
use palantir::{
    Background, ClipMode, Configure, Corners, Panel, Rect, RgbaF32, Shadow, Sizing, Stroke, Ui,
};
use std::f64::consts::SQRT_2;

use crate::fixtures::canvas;
use crate::goldens::{assert_same, assert_same_in, crop};
use crate::harness::Harness;

const VIEWPORT: UVec2 = UVec2::new(220, 180);
const CLEAR: RgbaF32 = RgbaF32::WHITE;

/// The shadow colour the probes' arithmetic assumes: over white it leaves
/// `1 − 0.85·coverage` linear ([`under_ink`]).
const INK: RgbaF32 = RgbaF32::srgba(0.0, 0.0, 0.0, 0.85);

/// One frame over `clear`, with `scene` on a full-surface canvas.
fn render(clear: RgbaF32, mut scene: impl FnMut(&mut Ui)) -> RgbaImage {
    Harness::new()
        .size(VIEWPORT)
        .clear(clear)
        .frame(|ui| canvas(ui, &mut scene))
        .image
}

/// An [`INK`] shadow of `source` over white.
fn render_shadow(
    source: Rect,
    corners: impl Into<Corners>,
    offset: Vec2,
    blur: f32,
    spread: f32,
    inset: bool,
) -> RgbaImage {
    let shadow = Shadow {
        color: INK,
        offset,
        blur,
        spread,
        inset,
    };
    let corners = corners.into();
    render(CLEAR, |ui| {
        ui.add_shape(Shape::shadow(shadow).at(source).corners(corners));
    })
}

/// The sRGB byte of grey `lin` in linear light.
fn grey(lin: f64) -> u8 {
    let lin = lin as f32;
    RgbaF32::new(lin, lin, lin, 1.0).to_srgba_u8().r
}

/// The sRGB byte of white under [`INK`] at `coverage`.
fn under_ink(coverage: f64) -> u8 {
    grey(1.0 - 0.85 * coverage)
}

/// Each channel's distance between `a` and `b`.
fn channel_deltas<'a>(a: &'a RgbaImage, b: &'a RgbaImage) -> impl Iterator<Item = u8> + 'a {
    a.as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(&x, &y)| x.abs_diff(y))
}

/// `image` with every pixel in each of `holes` set to the clear colour, so
/// shadow shapes compare without their (clipped) sources.
fn without(image: &RgbaImage, holes: &[Rect]) -> RgbaImage {
    let mut out = image.clone();
    for hole in holes {
        for y in hole.min.y as u32..hole.max().y as u32 {
            for x in hole.min.x as u32..hole.max().x as u32 {
                out.put_pixel(x, y, Rgba([255; 4]));
            }
        }
    }
    out
}

#[test]
fn shifted_drop_bbox_preserves_positive_and_negative_offset_pixels() {
    let source = Rect::new(64.0, 60.0, 72.0, 54.0);

    for offset in [Vec2::new(17.0, 13.0), Vec2::new(-19.0, -11.0)] {
        let moved = Rect {
            min: source.min + offset,
            size: source.size,
        };
        let shifted = render_shadow(source, 11.0, offset, 6.0, 4.0, false);
        let reference = render_shadow(moved, 11.0, Vec2::ZERO, 6.0, 4.0, false);
        let holes = [source, moved];
        let name = format!("shadow_offset_{}_{}", offset.x, offset.y);
        assert_same(
            &name,
            &without(&shifted, &holes),
            &without(&reference, &holes),
        );
    }
}

#[test]
fn inset_offset_matches_translated_zero_offset_pixels_inside_source() {
    let source = Rect::new(40.0, 35.0, 120.0, 100.0);
    let pixel_offset = IVec2::new(9, -7);
    let shifted = render_shadow(source, 11.0, pixel_offset.as_vec2(), 6.0, 8.0, true);
    let reference = render_shadow(source, 11.0, Vec2::ZERO, 6.0, 8.0, true);

    let region = Rect::new(60.0, 50.0, 85.0, 70.0);
    let unshifted = Rect {
        min: region.min - pixel_offset.as_vec2(),
        size: region.size,
    };
    assert_same(
        "shadow_inset_translated",
        &crop(&shifted, region),
        &crop(&reference, unshifted),
    );
}

/// A spread is the same shadow as a source grown or shrunk by it, radii moved
/// by the CSS spread rule: 11 px corners at spread −4 become `max(11 − 4, 0)
/// = 7`; an inset hole grown by 4 rounds at `11 + 4 = 15`.
#[test]
fn negative_spread_deflates_drop_and_inset_shadow_geometry() {
    let blur = 6.0;
    let spread = -4.0;

    let drop_source = Rect::new(64.0, 60.0, 72.0, 54.0);
    let drop = render_shadow(drop_source, 11.0, Vec2::ZERO, blur, spread, false);
    let deflated_source = drop_source.inflated(spread);
    let drop_reference =
        render_shadow(deflated_source, 11.0 + spread, Vec2::ZERO, blur, 0.0, false);
    let holes = [drop_source];
    assert_same_in(
        "shadow_drop_negative_spread",
        &without(&drop, &holes),
        &without(&drop_reference, &holes),
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
    assert_same_in(
        "shadow_inset_negative_spread",
        &inset,
        &inset_reference,
        inset_source.inflated(-12.0),
    );
}

/// Drop shadows with no blur, so each edge is hard. The radius follows the
/// spread by the CSS rule, then fits the shadow box. Probes sit outside the
/// source, which clips its own shadow.
///
/// 40 px circle (corners 20) at (110, 90):
/// - spread +6: 52 px box, radius 26; the probe 27.6 px out on the diagonal is
///   past it (radius 20 would reach 6√2 + 20 = 28.5).
/// - spread −6, moved 45 px down: 28 px box at (110, 135), radius 14; the
///   probe 13.5 px straight down is inside.
///
/// A sharp 40 px box with spread +6 stays sharp: it fills (84.5, 64.5).
///
/// A 60×40 box at (80, 70) with only its top-left corner rounded, at 60: the
/// radius fits the box first (40), then spreads to 46 on the 72×52 shadow box
/// from (74, 64), centred at (120, 110). (88.5, 78.5) is 44.5 px out, inside;
/// (86.5, 76.5) is 47.4 px, past. Spread first gives 66, fit to 52, centred
/// at (126, 116), putting the first probe 53.0 px out.
#[test]
fn a_spread_moves_the_shadow_radius_by_the_css_rule() {
    let circle = Rect::new(90.0, 70.0, 40.0, 40.0);
    // 85 % black over white leaves 0.15 linear, which encodes to sRGB 107.
    let dark = |img: &RgbaImage, x: u32, y: u32| img.get_pixel(x, y).0[0] < 120;
    let clear = |img: &RgbaImage, x: u32, y: u32| img.get_pixel(x, y).0[0] > 245;

    let grown = render_shadow(circle, 20.0, Vec2::ZERO, 0.0, 6.0, false);
    assert!(dark(&grown, 127, 107), "24.7 px out on the diagonal");
    assert!(clear(&grown, 129, 109), "27.6 px out on the diagonal");

    let shrunk = render_shadow(circle, 20.0, Vec2::new(0.0, 45.0), 0.0, -6.0, false);
    assert!(dark(&shrunk, 110, 148), "13.5 px below the centre");
    assert!(clear(&shrunk, 110, 150), "15.5 px below the centre");
    assert!(clear(&shrunk, 120, 145), "14.1 px out on the diagonal");

    let sharp = render_shadow(circle, 0.0, Vec2::ZERO, 0.0, 6.0, false);
    assert!(dark(&sharp, 84, 64), "the sharp shadow's corner pixel");

    let overlapping = render_shadow(
        Rect::new(80.0, 70.0, 60.0, 40.0),
        Corners::new(60.0, 0.0, 0.0, 0.0),
        Vec2::ZERO,
        0.0,
        6.0,
        false,
    );
    assert!(
        dark(&overlapping, 88, 78),
        "44.5 px from the fitted arc's centre"
    );
    assert!(
        clear(&overlapping, 86, 76),
        "47.4 px from the fitted arc's centre"
    );
}

/// An inset shadow on chrome paints over its opaque fill and inside its
/// border, as CSS `box-shadow: inset`: the same pixels as the shadow pushed
/// as a shape after chrome without it, on the 140×100 box less the 4 px
/// border, at radii `16 − 4 = 12`.
///
/// The probe 0.5 px inside the padding edge sees coverage Φ(0.5 / 4) = 0.55,
/// so 0.45 of the 0.6-alpha black lands: 0.787 linear × 0.73 = 0.575, sRGB 200.
#[test]
fn inset_chrome_shadow_paints_over_the_fill_inside_the_border() {
    let shadow = Shadow {
        color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.6),
        offset: Vec2::ZERO,
        blur: 4.0,
        spread: 0.0,
        inset: true,
    };
    let boxed = |on_chrome: bool| {
        render(CLEAR, |ui| {
            Panel::zstack()
                .id_salt("chrome")
                .position((40.0, 40.0))
                .size((Sizing::fixed(140.0), Sizing::fixed(100.0)))
                .background(
                    Background::rounded(RgbaF32::srgb(0.9, 0.9, 0.9), Corners::all(16.0))
                        .with_border(Stroke::new(RgbaF32::srgb(0.1, 0.2, 0.6), 4.0))
                        .with_shadow(if on_chrome { shadow } else { Shadow::NONE }),
                )
                .show(ui, |ui| {
                    if !on_chrome {
                        ui.add_shape(
                            Shape::shadow(shadow)
                                .at(Rect::new(4.0, 4.0, 132.0, 92.0))
                                .corners(12.0),
                        );
                    }
                });
        })
    };
    let chrome = boxed(true);
    assert_same("shadow_inset_chrome", &chrome, &boxed(false));
    let red = |x: u32, y: u32| chrome.get_pixel(x, y).0[0];
    assert!(red(110, 90) >= 228, "the middle is the bare fill");
    assert!(red(44, 90) < 215, "the padding edge is in shadow");
}

/// A drop shadow is clipped inside its casting box, as CSS clips an outer
/// `box-shadow`: under a 40 % fill, pixels 1 px or more inside the box equal
/// the fill alone. Bands stop a corner radius short of the ends. Below the
/// box the shadow still darkens.
#[test]
fn a_drop_shadow_is_clipped_inside_a_translucent_box() {
    let source = Rect::new(40.0, 40.0, 140.0, 100.0);
    let radius = 12.0;
    let fill = RgbaF32::srgba(0.2, 0.4, 0.9, 0.4);
    let shadow = Shadow::drop(RgbaF32::srgba(0.0, 0.0, 0.0, 0.6), Vec2::new(0.0, 6.0), 8.0);
    let cast = |shadow: Shadow, chrome: bool| {
        render(CLEAR, |ui| {
            if chrome {
                Panel::zstack()
                    .id_salt("box")
                    .position(source.min)
                    .size((Sizing::fixed(source.size.w), Sizing::fixed(source.size.h)))
                    .background(Background::rounded(fill, Corners::all(radius)).with_shadow(shadow))
                    .show(ui, |_| {});
            } else {
                ui.add_shape(Shape::shadow(shadow).at(source).corners(radius));
                ui.add_shape(Shape::rect(source).fill(fill).corners(radius));
            }
        })
    };
    let bands = [
        Rect::new(41.0, 52.0, 138.0, 76.0),
        Rect::new(52.0, 41.0, 116.0, 98.0),
    ];
    for (label, chrome) in [("chrome", true), ("shape", false)] {
        let shadowed = cast(shadow, chrome);
        let bare = cast(Shadow::NONE, chrome);
        for (i, band) in bands.into_iter().enumerate() {
            let name = format!("shadow_clip_{label}_{i}");
            assert_same_in(&name, &shadowed, &bare, band);
        }
        let below = |img: &RgbaImage| img.get_pixel(110, 145).0[0];
        assert!(
            below(&shadowed) < below(&bare) - 20,
            "{label}: the shadow shows below the box",
        );
    }
}

/// `erf` to 1.5e-7 (Abramowitz & Stegun 7.1.26), in f64.
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = ((((1.061_405_429 * t - 1.453_152_027) * t + 1.421_413_741) * t - 0.284_496_736)
        * t
        + 0.254_829_592)
        * t;
    (1.0 - poly * (-x * x).exp()).copysign(x)
}

/// The Gaussian CDF averaged across a pixel's 1 px box, by 64 midpoint
/// samples.
fn pixel_cdf(u: f64, sigma: f64) -> f64 {
    const SAMPLES: u32 = 64;
    (0..SAMPLES)
        .map(|i| {
            let t = (f64::from(i) + 0.5) / f64::from(SAMPLES) - 0.5;
            0.5 + 0.5 * erf((u + t) / (sigma * SQRT_2))
        })
        .sum::<f64>()
        / f64::from(SAMPLES)
}

/// Coverage at the pixel centred on `p` of `rect` with every corner rounded
/// by `radius`, blurred by σ. Summed in 1/2000-of-height rows through
/// `pixel_cdf`, weighted by the kernel's share on each row.
fn reference_coverage(p: Vec2, rect: Rect, radius: f32, sigma: f64) -> f64 {
    const ROWS: u32 = 2000;
    let (px, py) = (f64::from(p.x), f64::from(p.y));
    let (x0, y0) = (f64::from(rect.min.x), f64::from(rect.min.y));
    let (w, h, r) = (
        f64::from(rect.size.w),
        f64::from(rect.size.h),
        f64::from(radius),
    );
    let row = h / f64::from(ROWS);
    (0..ROWS)
        .map(|i| {
            let top = y0 + f64::from(i) * row;
            let mid = top + 0.5 * row;
            let from_edge = (mid - y0).min(y0 + h - mid);
            let inset = if from_edge < r {
                r - (r * r - (r - from_edge).powi(2)).sqrt()
            } else {
                0.0
            };
            let span = pixel_cdf(x0 + w - inset - px, sigma) - pixel_cdf(x0 + inset - px, sigma);
            let weight = pixel_cdf(top + row - py, sigma) - pixel_cdf(top - py, sigma);
            span * weight
        })
        .sum()
}

/// A blurred shadow is its box convolved with the Gaussian, checked per pixel
/// against the reference integral. 85 % black over white leaves
/// `1 − 0.85·coverage` linear. Tolerance is two 8-bit steps: one for target
/// rounding, one for the shader's corner slices (within 0.002 linear).
///
/// Probes where the box blur and an erf of its distance field part ways:
/// - outside a sharp corner (product of two edge falloffs, 0.25 on the
///   diagonal at the edge);
/// - outside a rounded one;
/// - in a 4 px box under σ = 8, spread to under 4 %;
/// - in a drop shadow that spread −8 shrinks to nothing;
/// - in an inset shadow whose hole spread 25 closes, shadow throughout.
#[test]
fn a_blurred_shadow_is_the_box_convolved_with_the_gaussian() {
    let probe = |img: &RgbaImage, x: u32, y: u32, want: f64, what: &str| {
        let got = img.get_pixel(x, y).0[0];
        let want = under_ink(want);
        assert!(
            got.abs_diff(want) <= 2,
            "{what} at ({x}, {y}): got {got}, want {want}"
        );
    };
    let centre = |x: u32, y: u32| Vec2::new(x as f32 + 0.5, y as f32 + 0.5);

    let sharp = Rect::new(40.0, 40.0, 60.0, 40.0);
    let img = render_shadow(sharp, 0.0, Vec2::ZERO, 6.0, 0.0, false);
    for (x, y) in [(39, 39), (33, 33), (30, 30), (70, 37), (36, 60)] {
        let want = reference_coverage(centre(x, y), sharp, 0.0, 6.0);
        probe(&img, x, y, want, "sharp box");
    }
    assert!(
        (reference_coverage(Vec2::new(40.0, 40.0), sharp, 0.0, 6.0) - 0.25).abs() < 1e-3,
        "the reference puts a quarter of the kernel on the corner",
    );

    let img = render_shadow(sharp, 12.0, Vec2::ZERO, 6.0, 0.0, false);
    for (x, y) in [(41, 41), (37, 37), (33, 33), (44, 38), (38, 44)] {
        let want = reference_coverage(centre(x, y), sharp, 12.0, 6.0);
        probe(&img, x, y, want, "rounded box");
    }

    let small = Rect::new(150.0, 20.0, 4.0, 4.0);
    let moved = Rect::new(150.0, 80.0, 4.0, 4.0);
    let img = render_shadow(small, 0.0, Vec2::new(0.0, 60.0), 8.0, 0.0, false);
    for (x, y) in [(151, 81), (152, 82), (158, 82), (152, 90)] {
        let want = reference_coverage(centre(x, y), moved, 0.0, 8.0);
        assert!(want < 0.04, "the reference spreads the box thin");
        probe(&img, x, y, want, "small box");
    }

    let collapsed = Rect::new(150.0, 120.0, 10.0, 10.0);
    let img = render_shadow(collapsed, 2.0, Vec2::new(0.0, 30.0), 4.0, -8.0, false);
    for (x, y) in [(154, 154), (155, 155), (150, 150)] {
        probe(&img, x, y, 0.0, "collapsed drop shadow");
    }

    let closed = Rect::new(20.0, 110.0, 40.0, 40.0);
    let img = render_shadow(closed, 6.0, Vec2::ZERO, 4.0, 25.0, true);
    for (x, y) in [(39, 129), (40, 130), (30, 140)] {
        probe(&img, x, y, 1.0, "closed inset hole");
    }
}

/// An inset shadow takes its source's own edge ramp: with its hole closed by
/// the spread it matches a fill of its colour pixel for pixel, partly covered
/// corner pixels included.
#[test]
fn an_inset_shadow_shares_its_source_edge_ramp() {
    let source = Rect::new(40.5, 40.25, 100.0, 80.0);
    let filled = render(CLEAR, |ui| {
        ui.add_shape(Shape::rect(source).fill(INK).corners(14.0));
    });
    assert_same(
        "shadow_inset_edge",
        &render_shadow(source, 14.0, Vec2::ZERO, 3.0, 60.0, true),
        &filled,
    );
}

/// A drop shadow's quad holds every pixel its coverage reaches.
///
/// - A sharp shadow moved by (0.75, 50) from the source at x = 40 has edges at
///   40.75 and 100.75, so columns 40 and 100 are a quarter and three quarters
///   covered. 85 % black over white leaves `1 − 0.85·coverage` linear.
/// - A white glow over black, σ = 8, reaches its quad's edge at 4σ = 32 px
///   past the box (x = 192), under half an 8-bit step; at 3σ (x = 184) it is
///   about 4 steps up. One step near black is under 0.0002 linear, so probes
///   allow one.
#[test]
fn a_drop_shadow_quad_holds_its_edge_ramp_and_its_tail() {
    let img = render_shadow(
        Rect::new(40.0, 20.0, 60.0, 40.0),
        0.0,
        Vec2::new(0.75, 50.0),
        0.0,
        0.0,
        false,
    );
    for (x, coverage) in [(39, 0.0), (40, 0.25), (41, 1.0), (100, 0.75), (101, 0.0)] {
        let got = img.get_pixel(x, 90).0[0];
        let want = under_ink(coverage);
        assert!(
            got.abs_diff(want) <= 1,
            "sharp shadow at ({x}, 90) is covered {coverage}: got {got}, want {want}",
        );
    }

    let source = Rect::new(60.0, 40.0, 100.0, 100.0);
    let img = render(RgbaF32::BLACK, |ui| {
        let glow = Shadow::drop(RgbaF32::WHITE, Vec2::ZERO, 8.0);
        ui.add_shape(Shape::shadow(glow).at(source));
    });
    assert!(
        grey(reference_coverage(Vec2::new(184.5, 90.5), source, 0.0, 8.0)) >= 3,
        "the tail past 3σ shows",
    );
    for x in 176..196 {
        let got = img.get_pixel(x, 90).0[0];
        let want = grey(reference_coverage(
            Vec2::new(x as f32 + 0.5, 90.5),
            source,
            0.0,
            8.0,
        ));
        assert!(
            got.abs_diff(want) <= 1,
            "glow at ({x}, 90): got {got}, want {want}",
        );
    }
}

/// A grid of shadows over the blurs and radii the corner cutout tables cover:
/// drop shadows in the top four rows, inset below. Under it, at σ = 16, two
/// shadows cross the viewport's edge. On-screen corners round 26 (the grid's
/// inset key); off-screen ones to keys nothing else uses: 12 (drop; right
/// corners' regions start at `905 − 12 − 64.5 = 828.5`, past x = 800) and 4
/// (inset; left corners' end at `−193 + 4 + 64.5 = −124.5`). Those get no
/// table, so `fs_shadow_tables` draws them with two corners it cuts nothing at.
fn cutout_grid(ui: &mut Ui) {
    const BLURS: [f32; 4] = [0.5, 2.0, 6.0, 16.0];
    const RADII: [f32; 4] = [0.0, 4.0, 12.0, 30.0];
    const CELL: f32 = 200.0;
    canvas(ui, |ui| {
        for (row, inset) in [false, true].into_iter().enumerate() {
            for (y, blur) in BLURS.into_iter().enumerate() {
                for (x, radius) in RADII.into_iter().enumerate() {
                    let at = Vec2::new(x as f32, (row * BLURS.len() + y) as f32) * CELL;
                    ui.add_shape(
                        Shape::shadow(Shadow {
                            color: INK,
                            offset: Vec2::new(3.0, 5.0),
                            blur,
                            spread: if inset { 4.0 } else { 2.0 },
                            inset,
                        })
                        .at(Rect::new(at.x + 70.0, at.y + 70.0, 60.0, 60.0))
                        .corners(radius),
                    );
                }
            }
        }
        for (inset, source, corners) in [
            (
                false,
                Rect::new(600.0, 1680.0, 300.0, 60.0),
                Corners::new(24.0, 10.0, 10.0, 24.0),
            ),
            (
                true,
                Rect::new(-200.0, 1640.0, 300.0, 160.0),
                Corners::new(8.0, 30.0, 30.0, 8.0),
            ),
        ] {
            ui.add_shape(
                Shape::shadow(Shadow {
                    color: INK,
                    offset: Vec2::new(3.0, 5.0),
                    blur: 16.0,
                    spread: if inset { 4.0 } else { 2.0 },
                    inset,
                })
                .at(source)
                .corners(corners),
            );
        }
    });
}

/// Baked cutout tables match the shaded cutout to one 8-bit level per
/// channel (tables err under 6e-4 coverage, the shaded form 0.0011). Some
/// pixel does round apart, proving the tables are in use. Likewise for the
/// two shadows past the viewport's edge.
#[test]
fn baked_cutout_tables_match_the_shaded_cutout() {
    let size = UVec2::new(800, 1840);
    let mut baked = Harness::new();
    let mut shaded = Harness::new();
    shaded.host.disable_cutout_tables();
    let [baked, shaded] = [&mut baked, &mut shaded]
        .map(|harness| harness.size(size).clear(CLEAR).frame(cutout_grid).image);
    let (most, differing) = channel_deltas(&baked, &shaded).fold((0, 0), |(most, differing), d| {
        (most.max(d), differing + usize::from(d != 0))
    });
    assert!(most <= 1, "a channel moved {most} levels");
    assert!(differing > 0, "no pixel read a table");
}

/// One shadow of the grid sweep.
#[derive(Clone, Copy, Debug)]
struct GridCase {
    inset: bool,
    blur: f32,
    corners: f32,
    size: Vec2,
    offset: Vec2,
    spread: f32,
}

/// A shadow cell of the sweep, in logical px: wide enough for the largest
/// blur's reach (`4σ`, 72 px at σ = 18) on both sides.
const GRID_CELL: f32 = 360.0;
const GRID_COLUMNS: usize = 6;

/// Cases the grid's cells must agree with the full form on: drop and inset
/// over σ = 0, 0.2 (below `CUTOUT_MIN_SIGMA`), 2 and 18, radius 0, 4 and 30;
/// then boxes whose corner cells meet or overlap (`2·(r + reach)` is 25 px at
/// σ = 2, r = 4), offsets past `reach`, spreads either way, and radii larger
/// than their box.
fn grid_cases() -> Vec<GridCase> {
    let mut cases = Vec::new();
    for inset in [false, true] {
        for blur in [0.0, 0.2, 2.0, 18.0] {
            for corners in [0.0, 4.0, 30.0] {
                cases.push(GridCase {
                    inset,
                    blur,
                    corners,
                    size: Vec2::new(120.0, 90.0),
                    offset: Vec2::new(3.0, 5.0),
                    spread: if inset { 4.0 } else { 2.0 },
                });
            }
        }
    }
    let base = GridCase {
        inset: false,
        blur: 2.0,
        corners: 4.0,
        size: Vec2::new(25.0, 25.0),
        offset: Vec2::ZERO,
        spread: 0.0,
    };
    for inset in [false, true] {
        cases.extend([
            GridCase { inset, ..base },
            GridCase {
                inset,
                size: Vec2::new(24.0, 26.0),
                ..base
            },
            GridCase {
                inset,
                blur: 18.0,
                size: Vec2::new(10.0, 8.0),
                ..base
            },
            GridCase {
                inset,
                size: Vec2::new(100.0, 70.0),
                offset: Vec2::new(30.0, -20.0),
                ..base
            },
            GridCase {
                inset,
                size: Vec2::new(100.0, 70.0),
                spread: -4.0,
                ..base
            },
            GridCase {
                inset,
                size: Vec2::new(100.0, 70.0),
                spread: 6.0,
                corners: 12.0,
                ..base
            },
            GridCase {
                inset,
                blur: 8.0,
                size: Vec2::new(40.0, 40.0),
                corners: 30.0,
                ..base
            },
        ]);
    }
    cases
}

fn grid_case_rect(i: usize, case: GridCase) -> Rect {
    let cell = Vec2::new((i % GRID_COLUMNS) as f32, (i / GRID_COLUMNS) as f32) * GRID_CELL;
    let min = cell + (Vec2::splat(GRID_CELL) - case.size) * 0.5;
    Rect::new(min.x, min.y, case.size.x, case.size.y)
}

fn grid_case_shape(case: GridCase, at: Rect) -> ShadowShape {
    Shape::shadow(Shadow {
        color: RgbaF32::srgba(0.1, 0.0, 0.3, 0.85),
        offset: case.offset,
        blur: case.blur,
        spread: case.spread,
        inset: case.inset,
    })
    .at(at)
    .corners(case.corners)
}

/// The sweep, then two shadows seen through clips: a scissor across one's
/// edge cells, a rounded (stencil) clip over another.
fn grid_sweep(ui: &mut Ui) {
    let cases = grid_cases();
    canvas(ui, |ui| {
        for (i, &case) in cases.iter().enumerate() {
            ui.add_shape(grid_case_shape(case, grid_case_rect(i, case)));
        }
        let clipped = GridCase {
            inset: false,
            blur: 18.0,
            corners: 30.0,
            size: Vec2::new(200.0, 120.0),
            offset: Vec2::new(6.0, 10.0),
            spread: 2.0,
        };
        let row = cases.len().div_ceil(GRID_COLUMNS) as f32 * GRID_CELL;
        for (x, rounded) in [(0.0, false), (GRID_CELL, true)] {
            Panel::canvas()
                .id_salt(("clip", rounded))
                .position((x + 40.0, row + 40.0))
                .size((Sizing::fixed(170.0), Sizing::fixed(150.0)))
                .background(Background::rounded(
                    RgbaF32::TRANSPARENT,
                    Corners::all(if rounded { 40.0 } else { 0.0 }),
                ))
                .clip(if rounded {
                    ClipMode::Rounded
                } else {
                    ClipMode::Rect
                })
                .show(ui, |ui| {
                    ui.add_shape(grid_case_shape(
                        clipped,
                        Rect::new(60.0, 50.0, clipped.size.x, clipped.size.y),
                    ));
                });
        }
    });
}

/// A shadow drawn as its grid of cells equals the one-cell full form to one
/// 8-bit level per channel: cells treat the Gaussian past `reach` as zero (at
/// most 3.2e-5 coverage). A pixel shaded by two cells would blend twice and
/// move the purple past a level, so the tolerance proves each is shaded once.
#[test]
fn a_shadow_grid_matches_the_full_form() {
    let cases = grid_cases();
    let rows = cases.len().div_ceil(GRID_COLUMNS) + 1;
    let size = UVec2::new(
        (GRID_COLUMNS as f32 * GRID_CELL) as u32,
        (rows as f32 * GRID_CELL) as u32,
    );
    let mut grid = Harness::new();
    let mut full = Harness::new();
    full.host.disable_shadow_grid();
    let [grid, full] = [&mut grid, &mut full]
        .map(|harness| harness.size(size).clear(CLEAR).frame(grid_sweep).image);
    let most = channel_deltas(&grid, &full).max().unwrap();
    assert!(most <= 1, "a channel moved {most} levels");
    for (i, case) in cases.iter().enumerate().filter(|(_, case)| !case.inset) {
        let source = grid_case_rect(i, *case);
        let inset = case.corners + 2.0;
        let (x0, y0) = ((source.min.x + inset) as u32, (source.min.y + inset) as u32);
        let (x1, y1) = (
            (source.min.x + source.size.w - inset) as u32,
            (source.min.y + source.size.h - inset) as u32,
        );
        for y in y0..y1 {
            for x in x0..x1 {
                assert_eq!(
                    grid.get_pixel(x, y),
                    full.get_pixel(x, y),
                    "case {i} {case:?}: pixel ({x}, {y}) inside the source",
                );
            }
        }
    }
}
