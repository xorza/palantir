//! High-water bytes of the scratch no frame reads from an earlier one, phase by phase, over a screen of form rows: what the buffers hold apart (their sum) against the least one arena reset between phases would need (the largest phase; growth leaves more).
//!
//! Leaves out text shaping's scratch, which the shared shaper uses both in layout and when the backend rasterizes glyphs, and the GPU backend's, as the harness opens no device.

use crate::bench::scene::{BENCH_DPR, BENCH_SURFACE};
use crate::fmt;
use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::sizing::Sizing;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use glam::{UVec2, Vec2};
use std::time::Duration;

/// Form rows the largest report records, about 1,000 nodes.
const ROWS_MAX: usize = 256;

/// What the screen's frames change: a transform and a counter.
#[derive(Debug, Default)]
struct Screen {
    scroll_offset: Vec2,
    tick: u32,
    checked: bool,
}

impl Screen {
    fn render(&mut self, scale: usize, ui: &mut Ui) {
        Panel::vstack()
            .id_salt("scratch-screen")
            .size((Sizing::FILL, Sizing::FILL))
            .transform(TranslateScale::from_translation(self.scroll_offset))
            .show(ui, |ui| {
                Text::new(fmt!(ui, "Frame {:08}", self.tick))
                    .id_salt("scratch-counter")
                    .show(ui);
                for row in 0..scale * ROWS_MAX / 32 {
                    Panel::hstack()
                        .id_salt(("scratch-row", row))
                        .show(ui, |ui| {
                            Text::new("label").id_salt("label").show(ui);
                            Button::new().id_salt("button").label("go").show(ui);
                            Checkbox::new(&mut self.checked).id_salt("check").show(ui);
                        });
                }
            });
    }
}

/// The heap bytes `v` holds, used or not: a retained buffer keeps its high water as capacity.
pub(crate) fn vec_bytes<T>(v: &Vec<T>) -> usize {
    v.capacity() * size_of::<T>()
}

/// The bench surface, then a 4K one at the same scale factor, so the tile grids reach a large display's high water.
const SURFACES: [UVec2; 2] = [BENCH_SURFACE, UVec2::new(3840, 2160)];

/// Frames on each surface: the first lays out from nothing, then a scroll step (incremental cascade, full repaint) and a counter tick (partial damage) alternate.
const FRAMES_PER_SURFACE: u32 = 8;

#[derive(Debug)]
struct Report {
    scale: usize,
    nodes: usize,
    /// `(phase, bytes)` in frame order.
    phases: [(&'static str, usize); 5],
}

impl Report {
    fn run(scale: usize) -> Self {
        let mut h = Harness::builder(BENCH_SURFACE)
            .system_scale(BENCH_DPR)
            .depth(Depth::Compose)
            .build();
        let mut state = Screen::default();
        let mut time = Duration::ZERO;
        for surface in SURFACES {
            h.resize(surface);
            for n in 0..FRAMES_PER_SURFACE {
                if n % 2 == 0 {
                    state.scroll_offset += Vec2::new(1.5, 0.7);
                } else {
                    state.tick = state.tick.wrapping_add(1);
                }
                time += Duration::from_millis(16);
                h.at(time);
                h.frame(|ui| state.render(scale, ui));
            }
        }
        let engines = h.engines();
        Self {
            scale,
            nodes: h.ui().forest().total_nodes(),
            phases: [
                ("record", h.ui().forest().scratch_bytes()),
                ("layout", engines.layout.scratch_bytes()),
                ("cascade", engines.cascade.scratch_bytes()),
                ("damage", engines.damage.scratch_bytes()),
                ("encode+compose", h.frontend().scratch_bytes()),
            ],
        }
    }

    fn print(&self) {
        let row = |name: &str, bytes: usize| {
            println!(
                "  {name:<15} {bytes:>10} B {:>9.1} KiB",
                bytes as f64 / 1024.0
            );
        };
        println!("scale {}, {} nodes", self.scale, self.nodes);
        for (name, bytes) in self.phases {
            row(name, bytes);
        }
        let sum = self.phases.iter().map(|(_, bytes)| bytes).sum();
        let max = self.phases.iter().map(|(_, bytes)| *bytes).max().unwrap();
        row("sum", sum);
        row("largest phase", max);
        row("sum - largest", sum - max);
    }
}

#[test]
#[ignore = "print-only"]
fn print_scratch_high_water() {
    println!();
    for scale in [1, 8, 32] {
        Report::run(scale).print();
    }
}
