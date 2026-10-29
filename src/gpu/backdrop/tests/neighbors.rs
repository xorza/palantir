//! Panes beside, inside and among other panes: a neighbor under a pane's reach, a rounded clip
//! around one, more stale panes than a partial frame holds, and results past the atlas.

use crate::gpu::backdrop::tests::{
    Expect, SIZE, assert_blurred, assert_within_a_step, whole, whole_at, window,
};
use crate::gpu::test_gpu::headless_test_gpu;
use crate::internals::record_app::RecordApp;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::backdrop_filter::BackdropFilter;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::color::RgbaF32;
use crate::renderer::pane_captures::PaneCaptures;
use crate::shape::Shape;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::ui::Ui;
use crate::ui::frame_report::FramePaint;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;
use glam::{UVec2, Vec2};

/// Stripes of three colors and widths, one every 4 px, across a window `width` px wide and 48
/// high.
fn stripes(ui: &mut Ui, width: u32) {
    for i in 0..width / 4 {
        let color = match i % 3 {
            0 => RgbaF32::WHITE,
            1 => RgbaF32::srgb(1.0, 0.2, 0.0),
            _ => RgbaF32::srgb(0.0, 0.3, 1.0),
        };
        ui.add_shape(
            Shape::rect(Rect::new(i as f32 * 4.0, 0.0, 1.0 + (i % 3) as f32, 48.0)).fill(color),
        );
    }
}

/// A canvas over the window holding `body`.
fn canvas(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::canvas()
        .id_salt("backdrop-neighbors")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, body);
}

const LEFT: Rect = Rect::new(4.0, 4.0, 24.0, 40.0);
/// Two px right of [`LEFT`], well inside σ 3's 12 px reach.
const RIGHT: Rect = Rect::new(30.0, 4.0, 30.0, 40.0);

/// Of two panes side by side, the second's reach covers the first, which was painted before it:
/// it shows the first's pane blurred, as the reference blurs a frame of the stripes and the first
/// pane alone, and the first is the same as in that frame, bit for bit.
#[test]
fn a_pane_beside_a_pane_shows_it_blurred() {
    let sigma = 3.0;
    let left = |ui: &mut Ui| {
        ui.add_shape(Shape::rect(LEFT).backdrop_filter(BackdropFilter::blur(sigma)));
    };
    let under = whole(|ui: &mut Ui| {
        canvas(ui, |ui| {
            stripes(ui, 64);
            left(ui);
        });
    });
    let got = whole(|ui: &mut Ui| {
        canvas(ui, |ui| {
            stripes(ui, 64);
            left(ui);
            ui.add_shape(Shape::rect(RIGHT).backdrop_filter(BackdropFilter::blur(sigma)));
        });
    });
    let at = |p: Vec2| {
        if RIGHT.contains(p) {
            Expect::Blurred
        } else {
            Expect::Same
        }
    };
    assert_blurred(&got, &under, BlurPlan::new(sigma), at, "beside");
}

/// Two panes apart past σ 1's 4 px reach share the main pass's break and blur in the same passes,
/// their scratch rows apart: each shows the stripes under it blurred, as the reference blurs a
/// frame of the stripes alone.
#[test]
fn panes_apart_blur_together() {
    let sigma = 1.0;
    let left = Rect::new(2.0, 4.0, 14.0, 40.0);
    let right = Rect::new(36.0, 6.0, 22.0, 30.0);
    let under = whole(|ui: &mut Ui| canvas(ui, |ui| stripes(ui, 64)));
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, SIZE, RgbaF32::BLACK);
    window.frame(&mut RecordApp::new(|ui: &mut Ui| {
        canvas(ui, |ui| {
            stripes(ui, 64);
            for rect in [left, right] {
                ui.add_shape(Shape::rect(rect).backdrop_filter(BackdropFilter::blur(sigma)));
            }
        });
    }));
    assert_eq!(window.host().backdrop_blurs(), 2);
    let at = |p: Vec2| {
        if left.contains(p) || right.contains(p) {
            Expect::Blurred
        } else {
            Expect::Same
        }
    };
    assert_blurred(
        &window.read_back(),
        &under,
        BlurPlan::new(sigma),
        at,
        "apart",
    );
}

/// A pane inside a rounded clip, which stamps the stencil, blurs what lies under it inside the
/// clip and leaves the rest: the pass breaks at the pane, and the walk after the break stamps the
/// clip again. Away from the corners and their coverage, the inside matches the reference blur,
/// and outside the clip the stripes are unchanged.
#[test]
fn a_pane_inside_a_rounded_clip_blurs_inside_it() {
    let sigma = 3.0;
    let clip = Rect::new(8.0, 6.0, 48.0, 36.0);
    let radius = 10.0;
    let under = whole(|ui: &mut Ui| canvas(ui, |ui| stripes(ui, 64)));
    let got = whole(|ui: &mut Ui| {
        canvas(ui, |ui| {
            stripes(ui, 64);
            Panel::zstack()
                .id_salt("backdrop-clipped")
                .pos((clip.min.x, clip.min.y))
                .size((Sizing::fixed(clip.size.w), Sizing::fixed(clip.size.h)))
                .chrome(Chrome::rounded(RgbaF32::TRANSPARENT, Corners::all(radius)))
                .clip_rounded()
                .show(ui, |ui| {
                    ui.add_shape(
                        Shape::rect(Rect::new(0.0, 0.0, 64.0, 48.0))
                            .backdrop_filter(BackdropFilter::blur(sigma)),
                    );
                });
        });
    });
    let inset = |x: f32, y: f32| {
        Rect::new(
            clip.min.x + x,
            clip.min.y + y,
            clip.size.w - 2.0 * x,
            clip.size.h - 2.0 * y,
        )
    };
    let (across, down) = (inset(radius, 1.0), inset(1.0, radius));
    let at = |p: Vec2| {
        if across.contains(p) || down.contains(p) {
            Expect::Blurred
        } else if clip.inflated(1.0).contains(p) {
            Expect::Either
        } else {
            Expect::Same
        }
    };
    assert_blurred(&got, &under, BlurPlan::new(sigma), at, "rounded clip");
}

/// Nine panes in a 3 × 3 grid, apart past σ 1's 4 px reach, each over a square of its own on a
/// kept window. A change under one or two captures them alone; under all nine, one more than a
/// partial frame holds, the frame paints whole. Each frame matches a whole repaint within one
/// 8-bit step. The second frame paints whole whatever its plan, as the first painted straight
/// into the target and left the backbuffer behind.
#[test]
fn more_stale_panes_than_a_partial_frame_holds_paint_whole() {
    assert_eq!(PaneCaptures::PARTIAL_CAP, 8);
    let size = UVec2::new(128, 96);
    let cell = |i: usize| Vec2::new(8.0 + 40.0 * (i % 3) as f32, 8.0 + 30.0 * (i / 3) as f32);
    let scene = |changed: usize| {
        move |ui: &mut Ui| {
            canvas(ui, |ui| {
                for i in 0..9 {
                    let at = cell(i);
                    let color = if i < changed {
                        RgbaF32::srgb(1.0, 0.2, 0.1)
                    } else {
                        RgbaF32::srgb(0.1, 0.3, 1.0)
                    };
                    ui.add_shape(
                        Shape::rect(Rect::new(at.x + 6.0, at.y + 4.0, 4.0, 4.0)).fill(color),
                    );
                }
                for i in 0..9 {
                    let at = cell(i);
                    ui.add_shape(
                        Shape::rect(Rect::new(at.x, at.y, 16.0, 12.0))
                            .fill(RgbaF32::new(1.0, 1.0, 1.0, 0.2))
                            .backdrop_filter(BackdropFilter::blur(1.0)),
                    );
                }
            });
        }
    };
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, size, RgbaF32::BLACK);
    for (changed, want) in [
        (0, (FramePaint::Full, 9)),
        (1, (FramePaint::Partial, 9)),
        (0, (FramePaint::Partial, 1)),
        (1, (FramePaint::Partial, 1)),
        (0, (FramePaint::Partial, 1)),
        (2, (FramePaint::Partial, 2)),
        (0, (FramePaint::Partial, 2)),
        (9, (FramePaint::Full, 9)),
    ] {
        let paint = window.frame(&mut RecordApp::new(scene(changed))).paint();
        let what = format!("{changed} changed");
        assert_eq!((paint, window.host().backdrop_blurs()), want, "{what}");
        assert_within_a_step(&window.read_back(), &whole_at(size, scene(changed)), &what);
    }
}

/// A pane whose result outgrows the 1024² atlas grows it, which copies the results it holds: a
/// small pane keeps its result through the frame that adds a pane 1060 px wide, its footprint
/// past 1024 at σ 2, on a window tall enough that it stays a partial frame. Each frame matches a
/// whole repaint within one 8-bit step. A dot painted after the panes flips on the second frame,
/// which paints whole as the first left the backbuffer behind.
#[test]
fn a_result_past_the_atlas_grows_it_and_keeps_the_rest() {
    let size = UVec2::new(1100, 200);
    let scene = |dot: bool, wide: bool| {
        move |ui: &mut Ui| {
            canvas(ui, |ui| {
                stripes(ui, 1100);
                ui.add_shape(
                    Shape::rect(Rect::new(4.0, 4.0, 16.0, 40.0))
                        .backdrop_filter(BackdropFilter::blur(2.0)),
                );
                if wide {
                    ui.add_shape(
                        Shape::rect(Rect::new(36.0, 4.0, 1060.0, 40.0))
                            .backdrop_filter(BackdropFilter::blur(2.0)),
                    );
                }
                let dot = if dot { RgbaF32::WHITE } else { RgbaF32::BLACK };
                ui.add_shape(Shape::rect(Rect::new(1096.0, 44.0, 2.0, 2.0)).fill(dot));
            });
        }
    };
    let gpu = headless_test_gpu();
    let mut window = window(&gpu, size, RgbaF32::BLACK);
    for (dot, wide, want, what) in [
        (false, false, (FramePaint::Full, 1), "first"),
        (true, false, (FramePaint::Partial, 1), "dot"),
        (true, true, (FramePaint::Partial, 1), "grown"),
    ] {
        let painted = window.frame(&mut RecordApp::new(scene(dot, wide))).paint();
        assert_eq!((painted, window.host().backdrop_blurs()), want, "{what}");
        assert_within_a_step(&window.read_back(), &whole_at(size, scene(dot, wide)), what);
    }
}
