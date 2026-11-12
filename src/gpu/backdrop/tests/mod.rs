//! Backdrops painted end to end on a headless window, against reference arithmetic and
//! against whole repaints.

use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::test_gpu::{TestGpu, headless_test_gpu};
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::urect::URect;
use crate::primitives::layout::anchor::Anchor;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::libm_ext::Libm;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::shadow::Shadow;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::theme::popup::PopupTheme;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use glam::{UVec2, Vec2};
use image::RgbaImage;

mod clear;
mod dispersion;
mod fill;
mod neighbors;

const SIZE: UVec2 = UVec2::new(64, 48);
/// A clear three tenths of a step past the sRGB bytes 51, 115 and 178, so a backdrop that showed it
/// unrounded could miss the clear around it by a step, and far enough from the midpoints that
/// the target rounds it as the CPU does.
pub(super) const CLEAR: RgbaF32 = RgbaF32::srgb(51.3 / 255.0, 115.3 / 255.0, 178.3 / 255.0);
const BACKDROP: Rect = Rect::new(16.0, 8.0, 32.0, 32.0);

/// Stripes of three colors and widths under where the backdrop sits, and the backdrop when `sigma`
/// is some.
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
                    Shape::rect().at(Rect::new(x, 0.0, 1.0 + (i % 3) as f32, 48.0)),
                    color,
                );
            }
            if let Some(sigma) = sigma {
                ui.add_shape(Shape::rect().at(BACKDROP), Material::blur(sigma));
            }
        });
}

fn decode(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).libm_powf(2.4)
    }
}

fn encode(linear: f64) -> f64 {
    let c = linear.clamp(0.0, 1.0);
    255.0
        * if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.libm_powf(1.0 / 2.4) - 0.055
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

/// What a pixel of a frame with a backdrop holds against the same frame without it.
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

/// A backdrop shows the pixels painted before it blurred, as the reference arithmetic blurs the
/// same frame without it: every channel within one 8-bit step, the dither's half step and the `f16`
/// stages' rounding. Outside the backdrop the frame is unchanged, bit for bit. σ 3 blurs at full
/// resolution and σ 6 one level down.
#[test]
fn a_backdrop_shows_the_pixels_before_it_blurred() {
    let gpu = headless_test_gpu();
    let mut reference = Reference::new(&gpu, SIZE, RgbaF32::BLACK);
    let under = reference.paint(|ui| scene(ui, None));
    for sigma in [3.0f32, 6.0] {
        let inside = |p: Vec2| {
            if BACKDROP.contains(p) {
                Expect::Blurred
            } else {
                Expect::Same
            }
        };
        let what = format!("σ {sigma}");
        assert_blurred(
            &reference.paint(|ui| scene(ui, Some(sigma))),
            &under,
            BlurPlan::new(sigma),
            inside,
            &what,
        );
    }
}

/// A backdrop wider than half a page shows the pixels before it blurred through a scratch texture
/// of its own, beside one on a page in the same draw batch: each as the reference arithmetic
/// blurs the frame without them, every channel within one 8-bit step, and the frame unchanged
/// outside them. At σ 3 the wide one's 600 px, grown by its reach and apron, pass the 512 a page of
/// 1024 takes; the narrow one's 40 do not. Each frame's submit decays the pool, and once the
/// glasses go, their scratch texture idles an expiry window of frames and frees.
#[test]
fn a_wide_backdrop_shows_its_pixels_through_a_scratch_texture() {
    const WIDE: UVec2 = UVec2::new(720, 64);
    let (wide, narrow) = (
        Rect::new(8.0, 16.0, 600.0, 32.0),
        Rect::new(640.0, 16.0, 40.0, 32.0),
    );
    let stripes = |ui: &mut Ui, sigma: Option<f32>| {
        Panel::canvas()
            .id_salt("wide-backdrop-scene")
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for i in 0..180 {
                    let color = match i % 3 {
                        0 => RgbaF32::WHITE,
                        1 => RgbaF32::srgb(1.0, 0.2, 0.0),
                        _ => RgbaF32::srgb(0.0, 0.3, 1.0),
                    };
                    let x = i as f32 * 4.0;
                    ui.add_shape(
                        Shape::rect().at(Rect::new(x, 0.0, 1.0 + (i % 3) as f32, 64.0)),
                        color,
                    );
                }
                if let Some(sigma) = sigma {
                    for at in [wide, narrow] {
                        ui.add_shape(Shape::rect().at(at), Material::blur(sigma));
                    }
                }
            });
    };
    let gpu = headless_test_gpu();
    let mut reference = Reference::new(&gpu, WIDE, RgbaF32::BLACK);
    let under = reference.paint(|ui| stripes(ui, None));
    let got = reference.paint(|ui| stripes(ui, Some(3.0)));
    assert!(
        reference.0.backdrop_scratch_texels() > 0,
        "the wide result holds a scratch texture"
    );
    let inside = |p: Vec2| {
        if wide.contains(p) || narrow.contains(p) {
            Expect::Blurred
        } else {
            Expect::Same
        }
    };
    assert_blurred(&got, &under, BlurPlan::new(3.0), inside, "σ 3");
    assert!(
        reference.0.atlas_decayed(),
        "each frame's submit decays the pool"
    );

    // The glasses gone, the scratch texture idles from the first frame without them, and frees an
    // expiry window of display frames later.
    let held = reference.0.backdrop_scratch_texels();
    for frame in 1..=EXPIRY_FRAMES {
        reference.paint(|ui| stripes(ui, None));
        assert_eq!(reference.0.backdrop_scratch_texels(), held, "frame {frame}");
    }
    reference.paint(|ui| stripes(ui, None));
    assert_eq!(reference.0.backdrop_scratch_texels(), 0, "expired");
}

/// A luminance ceiling scales what a backdrop shows down to it before the fill, and leaves what is
/// darker alone. White on the left, the black clear on the right, under a backdrop of σ 1 filled
/// gray 0.2 at alpha 0.5 and capped at 0.1, which lowers to the f16 0.0999755859375: away from the
/// seam, the white side composites to `0.5 · 0.0999755859375 + 0.5 · 0.2 = 0.14998779296875` and
/// the black side to `0.5 · 0 + 0.5 · 0.2 = 0.1`, each channel within one 8-bit step. Without the
/// ceiling the white side is `0.5 · 1 + 0.5 · 0.2 = 0.6`.
#[test]
fn a_luminance_ceiling_holds_a_bright_backdrop_under_it() {
    let gpu = headless_test_gpu();
    let mut reference = Reference::new(&gpu, SIZE, RgbaF32::BLACK);
    let mut paint = |ceiling: Option<f32>| {
        reference.paint(move |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("backdrop-ceiling")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect().at(Rect::new(0.0, 0.0, 32.0, 48.0)),
                        RgbaF32::WHITE,
                    );
                    let filter = Material {
                        max_luminance: ceiling,
                        ..Material::blur(1.0)
                    };
                    ui.add_shape(
                        Shape::rect().at(Rect::new(0.0, 0.0, 64.0, 48.0)),
                        Paint::from(RgbaF32::new(0.2, 0.2, 0.2, 0.5)).with_material(filter),
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

/// A luminance floor lifts what a backdrop shows up to it by a neutral before the fill, black too,
/// and leaves what is brighter alone. White on the left, the black clear on the right, under a
/// backdrop of σ 1 filled gray 0.2 at alpha 0.5 with a floor of 0.3, which lowers to the f16 at or
/// above it, 0.300048828125: away from the seam, the black side composites to
/// `0.5 · 0.300048828125 + 0.5 · 0.2 = 0.2500244140625` and the white side stays
/// `0.5 · 1 + 0.5 · 0.2 = 0.6`. Without the floor the black side is `0.1`.
#[test]
fn a_luminance_floor_lifts_a_dark_backdrop_to_it() {
    let gpu = headless_test_gpu();
    let mut reference = Reference::new(&gpu, SIZE, RgbaF32::BLACK);
    let mut paint = |floor: Option<f32>| {
        reference.paint(move |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("backdrop-floor")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect().at(Rect::new(0.0, 0.0, 32.0, 48.0)),
                        RgbaF32::WHITE,
                    );
                    let filter = Material {
                        min_luminance: floor,
                        ..Material::blur(1.0)
                    };
                    ui.add_shape(
                        Shape::rect().at(Rect::new(0.0, 0.0, 64.0, 48.0)),
                        Paint::from(RgbaF32::new(0.2, 0.2, 0.2, 0.5)).with_material(filter),
                    );
                });
        })
    };
    for (floor, white, black) in [(Some(0.3), 0.6, 0.250_024_414_062_5), (None, 0.6, 0.1)] {
        let got = paint(floor);
        for (xs, want) in [(4..28, white), (36..60, black)] {
            for x in xs {
                for y in 4..44 {
                    let pixel = got.get_pixel(x, y);
                    for c in 0..3 {
                        let g = f64::from(pixel[c]);
                        assert!(
                            (g - encode(want)).abs() <= 1.0,
                            "{floor:?}: ({x}, {y}) channel {c} is {g}, want {:.2}",
                            encode(want)
                        );
                    }
                }
            }
        }
    }
}

/// A canvas with a square of `under` painted before a backdrop of σ `sigma` at `backdrop`, and a
/// square of `over` painted after it, inside it.
fn layered(ui: &mut Ui, under: RgbaF32, backdrop: Rect, sigma: f32, over: RgbaF32) {
    Panel::canvas()
        .id_salt("backdrop-layered")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            ui.add_shape(Shape::rect().at(Rect::new(6.0, 10.0, 14.0, 14.0)), under);
            for i in 0..8 {
                ui.add_shape(
                    Shape::rect().at(Rect::new(4.0 + 8.0 * i as f32, 30.0, 3.0, 12.0)),
                    RgbaF32::WHITE,
                );
            }
            ui.add_shape(Shape::rect().at(backdrop), Material::blur(sigma));
            ui.add_shape(Shape::rect().at(Rect::new(30.0, 20.0, 6.0, 6.0)), over);
        });
}

/// A window that paints each scene it is given whole, as a fresh window's first frame does: the
/// reference a frame is held to, built once per test, its target repainted whole each time.
#[derive(Debug)]
struct Reference(Harness);

impl Reference {
    /// A reference window on `gpu` of `size` that clears to `clear`.
    fn new(gpu: &TestGpu, size: UVec2, clear: RgbaF32) -> Self {
        Self(window(gpu, size, clear))
    }

    /// What a fresh window paints of `scene` on its first, whole frame.
    fn paint(&mut self, scene: impl FnMut(&mut Ui)) -> RgbaImage {
        self.0.frame_without_baseline(scene);
        self.0.read_back()
    }

    /// The blurs the last [`Self::paint`] ran.
    fn blurs(&self) -> usize {
        self.0.backdrop_blurs()
    }
}

/// A fresh window on `gpu` of `size` at scale 1 that clears to `clear`.
fn window(gpu: &TestGpu, size: UVec2, clear: RgbaF32) -> Harness {
    let mut window = Harness::builder(size)
        .mono()
        .depth(Depth::gpu(gpu.handles()))
        .build();
    window.ui_mut().theme_mut().window_bg = clear;
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

/// On a window kept across frames, a change under a backdrop captures it again, one above it inside
/// it does not, and a backdrop sliding inside the region it captured keeps the capture; each
/// partial frame matches a whole repaint of its scene within one 8-bit step.
#[test]
fn a_partial_frame_recaptures_only_a_stale_backdrop() {
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, SIZE, RgbaF32::BLACK);
    let mut reference = Reference::new(&gpu, SIZE, RgbaF32::BLACK);
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    let red = RgbaF32::srgb(1.0, 0.2, 0.1);
    // Two levels down at σ 10: an 4 px lattice, which a one-pixel slide can stay inside.
    let sigma = 10.0;
    let plan = BlurPlan::new(sigma);
    assert_eq!(plan.lattice(), 4);
    let backdrop = Rect::new(16.0, 8.0, 30.0, 30.0);
    let slid = Rect::new(17.0, 8.0, 30.0, 30.0);
    // The slide keeps the footprint inside the captured, lattice-snapped region on every side.
    let region = |rect: Rect| {
        let reach = UVec2::splat(plan.reach());
        let px = URect::covering(rect);
        let footprint =
            URect::from_min_max(px.min.saturating_sub(reach), (px.max() + reach).min(SIZE));
        plan.region(footprint, SIZE).unwrap()
    };
    assert_eq!(region(backdrop).union(region(slid)), region(backdrop));
    let steps = [
        (blue, backdrop, blue, "first", Some(1)),
        (red, backdrop, blue, "under", Some(1)),
        (red, backdrop, red, "over", Some(0)),
        (red, slid, red, "slid", Some(0)),
    ];
    for (under, rect, over, what, blurs) in steps {
        let mut scene = |ui: &mut Ui| layered(ui, under, rect, sigma, over);
        window.frame(&mut scene);
        assert_eq!(Some(window.backdrop_blurs()), blurs, "{what}: blurs");
        assert_within_a_step(&window.read_back(), &reference.paint(scene), what);
    }
}

/// Two backdrops, the second over the first, with squares under, between and over them, on a kept
/// window: each frame matches a whole repaint of its scene within one 8-bit step. A change under
/// the first backdrop captures both, as the first's rect lies under the second's footprint; one
/// over the second, painted after both, captures neither. The window is large enough that the
/// change under covers less than the share past which a partial frame repaints whole; that frame
/// still paints whole, as the first frame painted straight into the target and left the backbuffer
/// behind.
#[test]
fn a_backdrop_over_a_backdrop_repaints_as_a_whole_frame() {
    let scene = |under: RgbaF32, over: RgbaF32| {
        move |ui: &mut Ui| {
            Panel::canvas()
                .id_salt("backdrop-stacked")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..8 {
                        ui.add_shape(
                            Shape::rect().at(Rect::new(4.0 + 8.0 * i as f32, 2.0, 3.0, 44.0)),
                            RgbaF32::WHITE,
                        );
                    }
                    ui.add_shape(Shape::rect().at(Rect::new(8.0, 8.0, 6.0, 6.0)), under);
                    ui.add_shape(
                        Shape::rect().at(Rect::new(4.0, 4.0, 30.0, 30.0)),
                        Material::blur(3.0),
                    );
                    ui.add_shape(
                        Shape::rect().at(Rect::new(26.0, 6.0, 8.0, 8.0)),
                        RgbaF32::srgb(1.0, 0.2, 0.1),
                    );
                    ui.add_shape(
                        Shape::rect().at(Rect::new(20.0, 14.0, 36.0, 28.0)),
                        Material::blur(6.0),
                    );
                    ui.add_shape(Shape::rect().at(Rect::new(40.0, 30.0, 6.0, 6.0)), over);
                });
        }
    };
    let size = UVec2::new(128, 96);
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, size, RgbaF32::BLACK);
    let mut reference = Reference::new(&gpu, size, RgbaF32::BLACK);
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    let green = RgbaF32::srgb(0.1, 0.9, 0.2);
    for (under, over, what, want_paint, blurs) in [
        (blue, blue, "whole", FramePaint::Full, 2),
        (green, blue, "under the first", FramePaint::Partial, 2),
        (green, green, "over the second", FramePaint::Partial, 0),
    ] {
        let paint = window.frame(&mut scene(under, over)).paint();
        assert_eq!(
            (paint, window.backdrop_blurs()),
            (want_paint, blurs),
            "{what}"
        );
        assert_within_a_step(
            &window.read_back(),
            &reference.paint(scene(under, over)),
            what,
        );
    }
}

/// A frosted popup with a translucent border and a shadow that shrinks, then grows back, as a
/// palette does while a search narrows and clears, on a kept window: each frame matches a whole
/// repaint within one 8-bit step. Growing back captures the backdrop again, and its footprint holds
/// the damage rect; were the two scissors to overlap, the walk past the backdrop's barrier would
/// composite the border, and everything translucent after it, twice there.
#[test]
fn a_backdrop_growing_back_paints_once_under_overlapping_repaints() {
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
                    let style = PopupTheme {
                        panel: Chrome::rounded(
                            RgbaF32::new(0.04, 0.04, 0.04, 0.72),
                            Corners::all(8.0),
                        )
                        .with_border(Border::new(RgbaF32::new(1.0, 1.0, 1.0, 0.12), 1.0))
                        .with_shadow(Shadow::drop(
                            RgbaF32::new(0.0, 0.0, 0.0, 0.5),
                            Vec2::new(0.0, 8.0),
                            16.0,
                        )),
                        material: Some(Material::frosted(8.0)),
                        padding: Spacing::ZERO,
                    };
                    Popup::new()
                        .anchor(Anchor::at_point(Vec2::new(30.0, 20.0)))
                        .id_salt("backdrop-grow-backdrop")
                        .layer(Layer::Menu)
                        .style(&style)
                        .size((Sizing::fixed(260.0), Sizing::fixed(h)))
                        .show(ui, |ui| {
                            Panel::canvas()
                                .id_salt("backdrop-grow-row")
                                .size((Sizing::fixed(30.0), Sizing::fixed(10.0)))
                                .chrome(Chrome::new(inner))
                                .show(ui, |_| {});
                        });
                });
        }
    };
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, size, RgbaF32::BLACK);
    let mut reference = Reference::new(&gpu, size, RgbaF32::BLACK);
    let red = RgbaF32::srgb(1.0, 0.2, 0.1);
    let blue = RgbaF32::srgb(0.1, 0.3, 1.0);
    for (what, h, inner) in [
        ("tall", 400.0, red),
        ("short", 120.0, blue),
        ("short again", 120.0, red),
        ("tall again", 400.0, blue),
        ("still tall", 400.0, red),
    ] {
        window.frame(&mut scene(h, inner));
        assert_within_a_step(&window.read_back(), &reference.paint(scene(h, inner)), what);
    }
}
