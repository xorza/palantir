//! Backdrop panes painted end to end on a headless window, against reference arithmetic and
//! against whole repaints.

use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
use crate::host::headless_window::HeadlessWindow;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::urect::URect;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use glam::{UVec2, Vec2};
use image::RgbaImage;

mod clear;
mod neighbors;

const SIZE: UVec2 = UVec2::new(64, 48);
const PANE: Rect = Rect::new(16.0, 8.0, 32.0, 32.0);

/// Stripes of three colors and widths under where the pane sits, and the pane when `sigma` is
/// some.
fn scene(ui: &mut Ui, sigma: Option<f32>) {
    Panel::canvas()
        .id_salt("backdrop-scene")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for i in 0..16 {
                let x = i as f32 * 4.0;
                let color = match i % 3 {
                    0 => RgbaF32::WHITE,
                    1 => RgbaF32::srgb(1.0, 0.2, 0.0),
                    _ => RgbaF32::srgb(0.0, 0.3, 1.0),
                };
                ui.add_shape(
                    Shape::rect(Rect::new(x, 0.0, 1.0 + (i % 3) as f32, 48.0)).fill(color),
                );
            }
            if let Some(sigma) = sigma {
                ui.add_shape(Shape::rect(PANE).backdrop_filter(BackdropFilter::blur(sigma)));
            }
        });
}

fn frame(sigma: Option<f32>) -> RgbaImage {
    let gpu = headless_test_gpu();
    let mut window = HeadlessWindow::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .open(SIZE, 1.0);
    window.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let mut app = RecordApp::new(|ui: &mut Ui| scene(ui, sigma));
    window.frame(&mut app);
    window.read_back()
}

fn decode(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(linear: f64) -> f64 {
    let c = linear.clamp(0.0, 1.0);
    255.0
        * if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
}

/// One channel of `image` blurred by `plan` as the GPU's stages run, read at every pixel.
fn reference(plan: BlurPlan, image: &RgbaImage, channel: usize) -> Vec<f64> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let rows: Vec<Vec<f64>> = (0..h)
        .map(|y| {
            let line: Vec<f64> = (0..w)
                .map(|x| decode(image.get_pixel(x as u32, y as u32)[channel]))
                .collect();
            plan.reference_residual(&plan.reference_level(&line))
        })
        .collect();
    let level_w = rows[0].len();
    let columns: Vec<Vec<f64>> = (0..level_w)
        .map(|x| {
            let column: Vec<f64> = rows.iter().map(|row| row[x]).collect();
            plan.reference_residual(&plan.reference_level(&column))
        })
        .collect();
    let mut out = vec![0.0; w * h];
    let column_reads: Vec<Vec<f64>> = columns
        .iter()
        .map(|column| (0..h).map(|y| plan.reference_read(column, y)).collect())
        .collect();
    for y in 0..h {
        let row: Vec<f64> = column_reads.iter().map(|c| c[y]).collect();
        for x in 0..w {
            out[y * w + x] = plan.reference_read(&row, x);
        }
    }
    out
}

/// What a pixel of a frame with a pane holds against the same frame without it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
    /// The pixels under it blurred, within one 8-bit step: the dither's half step and the `f16`
    /// stages' rounding.
    Blurred,
    /// The pixel under it, bit for bit.
    Same,
    /// Neither: an edge's coverage.
    Either,
}

/// Every pixel of `got` against `under`, as `at` classes its center: blurred by `plan` as the
/// reference arithmetic blurs `under`, or equal to it.
fn assert_blurred(
    got: &RgbaImage,
    under: &RgbaImage,
    plan: BlurPlan,
    at: impl Fn(Vec2) -> Expect,
    what: &str,
) {
    let want: Vec<Vec<f64>> = (0..3).map(|c| reference(plan, under, c)).collect();
    for (x, y, pixel) in got.enumerate_pixels() {
        match at(Vec2::new(x as f32 + 0.5, y as f32 + 0.5)) {
            Expect::Same => assert_eq!(pixel, under.get_pixel(x, y), "{what}: ({x}, {y})"),
            Expect::Blurred => {
                for (c, want) in want.iter().enumerate() {
                    let w = encode(want[(y * got.width() + x) as usize]);
                    let g = f64::from(pixel[c]);
                    assert!(
                        (g - w).abs() <= 1.0,
                        "{what}: ({x}, {y}) channel {c} is {g}, want {w:.2}"
                    );
                }
            }
            Expect::Either => {}
        }
    }
}

/// A pane shows the pixels painted before it blurred, as the reference arithmetic blurs the same
/// frame without it: every channel within one 8-bit step, the dither's half step and the `f16`
/// stages' rounding. Outside the pane the frame is unchanged, bit for bit. σ 3 blurs at full
/// resolution and σ 6 one level down.
#[test]
fn a_pane_shows_the_pixels_before_it_blurred() {
    let under = frame(None);
    for sigma in [3.0f32, 6.0] {
        let inside = |p: Vec2| {
            if PANE.contains(p) {
                Expect::Blurred
            } else {
                Expect::Same
            }
        };
        let what = format!("σ {sigma}");
        assert_blurred(
            &frame(Some(sigma)),
            &under,
            BlurPlan::new(sigma),
            inside,
            &what,
        );
    }
}

/// A luminance ceiling scales what a pane shows down to it before the tint, and leaves what is
/// darker alone. White on the left, the black clear on the right, under a pane of σ 1 tinted gray
/// 0.2 at alpha 0.5 and capped at 0.1, which lowers to the f16 0.0999755859375: away from the
/// seam, the white side composites to `0.5 · 0.0999755859375 + 0.5 · 0.2 = 0.14998779296875` and
/// the black side to `0.5 · 0 + 0.5 · 0.2 = 0.1`, each channel within one 8-bit step. Without
/// the ceiling the white side is `0.5 · 1 + 0.5 · 0.2 = 0.6`.
#[test]
fn a_luminance_ceiling_holds_a_bright_backdrop_under_it() {
    let paint = |ceiling: Option<f32>| {
        whole(move |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("backdrop-ceiling")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(Rect::new(0.0, 0.0, 32.0, 48.0)).fill(RgbaF32::WHITE));
                    let filter = BackdropFilter {
                        max_luminance: ceiling,
                        ..BackdropFilter::blur(1.0)
                    };
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 64.0, 48.0))
                            .fill(RgbaF32::new(0.2, 0.2, 0.2, 0.5))
                            .backdrop_filter(filter),
                    );
                });
        })
    };
    for (ceiling, white, black) in [(Some(0.1), 0.149_987_792_968_75, 0.1), (None, 0.6, 0.1)] {
        let got = paint(ceiling);
        for (xs, want) in [(4..28, white), (36..60, black)] {
            for x in xs {
                for y in 4..44 {
                    let pixel = got.get_pixel(x, y);
                    for c in 0..3 {
                        let g = f64::from(pixel[c]);
                        assert!(
                            (g - encode(want)).abs() <= 1.0,
                            "{ceiling:?}: ({x}, {y}) channel {c} is {g}, want {:.2}",
                            encode(want)
                        );
                    }
                }
            }
        }
    }
}

/// A canvas with a square of `under` painted before a pane of σ `sigma` at `pane`, and a square
/// of `over` painted after it, inside it.
fn layered(ui: &mut Ui, under: RgbaF32, pane: Rect, sigma: f32, over: RgbaF32) {
    Panel::canvas()
        .id_salt("backdrop-layered")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            ui.add_shape(Shape::rect(Rect::new(6.0, 10.0, 14.0, 14.0)).fill(under));
            for i in 0..8 {
                ui.add_shape(
                    Shape::rect(Rect::new(4.0 + 8.0 * i as f32, 30.0, 3.0, 12.0))
                        .fill(RgbaF32::WHITE),
                );
            }
            ui.add_shape(Shape::rect(pane).backdrop_filter(BackdropFilter::blur(sigma)));
            ui.add_shape(Shape::rect(Rect::new(30.0, 20.0, 6.0, 6.0)).fill(over));
        });
}

/// What a fresh window paints of `scene` on its first, whole frame.
fn whole(scene: impl FnMut(&mut Ui)) -> RgbaImage {
    whole_at(SIZE, scene)
}

/// [`whole`] on a window of `size`.
fn whole_at(size: UVec2, scene: impl FnMut(&mut Ui)) -> RgbaImage {
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, size, RgbaF32::BLACK);
    window.frame(&mut RecordApp::new(scene));
    window.read_back()
}

/// A fresh window on `gpu` of `size` at scale 1 that clears to `clear`.
fn window(gpu: &HeadlessTestGpuLease, size: UVec2, clear: RgbaF32) -> HeadlessWindow {
    let mut window = HeadlessWindow::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .open(size, 1.0);
    window.ui().theme_mut().window_clear = clear;
    window
}

/// Every channel of `got` within one 8-bit step of `want`, the dither's half step either way.
fn assert_within_a_step(got: &RgbaImage, want: &RgbaImage, what: &str) {
    for (x, y, pixel) in got.enumerate_pixels() {
        let other = want.get_pixel(x, y);
        for c in 0..4 {
            assert!(
                pixel[c].abs_diff(other[c]) <= 1,
                "{what}: ({x}, {y}) channel {c} is {}, a whole repaint {}",
                pixel[c],
                other[c]
            );
        }
    }
}

/// On a window kept across frames, a change under a pane captures it again, one above it inside
/// it does not, and a pane sliding inside the region it captured keeps the capture; each partial
/// frame matches a whole repaint of its scene within one 8-bit step.
#[test]
fn a_partial_frame_recaptures_only_a_stale_pane() {
    let gpu = headless_test_gpu();
    let mut window = HeadlessWindow::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .open(SIZE, 1.0);
    window.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    let red = RgbaF32::srgb(1.0, 0.2, 0.1);
    // Two levels down at σ 10: an 4 px lattice, which a one-pixel slide can stay inside.
    let sigma = 10.0;
    let plan = BlurPlan::new(sigma);
    assert_eq!(plan.lattice(), 4);
    let pane = Rect::new(16.0, 8.0, 30.0, 30.0);
    let slid = Rect::new(17.0, 8.0, 30.0, 30.0);
    // The slide keeps the footprint inside the captured, lattice-snapped region on every side.
    let region = |rect: Rect| {
        let reach = UVec2::splat(plan.reach());
        let px = URect::covering(rect);
        let footprint =
            URect::from_min_max(px.min.saturating_sub(reach), (px.max() + reach).min(SIZE));
        plan.region(footprint, SIZE).unwrap()
    };
    assert_eq!(region(pane).union(region(slid)), region(pane));
    let steps = [
        (blue, pane, blue, "first", Some(1)),
        (red, pane, blue, "under", Some(1)),
        (red, pane, red, "over", Some(0)),
        (red, slid, red, "slid", Some(0)),
    ];
    for (under, rect, over, what, blurs) in steps {
        let scene = |ui: &mut Ui| layered(ui, under, rect, sigma, over);
        window.frame(&mut RecordApp::new(scene));
        assert_eq!(Some(window.host().backdrop_blurs()), blurs, "{what}: blurs");
        assert_within_a_step(&window.read_back(), &whole(scene), what);
    }
}

/// Two panes, the second over the first, with squares under, between and over them, on a kept
/// window: each frame matches a whole repaint of its scene within one 8-bit step. A change under
/// the first pane captures both, as the first's rect lies under the second's footprint; one over
/// the second, painted after both, captures neither. The window is large enough that the change
/// under covers less than the share past which a partial frame repaints whole; that frame still
/// paints whole, as the first frame painted straight into the target and left the backbuffer
/// behind.
#[test]
fn a_pane_over_a_pane_repaints_as_a_whole_frame() {
    let scene = |under: RgbaF32, over: RgbaF32| {
        move |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("backdrop-stacked")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..8 {
                        ui.add_shape(
                            Shape::rect(Rect::new(4.0 + 8.0 * i as f32, 2.0, 3.0, 44.0))
                                .fill(RgbaF32::WHITE),
                        );
                    }
                    ui.add_shape(Shape::rect(Rect::new(8.0, 8.0, 6.0, 6.0)).fill(under));
                    ui.add_shape(
                        Shape::rect(Rect::new(4.0, 4.0, 30.0, 30.0))
                            .backdrop_filter(BackdropFilter::blur(3.0)),
                    );
                    ui.add_shape(
                        Shape::rect(Rect::new(26.0, 6.0, 8.0, 8.0))
                            .fill(RgbaF32::srgb(1.0, 0.2, 0.1)),
                    );
                    ui.add_shape(
                        Shape::rect(Rect::new(20.0, 14.0, 36.0, 28.0))
                            .backdrop_filter(BackdropFilter::blur(6.0)),
                    );
                    ui.add_shape(Shape::rect(Rect::new(40.0, 30.0, 6.0, 6.0)).fill(over));
                });
        }
    };
    let size = UVec2::new(128, 96);
    let gpu = headless_test_gpu();
    let mut window = HeadlessWindow::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .open(size, 1.0);
    window.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    let green = RgbaF32::srgb(0.1, 0.9, 0.2);
    for (under, over, what, want_paint, blurs) in [
        (blue, blue, "whole", FramePaint::Full, 2),
        (green, blue, "under the first", FramePaint::Partial, 2),
        (green, green, "over the second", FramePaint::Partial, 0),
    ] {
        let paint = window
            .frame(&mut RecordApp::new(scene(under, over)))
            .paint();
        assert_eq!(
            (paint, window.host().backdrop_blurs()),
            (want_paint, blurs),
            "{what}"
        );
        assert_within_a_step(
            &window.read_back(),
            &whole_at(size, scene(under, over)),
            what,
        );
    }
}

/// A chrome with a backdrop filter paints the pane a rect shape with the same rect, corners, fill
/// and filter paints, bit for bit: both lower to one pane quad, keyed by the owner's first row.
#[test]
fn a_chrome_pane_paints_as_a_rect_pane() {
    let tint = RgbaF32::new(1.0, 1.0, 1.0, 0.25);
    let filter = BackdropFilter::frosted(4.0);
    let stripes = |ui: &mut Ui| {
        Panel::canvas()
            .id_salt("backdrop-chrome-stripes")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| scene(ui, None));
    };
    let chrome = whole(|ui: &mut Ui| {
        Panel::zstack()
            .id_salt("backdrop-chrome")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                stripes(ui);
                Panel::canvas()
                    .id_salt("backdrop-chrome-pane")
                    .size((Sizing::FILL, Sizing::FILL))
                    .chrome(Chrome::rounded(tint, Corners::all(8.0)).with_backdrop_filter(filter))
                    .show(ui, |_| {});
            });
    });
    let shape = whole(|ui: &mut Ui| {
        Panel::zstack()
            .id_salt("backdrop-chrome")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                stripes(ui);
                Panel::canvas()
                    .id_salt("backdrop-chrome-pane")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::owner_rect()
                                .corners(8.0)
                                .fill(tint)
                                .backdrop_filter(filter),
                        );
                    });
            });
    });
    assert!(chrome == shape, "a chrome pane differs from a rect pane");
    assert!(chrome != whole(stripes), "the pane paints");
}

/// A frosted popup with a translucent border and a shadow that shrinks, then grows back, as a
/// palette does while a search narrows and clears, on a kept window: each frame matches a whole
/// repaint within one 8-bit step. Growing back captures the pane again, and its footprint holds
/// the damage rect; were the two scissors to overlap, the walk past the pane's barrier would
/// composite the border, and everything translucent after it, twice there.
#[test]
fn a_pane_growing_back_paints_once_under_overlapping_repaints() {
    let size = UVec2::new(1000, 700);
    let scene = |h: f32, inner: RgbaF32| {
        move |ui: &mut Ui| {
            Panel::zstack()
                .id_salt("backdrop-grow")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id_salt("backdrop-grow-stripes")
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| scene(ui, None));
                    let chrome =
                        Chrome::rounded(RgbaF32::new(0.04, 0.04, 0.04, 0.72), Corners::all(8.0))
                            .with_border(Stroke::new(RgbaF32::new(1.0, 1.0, 1.0, 0.12), 1.0))
                            .with_shadow(Shadow::drop(
                                RgbaF32::new(0.0, 0.0, 0.0, 0.5),
                                Vec2::new(0.0, 8.0),
                                16.0,
                            ))
                            .with_backdrop_filter(BackdropFilter::frosted(8.0));
                    Popup::new(Anchor::at_point(Vec2::new(30.0, 20.0)))
                        .id_salt("backdrop-grow-pane")
                        .layer(Layer::Menu)
                        .chrome(chrome)
                        .size((Sizing::fixed(260.0), Sizing::fixed(h)))
                        .show(ui, |ui, _| {
                            Panel::canvas()
                                .id_salt("backdrop-grow-row")
                                .size((Sizing::fixed(30.0), Sizing::fixed(10.0)))
                                .chrome(Chrome::fill(inner))
                                .show(ui, |_| {});
                        });
                });
        }
    };
    let gpu = headless_test_gpu();
    let mut window = HeadlessWindow::builder(gpu.handles())
        .shaper(TextShaper::test_mono())
        .open(size, 1.0);
    window.ui().theme_mut().window_clear = RgbaF32::BLACK;
    let red = RgbaF32::srgb(1.0, 0.2, 0.1);
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    for (what, h, inner) in [
        ("tall", 400.0, red),
        ("short", 120.0, blue),
        ("short again", 120.0, red),
        ("tall again", 400.0, blue),
        ("still tall", 400.0, red),
    ] {
        window.frame(&mut RecordApp::new(scene(h, inner)));
        assert_within_a_step(&window.read_back(), &whole_at(size, scene(h, inner)), what);
    }
}
