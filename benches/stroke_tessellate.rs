//! Stroke-tessellation throughput. Covers the three color modes,
//! the cap × join matrix, hairline / mid / fat widths, and a short-
//! polyline batch case representative of UI scenes with many small
//! strokes. Vert + index scratch lives outside the iter loop so the
//! measurement is dominated by tessellation, not allocation.

use criterion::{Criterion, criterion_group, criterion_main};
use glam::Vec2;
use palantir::support::internals::{TessColorMode, TessStyle, tessellate_polyline_for_bench};
use palantir::{Color, LineCap, LineJoin, MeshVertex};
use std::hint::black_box;

fn zigzag(n: usize, period: f32, amplitude: f32) -> Vec<Vec2> {
    (0..n)
        .map(|i| {
            let x = i as f32 * period;
            let y = if i & 1 == 0 { 0.0 } else { amplitude };
            Vec2::new(x, y)
        })
        .collect()
}

fn smooth_curve(n: usize, dx: f32) -> Vec<Vec2> {
    (0..n)
        .map(|i| {
            let t = i as f32 * dx;
            Vec2::new(t, (t * 0.05).sin() * 50.0)
        })
        .collect()
}

fn red() -> Color {
    Color::rgba(1.0, 0.0, 0.0, 1.0)
}
fn green() -> Color {
    Color::rgba(0.0, 1.0, 0.0, 1.0)
}

fn run(
    points: &[Vec2],
    colors: &[Color],
    style: TessStyle,
    verts: &mut Vec<MeshVertex>,
    indices: &mut Vec<u16>,
) {
    verts.clear();
    indices.clear();
    tessellate_polyline_for_bench(black_box(points), black_box(colors), style, verts, indices);
    black_box(verts.len());
    black_box(indices.len());
}

fn style(mode: TessColorMode, cap: LineCap, join: LineJoin, width_phys: f32) -> TessStyle {
    TessStyle {
        mode,
        cap,
        join,
        width_phys,
    }
}

fn bench_tessellate(c: &mut Criterion) {
    let mut group = c.benchmark_group("stroke_tessellate");

    let mut verts: Vec<MeshVertex> = Vec::with_capacity(64_000);
    let mut indices: Vec<u16> = Vec::with_capacity(192_000);

    // Smooth 1000-pt curve: every interior is a shallow miter →
    // single-cross-section merged path.
    let smooth_1k = smooth_curve(1000, 1.0);
    let smooth_1k_per_pt: Vec<Color> = (0..1000)
        .map(|i| {
            let t = i as f32 / 999.0;
            Color::rgba(t, 1.0 - t, 0.5, 1.0)
        })
        .collect();
    let smooth_1k_per_seg: Vec<Color> = (0..999)
        .map(|i| {
            let t = i as f32 / 998.0;
            Color::rgba(t, 1.0 - t, 0.5, 1.0)
        })
        .collect();

    group.bench_function("smooth1k/single/butt_miter/w2", |b| {
        b.iter(|| {
            run(
                &smooth_1k,
                &[red()],
                style(TessColorMode::Single, LineCap::Butt, LineJoin::Miter, 2.0),
                &mut verts,
                &mut indices,
            )
        })
    });
    group.bench_function("smooth1k/per_point/butt_miter/w2", |b| {
        b.iter(|| {
            run(
                &smooth_1k,
                &smooth_1k_per_pt,
                style(TessColorMode::PerPoint, LineCap::Butt, LineJoin::Miter, 2.0),
                &mut verts,
                &mut indices,
            )
        })
    });
    group.bench_function("smooth1k/per_segment/butt_miter/w2", |b| {
        b.iter(|| {
            run(
                &smooth_1k,
                &smooth_1k_per_seg,
                style(
                    TessColorMode::PerSegment,
                    LineCap::Butt,
                    LineJoin::Miter,
                    2.0,
                ),
                &mut verts,
                &mut indices,
            )
        })
    });

    // Sharp zigzag, 200 points. Every interior is a sharp join →
    // bevel chrome + concave fill. Stresses the dual path.
    let zigzag_200 = zigzag(200, 1.0, 0.5);
    group.bench_function("zigzag200/single/butt_miter/w2", |b| {
        b.iter(|| {
            run(
                &zigzag_200,
                &[red()],
                style(TessColorMode::Single, LineCap::Butt, LineJoin::Miter, 2.0),
                &mut verts,
                &mut indices,
            )
        })
    });

    // Fat smooth curve with round join + round cap — exercises
    // the round-fan path; `round_segments` returns max=16 here.
    group.bench_function("smooth1k/single/round_round/w16", |b| {
        b.iter(|| {
            run(
                &smooth_1k,
                &[red()],
                style(TessColorMode::Single, LineCap::Round, LineJoin::Round, 16.0),
                &mut verts,
                &mut indices,
            )
        })
    });

    // Hairline regime: alpha-scaled, frozen at 1 px.
    group.bench_function("smooth1k/single/butt_miter/hairline", |b| {
        b.iter(|| {
            run(
                &smooth_1k,
                &[red()],
                style(TessColorMode::Single, LineCap::Butt, LineJoin::Miter, 0.3),
                &mut verts,
                &mut indices,
            )
        })
    });

    // Many short polylines accumulated into one scratch
    // (composer-style call pattern).
    let short = smooth_curve(10, 5.0);
    group.bench_function("short_x100/single/butt_miter/w2", |b| {
        b.iter(|| {
            verts.clear();
            indices.clear();
            for _ in 0..100 {
                tessellate_polyline_for_bench(
                    black_box(&short),
                    black_box(&[red()]),
                    style(TessColorMode::Single, LineCap::Butt, LineJoin::Miter, 2.0),
                    &mut verts,
                    &mut indices,
                );
            }
            black_box(verts.len());
            black_box(indices.len());
        })
    });

    // PerSegment same-color — exercises the trailing == leading
    // merge at every interior join (should match Single's cost).
    let red_per_seg = vec![red(); 999];
    group.bench_function("smooth1k/per_segment_same_color/butt_miter/w2", |b| {
        b.iter(|| {
            run(
                &smooth_1k,
                &red_per_seg,
                style(
                    TessColorMode::PerSegment,
                    LineCap::Butt,
                    LineJoin::Miter,
                    2.0,
                ),
                &mut verts,
                &mut indices,
            )
        })
    });

    // PerSegment alternating — defeats the merge, two cross-
    // sections at every interior join.
    let alt_per_seg: Vec<Color> = (0..999)
        .map(|i| if i & 1 == 0 { red() } else { green() })
        .collect();
    group.bench_function("smooth1k/per_segment_alt/butt_miter/w2", |b| {
        b.iter(|| {
            run(
                &smooth_1k,
                &alt_per_seg,
                style(
                    TessColorMode::PerSegment,
                    LineCap::Butt,
                    LineJoin::Miter,
                    2.0,
                ),
                &mut verts,
                &mut indices,
            )
        })
    });

    group.finish();
}

criterion_group!(benches, bench_tessellate);
criterion_main!(benches);
