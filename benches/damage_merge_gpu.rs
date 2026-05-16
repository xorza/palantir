//! GPU-side damage merge bench. Drives `Ui::run_frame` ➔
//! `Host::render` ➔ `device.poll(Wait)` so the criterion
//! timing window includes the actual GPU pass cost (pipeline state
//! setup, scissor changes, pixel shading, copy-to-surface).
//!
//! Question being measured: when two damage rects are nearby but
//! disjoint, is it cheaper to keep them as separate render passes
//! (N pass setups, no overdraw) or merge them into one bbox (1 pass
//! setup, gap-area overdraw)? Sweeps `gap` (column distance between
//! two flipping cells in a 32×32 grid) and reports `separate` vs
//! `merged` strategies side-by-side at each gap.
//!
//! DamageEngine region is forced via
//! `support::internals::force_frame_damage_to_rects`, bypassing the
//! production merge policy so the same scene is submitted with
//! either strategy. Cell-rect computation hard-codes the grid
//! geometry (matches `build_grid` below); the rect is padded by 2 px
//! to guarantee leaf paints intersect the damage filter.
//!
//! The crossover gap value (where `merged` time crosses `separate`
//! time) is the right `pass_cost / pixel_cost` ratio for the active
//! GPU + driver. See `docs/roadmap/damage-merge-research.md` for
//! how to interpret the numbers.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use palantir::host::test_support::{
    cpu_frame as host_cpu_frame, render_to_texture as host_render_to_texture,
};
use palantir::{Background, Color, Configure, Display, Frame, Host, Panel, Rect, Sizing, Ui};
use std::hint::black_box;
use std::time::Duration;

const SURFACE: glam::UVec2 = glam::UVec2::new(1280, 800);
const SCALE: f32 = 2.0;
const COLS: usize = 32;
const ROWS: usize = 32;
const CELL_W: f32 = 30.0;
const CELL_H: f32 = 20.0;
const GAP: f32 = 2.0;
const PADDING: f32 = 4.0;

/// Headless wgpu setup. Surface texture stands in for the swapchain;
/// `Host` renders into its own backbuffer then copies to this
/// texture (same path the windowed examples take). No present, no
/// winit.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    surface_tex: wgpu::Texture,
}

fn init_gpu() -> Gpu {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("request adapter (headless)");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("damage_merge_gpu.device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("request device");
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let surface_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("damage_merge_gpu.surface"),
        size: wgpu::Extent3d {
            width: SURFACE.x,
            height: SURFACE.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    Gpu {
        device,
        queue,
        format,
        surface_tex,
    }
}

fn build_grid<T>(ui: &mut Ui<T>, hot: &[usize], hot_color: Color) {
    Panel::vstack()
        .id_salt("root")
        .gap(GAP)
        .padding(PADDING)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for r in 0..ROWS {
                Panel::hstack()
                    .id_salt(("row", r))
                    .gap(GAP)
                    .size((Sizing::FILL, Sizing::Fixed(CELL_H)))
                    .show(ui, |ui| {
                        for c in 0..COLS {
                            let i = r * COLS + c;
                            let fill = if hot.contains(&i) {
                                hot_color
                            } else {
                                Color::rgb(0.2, 0.2, 0.25)
                            };
                            Frame::new()
                                .id_salt(("cell", r, c))
                                .size((Sizing::Fixed(CELL_W), Sizing::FILL))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            }
        });
}

/// Cell logical-px rect. Geometry mirrors `build_grid` exactly;
/// padded by 1 px on each side so the damage filter
/// (`region.any_intersects(rect)`) reliably accepts the cell's leaf
/// even with floating-point slop in the layout pass.
fn cell_logical_rect(row: usize, col: usize) -> Rect {
    let x = PADDING + col as f32 * (CELL_W + GAP);
    let y = PADDING + row as f32 * (CELL_H + GAP);
    Rect::new(x - 1.0, y - 1.0, CELL_W + 2.0, CELL_H + 2.0)
}

/// Run one frame end-to-end (record → encode → compose → submit →
/// GPU sync). `device.poll(Wait)` blocks until the GPU has finished
/// the queued submit, so criterion's wall-clock window includes the
/// pass cost. Returns nothing; the bench just times this call.
fn render_frame(
    host: &mut Host,
    gpu: &Gpu,
    display: Display,
    cells: &[usize],
    color: Color,
    forced_damage: Option<&[Rect]>,
) {
    let mut report = host_cpu_frame(host, display, &mut (), |ui| build_grid(ui, cells, color));
    if let Some(rects) = forced_damage {
        report.force_damage_to_rects(rects, color);
    }
    host_render_to_texture(host, &gpu.surface_tex, &report);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll wait");
}

fn warm(host: &mut Host, gpu: &Gpu, display: Display, cells: &[usize], cold: Color, hot: Color) {
    let mut toggle = false;
    for _ in 0..3 {
        toggle = !toggle;
        let color = if toggle { hot } else { cold };
        render_frame(host, gpu, display, cells, color, None);
    }
}

/// Sweep gap (column distance between two flipping cells in row 0)
/// and compare `separate` (two-pass) vs `merged` (one-pass-with-bbox)
/// strategies. The crossover gap is the metric we want.
fn bench_two_cells(c: &mut Criterion) {
    let gpu = init_gpu();
    let display = Display::from_physical(SURFACE, SCALE);
    let cold = Color::rgb(0.2, 0.4, 0.8);
    let hot = Color::rgb(0.9, 0.4, 0.2);

    let mut host = Host::new(gpu.device.clone(), gpu.queue.clone(), gpu.format);

    let mut group = c.benchmark_group("damage/merge_gpu/two_cells");

    for &gap in &[1usize, 2, 4, 8, 16, 24, 31] {
        let cells = [0usize, gap];
        warm(&mut host, &gpu, display, &cells, cold, hot);

        let cell_a = cell_logical_rect(0, 0);
        let cell_b = cell_logical_rect(0, gap);
        let bbox = cell_a.union(cell_b);
        let separate = [cell_a, cell_b];
        let merged = [bbox];

        for (label, region) in [("separate", &separate[..]), ("merged", &merged[..])] {
            let mut toggle = false;
            group.bench_with_input(BenchmarkId::new(label, gap), &gap, |b, _| {
                b.iter(|| {
                    toggle = !toggle;
                    let color = if toggle { hot } else { cold };
                    render_frame(&mut host, &gpu, display, &cells, color, Some(region));
                    black_box(&gpu.surface_tex);
                });
            });
        }
    }

    group.finish();
}

/// Single-pass scaling baseline: how does total time scale with
/// covered area when there's exactly one render pass? Sweeps the
/// merged-bbox area by extending the gap. Lets us pull `pixel_cost`
/// out of the `merged` curve in `bench_two_cells` (slope of time
/// vs covered area) so the `pass_cost` term in the cost model has
/// a numeric foundation.
fn bench_single_pass_scaling(c: &mut Criterion) {
    let gpu = init_gpu();
    let display = Display::from_physical(SURFACE, SCALE);
    let cold = Color::rgb(0.2, 0.4, 0.8);
    let hot = Color::rgb(0.9, 0.4, 0.2);

    let mut host = Host::new(gpu.device.clone(), gpu.queue.clone(), gpu.format);

    let mut group = c.benchmark_group("damage/merge_gpu/single_pass_scaling");

    // Always one cell flipping; the damage rect grows from a tight
    // single-cell bbox to most of the row to a row-spanning rect.
    // Pixel cost scales (roughly) linearly with damage area; the
    // intercept of the regression is the pass-setup cost.
    let cells = [0usize];
    warm(&mut host, &gpu, display, &cells, cold, hot);
    let cell = cell_logical_rect(0, 0);

    for &area_cells in &[1usize, 2, 4, 8, 16, 31] {
        let synthetic = Rect::new(
            cell.min.x,
            cell.min.y,
            cell.size.w + (area_cells as f32 - 1.0) * (CELL_W + GAP),
            cell.size.h,
        );
        let region = [synthetic];
        let mut toggle = false;
        group.bench_with_input(
            BenchmarkId::new("damage_width_cells", area_cells),
            &area_cells,
            |b, _| {
                b.iter(|| {
                    toggle = !toggle;
                    let color = if toggle { hot } else { cold };
                    render_frame(&mut host, &gpu, display, &cells, color, Some(&region));
                    black_box(&gpu.surface_tex);
                });
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(50)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(3));
    targets = bench_two_cells, bench_single_pass_scaling
}
criterion_main!(benches);
