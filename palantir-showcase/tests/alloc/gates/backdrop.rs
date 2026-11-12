//! Backdrops over the showcase's first page add no allocation to a steady frame.

use palantir::bench::scene::{BENCH_DPR, BENCH_SURFACE};
use palantir::widget::Shape;
use palantir::{App, Depth, Harness};
use palantir::{Configure, FramePaint, Lens, Material, Panel, Rect, RgbaF32, Sizing, Ui};
use palantir_showcase::{MAIN_WINDOW, Shell};

use crate::gates::{MEASURE_FRAMES, WARMUP_FRAMES};
use crate::harness::Audit;
use palantir::Paint;

/// The look of the backdrops.
#[derive(Clone, Copy, Debug)]
enum Look {
    Plain,
    Frosted,
    Glass,
}

/// A toolbar, a sidebar and a popup as backdrops over the tree in `look`, and a square under the
/// toolbar's backdrop in `under`.
fn backdrops(ui: &mut Ui, shell: &mut Shell, look: Look, under: RgbaF32) {
    Panel::zstack()
        .id_salt("alloc-backdrop")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            shell.record(MAIN_WINDOW, ui);
            Panel::canvas()
                .id_salt("alloc-backdrops")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    ui.add_shape(Shape::rect().at(Rect::new(300.0, 20.0, 12.0, 12.0)), under);
                    for rect in [
                        Rect::new(0.0, 0.0, 1280.0, 48.0),
                        Rect::new(0.0, 48.0, 240.0, 672.0),
                        Rect::new(500.0, 200.0, 320.0, 220.0),
                    ] {
                        let paint = match look {
                            Look::Plain => Paint::from(Material::blur(12.0)),
                            Look::Frosted => Paint::from(RgbaF32::new(1.0, 1.0, 1.0, 0.2))
                                .with_material(Material::frosted(16.0)),
                            Look::Glass => Paint::from(
                                Material::glass(Lens::WINDOW_GLASS).with_backdrop_blur(2.0),
                            ),
                        };
                        ui.add_shape(Shape::rect().at(rect), paint);
                    }
                });
        });
}

/// Record through compose with backdrops, deviceless, strict zero: a still frame, whose skip paints
/// the whole scene again, and a frame whose change under a backdrop makes it and the sidebar's
/// stale, for every look.
#[test]
fn backdrop_cpu_frames_alloc_free() {
    for look in [Look::Plain, Look::Frosted, Look::Glass] {
        for flips in [false, true] {
            let mut shell = Shell::open(0);
            let mut frontend = Harness::builder(BENCH_SURFACE)
                .system_scale(BENCH_DPR)
                .depth(Depth::Compose)
                .build();
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
                    let report = frontend.frame(|ui| backdrops(ui, &mut shell, look, under));
                    if report.paint() == FramePaint::Skip {
                        frontend.paint_full();
                    }
                });
        }
    }
}
