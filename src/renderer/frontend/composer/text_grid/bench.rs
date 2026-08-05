//! Overlap-index benchmarks for the composer's text-rect grid.
//!
//! Split from the `composer` benches because they answer a different
//! question: not "how fast does compose run" but "is the tiled index the
//! right structure, and are its two constants at their optima". Inside a
//! whole frame the grid moves 1–3%, which no amount of averaging
//! resolves; here it is the entire measurement, tight to ±0.5%.
//!
//! Two workloads, deliberately at opposite ends:
//!
//! - `realistic` mirrors what the instrumented `frame/*_cpu` arms
//!   actually do — ~70–200 label-sized rects, two quad probes each, a
//!   third of them surviving the union pre-reject. This is the arm that
//!   decides `TILE_SIZE` and `TILE_CAP`, and the one that shows the
//!   tiled index beating a flat scan 7.0 µs to 56.7 µs at 200 labels.
//! - `saturated` is the pathology `TextRectGrid::spill` exists for and
//!   no real frame reaches: tiles filled past `TILE_CAP` plus wide rects
//!   spanning all of them, so the spill list carries the spanning rects
//!   and every query scans it linearly.

use crate::primitives::urect::URect;
use crate::renderer::frontend::composer::text_grid::{TILE_CAP, TILE_SIZE, TextRectGrid};
use criterion::{BenchmarkId, Criterion, Throughput};
use glam::UVec2;
use std::hint::black_box;
use std::time::Duration;

/// The pathology [`TextRectGrid::spill`] exists for and no real frame
/// reaches: a row of tiles each holding more than [`TILE_CAP`] rects,
/// plus wide rects spanning every one of them. The wide rects are the
/// interesting part — with all their tiles full they are reachable
/// *only* through the tile-blind spill scan, so every query pays for the
/// whole list.
#[derive(Debug)]
struct SaturatedFixture {
    grid: TextRectGrid,
    tiles: u32,
    wide: u32,
}

impl SaturatedFixture {
    /// `tiles` saturated tiles across, `wide` rects spanning all of them.
    fn new(tiles: u32, wide: u32) -> Self {
        let mut fixture = Self {
            grid: TextRectGrid::default(),
            tiles,
            wide,
        };
        fixture
            .grid
            .start_frame(UVec2::new(tiles * TILE_SIZE, TILE_SIZE));
        fixture.register();
        fixture
    }

    /// Fill each tile past capacity, then lay the spanning rects in the
    /// y-band the small ones leave free so they overlap the same
    /// saturated tiles without stacking on each other.
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

    /// One compose-shaped round: rebuild the batch, then run one overlap
    /// query per tile the way the composer probes per quad. Returns the
    /// hit count so nothing can be elided.
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

/// The realistic counterpart, shaped from what the `frame/*_cpu` arms
/// actually do (instrumented run, 25–63 M queries each): ~70–200
/// label-sized rects live at once, ~2 quad queries per rect, and roughly
/// a third of those queries surviving the union pre-reject. Those ratios
/// are what make the tile walk worth anything, and inside a whole frame
/// they move ~1% — too little to resolve. Here they are the entire
/// measurement.
#[derive(Debug)]
struct RealisticFixture {
    grid: TextRectGrid,
    viewport: UVec2,
    texts: Vec<URect>,
    queries: Vec<URect>,
}

impl RealisticFixture {
    /// `labels` text rects laid out as rows of columns across a
    /// 1920×1080 viewport, the way a dense panel of form rows or
    /// graph-node captions lands. Queries are quad-sized probes: two per
    /// label, half aimed at a label (the hit path, which exits early)
    /// and half at the gaps between rows (the miss path, which is what a
    /// linear scan pays full price for).
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
            queries.push(URect::new(t.x, t.y, 140, 18));
            queries.push(URect::new(t.x + (i as u32 % 7) * 13, t.y + 16, 60, 6));
        }
        Self {
            grid: TextRectGrid::default(),
            viewport,
            texts,
            queries,
        }
    }

    /// One frame: reset, register every text rect, then run every query.
    /// Returns the hit count so nothing can be elided — and so a variant
    /// that silently stops finding overlaps fails loudly instead of
    /// benchmarking faster.
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

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Run<'_>) {
    // Label-sized text rects and quad-sized probes in the proportions
    // the instrumented `frame/*_cpu` arms showed. This is the arm that
    // decides whether the tile walk pays for itself.
    let mut group = c.benchmark_group("text_grid/realistic");
    group.sample_size(50);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for labels in [64u32, 200, 600] {
        let mut fixture = RealisticFixture::new(labels);
        let hits = fixture.round();
        assert!(hits > 0, "fixture must produce hits to be meaningful");
        group.throughput(Throughput::Elements(labels as u64));
        group.bench_with_input(BenchmarkId::from_parameter(labels), &labels, |b, _| {
            b.iter(|| black_box(fixture.round()));
        });
    }
    group.finish();

    // Spill length prints as the secondary metric; the per-round wall
    // time is the decision metric.
    let mut group = c.benchmark_group("text_grid/saturated");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for (tiles, wide) in [(8u32, 16u32), (16, 32)] {
        let mut fixture = SaturatedFixture::new(tiles, wide);
        fixture.round();
        eprintln!(
            "[text_grid] tiles={tiles} wide={wide} spill={}",
            fixture.grid.spill.len(),
        );
        group.throughput(Throughput::Elements((tiles * wide) as u64));
        group.bench_with_input(
            BenchmarkId::new(format!("{tiles}x{wide}"), tiles),
            &tiles,
            |b, _| b.iter(|| black_box(fixture.round())),
        );
    }
    group.finish();
}
