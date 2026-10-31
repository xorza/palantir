//! High-water bytes of the scratch no frame reads from an earlier one, phase by phase, over the frame fixture: what the buffers hold apart (their sum) against the least one arena reset between phases would need (the largest phase; growth leaves more).
//!
//! Leaves out text shaping's scratch, which the shared shaper uses both in layout and when the backend rasterizes glyphs, and the GPU backend's, as the harness opens no device.

use crate::host::harness::Harness;
use crate::host::harness::harness_builder::Depth;
use crate::internals::frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE, FrameFixture};
use glam::{UVec2, Vec2};
use std::time::Duration;

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
            .scale(BENCH_DPR)
            .depth(Depth::Compose)
            .build();
        let mut state = FrameFixture::default();
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
            nodes: h.ui_ref().forest().total_nodes(),
            phases: [
                ("record", h.ui_ref().forest().scratch_bytes()),
                ("layout", engines.layout.scratch.scratch_bytes()),
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
    for scale in [1, 8, BENCH_SCALE] {
        Report::run(scale).print();
    }
}
