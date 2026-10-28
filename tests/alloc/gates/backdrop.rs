//! Backdrop panes over the frame bench's tree add no allocation to a steady frame.

use palantir::internals::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use palantir::internals::ui_harness::UiHarness;
use palantir::internals::ui_harness::frontend_harness::FrontendHarness;
use palantir::widget::Shape;
use palantir::{BackdropFilter, Configure, FramePaint, Lens, Panel, Rect, RgbaF32, Sizing, Ui};

use crate::gates::{MEASURE_FRAMES, WARMUP_FRAMES};
use crate::harness::Audit;

/// The look of the panes.
#[derive(Clone, Copy, Debug)]
enum Look {
    Plain,
    Frosted,
    Glass,
}

/// A toolbar, a sidebar and a popup as panes over the tree in `look`, and a square under the
/// toolbar's pane in `under`.
fn panes(ui: &mut Ui, state: &mut FrameFixture, look: Look, under: RgbaF32) {
    Panel::zstack()
        .id_salt("alloc-backdrop")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            state.render(BENCH_SCALE, ui);
            Panel::canvas()
                .id_salt("alloc-backdrop-panes")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect(Rect::new(300.0, 20.0, 12.0, 12.0)).fill(under));
                    for rect in [
                        Rect::new(0.0, 0.0, 1280.0, 48.0),
                        Rect::new(0.0, 48.0, 240.0, 672.0),
                        Rect::new(500.0, 200.0, 320.0, 220.0),
                    ] {
                        let shape = match look {
                            Look::Plain => {
                                Shape::rect(rect).backdrop_filter(BackdropFilter::blur(12.0))
                            }
                            Look::Frosted => Shape::rect(rect)
                                .fill(RgbaF32::new(1.0, 1.0, 1.0, 0.2))
                                .backdrop_filter(BackdropFilter::frosted(16.0)),
                            Look::Glass => Shape::rect(rect).backdrop_filter(
                                BackdropFilter::glass(Lens::default()).with_blur(2.0),
                            ),
                        };
                        ui.add_shape(shape);
                    }
                });
        });
}

/// Record through compose with panes, deviceless, strict zero: a still frame, whose skip paints
/// the whole scene again, and a frame whose change under a pane makes it and the sidebar's stale,
/// for every look.
#[test]
fn backdrop_cpu_frames_alloc_free() {
    for look in [Look::Plain, Look::Frosted, Look::Glass] {
        for flips in [false, true] {
            let mut state = FrameFixture::default();
            let mut frontend =
                FrontendHarness::new(UiHarness::with_text(BENCH_SURFACE).scale(BENCH_DPR));
            let mut odd = false;
            Audit::new()
                .warmup(WARMUP_FRAMES)
                .frames(MEASURE_FRAMES)
                .run_frames(|| {
                    odd = flips && !odd;
                    let under = if odd {
                        RgbaF32::srgb(1.0, 0.3, 0.1)
                    } else {
                        RgbaF32::srgb(0.1, 0.3, 1.0)
                    };
                    let report = frontend.frame(|ui| panes(ui, &mut state, look, under));
                    if report.paint() == FramePaint::Skip {
                        frontend.paint_full();
                    }
                });
        }
    }
}
