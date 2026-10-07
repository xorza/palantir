//! Overlap-index benchmarks for the composer's rect grid: is the tiled index the right structure and are its two constants at their optima (inside a whole frame it moves only 1–3%).
//!
//!
//! Two workloads at opposite ends:
//!
//! - `realistic`: ~70–200 label-sized rects, two quad probes each, a third surviving the union pre-reject; decides `TILE_SIZE` and `TILE_CAP`.
//! - `saturated`: tiles filled past `TILE_CAP` plus wide rects spanning all of them, so every query walks a chain.

#![expect(
    clippy::print_stderr,
    reason = "a bench reports what criterion does not measure to the terminal"
)]

use crate::bench::Run;
use crate::primitives::geometry::urect::URect;
use crate::renderer::frontend::composer::rect_grid::{RectGrid, TILE_CAP, TILE_SIZE};
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::UVec2;
use std::hint::black_box;
use std::time::Duration;

/// The pathology the overflow chains exist for and no real frame reaches: a row of tiles each over [`TILE_CAP`], plus wide rects spanning all of them, reachable only through the chains.
#[derive(Debug)]
struct SaturatedFixture {
    grid: RectGrid,
    tiles: u32,
    wide: u32,
}

impl SaturatedFixture {
    /// `tiles` saturated tiles across, `wide` rects spanning all of them.
    fn new(tiles: u32, wide: u32) -> Self {
        let mut fixture = Self {
            grid: RectGrid::default(),
            tiles,
            wide,
        };
        fixture
            .grid
            .start_frame(UVec2::new(tiles * TILE_SIZE, TILE_SIZE));
        fixture.register();
        fixture
    }

    /// Fill each tile past capacity, then lay the spanning rects in the y-band the small ones leave free.
    fn register(&mut self) {
        for tx in 0..self.tiles {
            for i in 0..(TILE_CAP as u32 + 2) {
                self.grid.push(URect::new(tx * TILE_SIZE + 1, i * 3, 8, 2));
            }
        }
        for i in 0..self.wide {
            self.grid
                .push(URect::new(0, TILE_SIZE / 2 + i, self.tiles * TILE_SIZE, 1));
        }
    }

    /// One compose-shaped round: rebuild the batch, then one overlap query per tile; returns the hit count so nothing is elided.
    fn round(&mut self) -> usize {
        self.grid.clear();
        self.register();
        let mut hits = 0;
        for tx in 0..self.tiles {
            for y in 0..TILE_SIZE {
                if self
                    .grid
                    .any_overlap(URect::new(tx * TILE_SIZE + 2, y, 4, 1))
                {
                    hits += 1;
                }
            }
        }
        hits
    }
}

/// The realistic counterpart, shaped from the instrumented `frame/*_cpu` arms: ~70–200 label-sized rects live at once, ~2 quad queries per rect, about a third surviving the union pre-reject.
#[derive(Debug)]
struct RealisticFixture {
    grid: RectGrid,
    viewport: UVec2,
    texts: Vec<URect>,
    queries: Vec<URect>,
}

impl RealisticFixture {
    /// `labels` text rects in rows of columns across 1920×1080; two quad-sized probes per label, half at a label (hit path, exits early), half at row gaps (miss path).
    fn new(labels: u32) -> Self {
        let viewport = UVec2::new(1920, 1080);
        let cols = 6;
        let row_h = 24;
        let mut texts = Vec::new();
        for i in 0..labels {
            let col = i % cols;
            let row = i / cols;
            texts.push(URect::new(
                16 + col * 310,
                8 + (row * row_h) % (viewport.y - row_h),
                120,
                14,
            ));
        }
        let mut queries = Vec::new();
        for (i, t) in texts.iter().enumerate() {
            queries.push(URect::new(t.min.x, t.min.y, 140, 18));
            queries.push(URect::new(
                t.min.x + (i as u32 % 7) * 13,
                t.min.y + 16,
                60,
                6,
            ));
        }
        Self {
            grid: RectGrid::default(),
            viewport,
            texts,
            queries,
        }
    }

    /// One frame: reset, register every text rect, run every query. Returns the hit count so nothing is elided and a variant that stops finding overlaps fails loudly.
    fn round(&mut self) -> usize {
        self.grid.start_frame(self.viewport);
        for &t in &self.texts {
            self.grid.push(t);
        }
        let mut hits = 0;
        for &q in &self.queries {
            if self.grid.any_overlap(q) {
                hits += 1;
            }
        }
        hits
    }
}

pub(crate) fn bench(c: &mut Criterion, run: Run<'_>) {
    // Label-sized text rects and quad-sized probes in the proportions the instrumented arms showed.
    let mut group = run.subgroup(c, "realistic");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for labels in [64u32, 200, 600] {
        let mut fixture = RealisticFixture::new(labels);
        let hits = fixture.round();
        assert!(hits > 0, "fixture must produce hits to be meaningful");
        group.throughput(Throughput::Elements(u64::from(labels)));
        group.bench_with_input(BenchmarkId::from_parameter(labels), &labels, |b, _| {
            b.iter(|| black_box(fixture.round()));
        });
    }
    group.finish();

    // Overflow length is the secondary metric; per-round wall time decides.
    let mut group = run.subgroup(c, "saturated");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for (tiles, wide) in [(8u32, 16u32), (16, 32)] {
        let mut fixture = SaturatedFixture::new(tiles, wide);
        fixture.round();
        eprintln!(
            "[rect_grid] tiles={tiles} wide={wide} overflow={}",
            fixture.grid.overflow.len(),
        );
        group.throughput(Throughput::Elements(u64::from(tiles * wide)));
        group.bench_with_input(
            BenchmarkId::new(format!("{tiles}x{wide}"), tiles),
            &tiles,
            |b, _| b.iter(|| black_box(fixture.round())),
        );
    }
    group.finish();
}
