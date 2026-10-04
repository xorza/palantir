//! Pixel-level shadow fixtures.

use glam::{IVec2, UVec2, Vec2};
use palantir::golden::image::{Rgba, RgbaImage};
use palantir::widget::Shape;
use palantir::{Background, Configure, Corners, Panel, Rect, RgbaF32, Shadow, Sizing, Stroke};
use std::f64::consts::SQRT_2;

use crate::goldens::{assert_same, assert_same_in, crop};
use crate::harness::Harness;

const VIEWPORT: UVec2 = UVec2::new(220, 180);
const CLEAR: RgbaF32 = RgbaF32::WHITE;

fn render_shadow(
    source: Rect,
    corners: f32,
    offset: Vec2,
    blur: f32,
    spread: f32,
    inset: bool,
) -> RgbaImage {
    let mut harness = Harness::new();
    harness
        .size(VIEWPORT)
        .clear(CLEAR)
        .frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::shadow(Shadow {
                            color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.85),
                            offset,
                            blur,
                            spread,
                            inset,
                        })
                        .at(source)
                        .corners(corners),
                    );
                });
        })
        .image
}

/// `image` with every pixel in each of `holes` set to the clear colour. A
/// drop shadow is clipped inside its source, so two renders whose sources
/// differ also differ there; a comparison about the shadow's shape takes
/// both sources out of both images.
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
        let moved = Rect {
            min: source.min + offset,
            size: source.size,
        };
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

    // The shifted shadow at a region, against the unshifted one at the
    // same region moved back by the offset.
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

/// A spread is the same shadow as a source grown or shrunk by it, with
/// its radii moved by the CSS spread rule: a drop shadow's 11 px corners
/// at spread −4 become `max(11 − 4, 0) = 7`, and an inset hole grown by 4
/// rounds at `11 + 4 = 15`, since 11 ≥ 4.
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

/// Drop shadows with no blur, so each edge is a hard one a probe can
/// read: the radius grows or shrinks with the spread by the CSS rule,
/// then fits the shadow box. Every probe sits outside the source, which
/// clips its own shadow.
///
/// A 40 px circle (corners 20) centred at (110, 90):
/// - spread +6 → a 52 px box with radius 26, a circle. The probe 27.6 px
///   out along the diagonal is past it; with the radius left at 20 the
///   corner arc reached 6√2 + 20 = 28.5 there.
/// - spread −6, moved 45 px down clear of the source → a 28 px box centred
///   at (110, 135) with radius `max(20 − 6, 0) = 14`, a circle. The probe
///   13.5 px straight down is inside it; with the radius left at 20 the
///   shape was a squircle 13.6 px from the centre on that axis.
///
/// A sharp 40 px box with spread +6 stays sharp: its 52 px shadow fills
/// the corner pixel at (84.5, 64.5).
///
/// A 60×40 box at (80, 70) with only its top-left corner rounded, at 60:
/// the radius fits the box first, to 40 on its 40 px left side, then
/// spreads by 6 to 46 on the 72×52 shadow box from (74, 64), centred at
/// (120, 110). The pixel at (88.5, 78.5) is 44.5 px from that centre,
/// inside the arc; (86.5, 76.5) is 47.4 px out, past it. Spread first,
/// 66 fits the 52 px side to 52, centred at (126, 116), and the first
/// probe falls 53.0 px out, past that arc. Both probes are more than
/// 40 px from the source's own arc at (120, 110), so outside its clip.
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

    let mut harness = Harness::new();
    let overlapping = harness
        .size(VIEWPORT)
        .clear(CLEAR)
        .frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::shadow(Shadow {
                            color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.85),
                            offset: Vec2::ZERO,
                            blur: 0.0,
                            spread: 6.0,
                            inset: false,
                        })
                        .at(Rect::new(80.0, 70.0, 60.0, 40.0))
                        .corners(Corners::new(60.0, 0.0, 0.0, 0.0)),
                    );
                });
        })
        .image;
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
/// border, as CSS paints `box-shadow: inset`: the same pixels as the
/// shadow left off the chrome and pushed as a shape after it, on the
/// 140×100 box less the 4 px border, at radii `16 − 4 = 12`.
///
/// The probe 0.5 px inside the padding edge reads the shadow. The hole's
/// blurred coverage there is about Φ(0.5 / 4) = 0.55, so 0.45 of the
/// 0.6-alpha black lands: the 0.9 sRGB fill, 0.787 linear, darkens to
/// 0.787 × 0.73 = 0.575, sRGB 200. Under the fill it read the fill, 230.
#[test]
fn inset_chrome_shadow_paints_over_the_fill_inside_the_border() {
    let shadow = Shadow {
        color: RgbaF32::srgba(0.0, 0.0, 0.0, 0.6),
        offset: Vec2::ZERO,
        blur: 4.0,
        spread: 0.0,
        inset: true,
    };
    let render = |on_chrome: bool| {
        let mut harness = Harness::new();
        harness
            .size(VIEWPORT)
            .clear(CLEAR)
            .frame(|ui| {
                Panel::canvas()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::zstack()
                            .id_salt("chrome")
                            .position((40.0, 40.0))
                            .size((Sizing::fixed(140.0), Sizing::fixed(100.0)))
                            .background(Background {
                                fill: RgbaF32::srgb(0.9, 0.9, 0.9).into(),
                                border: Stroke::new(RgbaF32::srgb(0.1, 0.2, 0.6), 4.0),
                                corners: 16.0.into(),
                                shadow: if on_chrome { shadow } else { Shadow::NONE },
                            })
                            .show(ui, |ui| {
                                if !on_chrome {
                                    ui.add_shape(
                                        Shape::shadow(shadow)
                                            .at(Rect::new(4.0, 4.0, 132.0, 92.0))
                                            .corners(12.0),
                                    );
                                }
                            });
                    });
            })
            .image
    };
    let chrome = render(true);
    assert_same("shadow_inset_chrome", &chrome, &render(false));
    let red = |x: u32, y: u32| chrome.get_pixel(x, y).0[0];
    assert!(red(110, 90) >= 228, "the middle is the bare fill");
    assert!(red(44, 90) < 215, "the padding edge is in shadow");
}

/// A drop shadow is clipped inside the box that casts it, as CSS clips an
/// outer `box-shadow` inside the border box: under a 40 % fill, chrome or
/// shape, every pixel 1 px or more inside the box is the fill over the
/// clear colour alone, the same as the box drawn with no shadow. Bands
/// stop the corner radius short of the ends, so they hold only pixels
/// inside the rounded box. Below the box the shadow still darkens.
#[test]
fn a_drop_shadow_is_clipped_inside_a_translucent_box() {
    let source = Rect::new(40.0, 40.0, 140.0, 100.0);
    let radius = 12.0;
    let fill = RgbaF32::srgba(0.2, 0.4, 0.9, 0.4);
    let shadow = Shadow::drop(RgbaF32::srgba(0.0, 0.0, 0.0, 0.6), Vec2::new(0.0, 6.0), 8.0);
    let render = |shadow: Shadow, chrome: bool| {
        let mut harness = Harness::new();
        harness
            .size(VIEWPORT)
            .clear(CLEAR)
            .frame(|ui| {
                Panel::canvas()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        if chrome {
                            Panel::zstack()
                                .id_salt("box")
                                .position(source.min)
                                .size((Sizing::fixed(source.size.w), Sizing::fixed(source.size.h)))
                                .background(Background {
                                    fill: fill.into(),
                                    border: Stroke::NONE,
                                    corners: radius.into(),
                                    shadow,
                                })
                                .show(ui, |_| {});
                        } else {
                            ui.add_shape(Shape::shadow(shadow).at(source).corners(radius));
                            ui.add_shape(Shape::rect(source).fill(fill).corners(radius));
                        }
                    });
            })
            .image
    };
    let bands = [
        Rect::new(41.0, 52.0, 138.0, 76.0),
        Rect::new(52.0, 41.0, 116.0, 98.0),
    ];
    for (label, chrome) in [("chrome", true), ("shape", false)] {
        let shadowed = render(shadow, chrome);
        let bare = render(Shadow::NONE, chrome);
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

/// `erf` to 1.5e-7 (Abramowitz & Stegun 7.1.26), in f64 for the reference.
fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = ((((1.061_405_429 * t - 1.453_152_027) * t + 1.421_413_741) * t - 0.284_496_736)
        * t
        + 0.254_829_592)
        * t;
    (1.0 - poly * (-x * x).exp()).copysign(x)
}

/// What one pixel sees of a blurred half-line below `u`: the Gaussian's
/// CDF averaged across the pixel's 1 px box, by 64 midpoint samples.
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

/// The coverage, at the pixel centred on `p`, of the box `rect` with
/// every corner rounded by `radius`, blurred by σ — summed row by row the
/// long way: each 1/2000 of the box's height is a span whose two ends
/// the pixel sees through `pixel_cdf`, weighted by how much of the
/// kernel falls on that row.
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

/// A blurred shadow is its box convolved with the Gaussian, pixel by
/// pixel, against the reference integral above. 85 % black over white
/// leaves `1 − 0.85·coverage` linear. Two 8-bit steps: one for the
/// target's rounding, one for the shader's corner slices, which stay
/// within 0.002 linear of the integral — at most 0.7 of a step on the
/// darkest of these probes.
///
/// The probes are where the blur of the box and an erf of its distance
/// field part ways:
/// - outside a sharp corner, where the blur is the product of the two
///   edges' falloffs, 0.25 on the corner's own diagonal at the edge;
/// - outside a rounded one;
/// - in a 4 px box under σ = 8, which the blur spreads to under 4 %;
/// - in a drop shadow that spread −8 shrinks to nothing, which paints
///   nothing;
/// - in an inset shadow whose hole spread 25 closes, which is shadow
///   throughout.
#[test]
fn a_blurred_shadow_is_the_box_convolved_with_the_gaussian() {
    let encode = |coverage: f64| {
        let lin = (1.0 - 0.85 * coverage) as f32;
        RgbaF32::new(lin, lin, lin, 1.0).to_srgba_u8().r
    };
    let probe = |img: &RgbaImage, x: u32, y: u32, want: f64, what: &str| {
        let got = img.get_pixel(x, y).0[0];
        let want = encode(want);
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

/// An inset shadow takes its source's own edge ramp. With its hole closed
/// by the spread, it covers the source exactly as a fill of its colour
/// does, so the two renders match pixel for pixel, the rounded corners'
/// partly covered pixels included.
#[test]
fn an_inset_shadow_shares_its_source_edge_ramp() {
    let source = Rect::new(40.5, 40.25, 100.0, 80.0);
    let color = RgbaF32::srgba(0.0, 0.0, 0.0, 0.85);
    let render = |shadow: bool| {
        let mut harness = Harness::new();
        harness
            .size(VIEWPORT)
            .clear(CLEAR)
            .frame(|ui| {
                Panel::canvas()
                    .auto_id()
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        if shadow {
                            ui.add_shape(
                                Shape::shadow(Shadow {
                                    color,
                                    offset: Vec2::ZERO,
                                    blur: 3.0,
                                    spread: 60.0,
                                    inset: true,
                                })
                                .at(source)
                                .corners(14.0),
                            );
                        } else {
                            ui.add_shape(Shape::rect(source).fill(color).corners(14.0));
                        }
                    });
            })
            .image
    };
    assert_same("shadow_inset_edge", &render(true), &render(false));
}

/// A drop shadow's quad holds every pixel its coverage reaches: the
/// antialiasing ramp past a sharp edge, and the blurred tail out to where
/// it no longer shows.
///
/// - A sharp shadow moved by (0.75, 50) from the source at x = 40 has its
///   left edge at 40.75, a quarter pixel into column 40, and its right
///   edge at 100.75, three quarters into column 100. 85 % black over
///   white leaves `1 − 0.85·coverage` linear.
/// - A white glow over black, σ = 8, is the box's convolution with the
///   Gaussian out to its quad's edge, 4σ = 32 px past the box at x = 192,
///   where it is under half an 8-bit step. At 3σ, x = 184, the tail is
///   still about 4 steps up, and the reference holds that too. Near black
///   one step is under 0.0002 linear, so the probes allow one step.
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
    let encode = |coverage: f64| {
        let lin = (1.0 - 0.85 * coverage) as f32;
        RgbaF32::new(lin, lin, lin, 1.0).to_srgba_u8().r
    };
    for (x, coverage) in [(39, 0.0), (40, 0.25), (41, 1.0), (100, 0.75), (101, 0.0)] {
        let got = img.get_pixel(x, 90).0[0];
        let want = encode(coverage);
        assert!(
            got.abs_diff(want) <= 1,
            "sharp shadow at ({x}, 90) is covered {coverage}: got {got}, want {want}",
        );
    }

    let source = Rect::new(60.0, 40.0, 100.0, 100.0);
    let mut harness = Harness::new();
    let img = harness
        .size(VIEWPORT)
        .clear(RgbaF32::BLACK)
        .frame(|ui| {
            Panel::canvas()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::shadow(Shadow {
                            color: RgbaF32::WHITE,
                            offset: Vec2::ZERO,
                            blur: 8.0,
                            spread: 0.0,
                            inset: false,
                        })
                        .at(source),
                    );
                });
        })
        .image;
    let encode = |coverage: f64| {
        let lin = coverage as f32;
        RgbaF32::new(lin, lin, lin, 1.0).to_srgba_u8().r
    };
    assert!(
        encode(reference_coverage(Vec2::new(184.5, 90.5), source, 0.0, 8.0)) >= 3,
        "the tail past 3σ shows",
    );
    for x in 176..196 {
        let got = img.get_pixel(x, 90).0[0];
        let want = encode(reference_coverage(
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
