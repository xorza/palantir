//! GPU image-pipeline benchmark. Every workload paints the same stack of
//! full-viewport images; they differ in the `IMG_FLAG_*` bits they ship and,
//! for the tap cases, in how large a source those bits are applied to:
//!
//! - `bilinear` sends zero flags — the common case (every plain image and
//!   every `GpuView` composite), where the fragment shader samples the UV
//!   directly.
//! - `nearest` sends min+mag nearest, the control that must keep paying for
//!   the texel-footprint measurement and the texel-center snap.
//! - `minified_single` / `minified_mean` / `minified_peak` paint a source
//!   large enough to shrink 5×, so the tap loop actually engages. The first is
//!   the control with taps off; the other two isolate what up to 9 extra
//!   fetches per fragment cost, and whether the two reductions differ.
//!
//! The scene is deliberately fragment-bound (one texture, one bind, a few
//! dozen draws, ~25M fragments) so the shader's per-fragment cost — not
//! command recording — is what moves. Each iteration toggles the tint so
//! damage repaints every layer, then waits for the GPU; the keep-or-revert
//! signal is the median image-batch timestamp printed before each case.

use crate::app::internals::RecordApp;
use crate::diagnostics::gpu_stats::BatchKind;
use crate::host::bench_gpu::{BenchGpu, Timing};
use crate::host::offscreen::OffscreenHost;
use crate::primitives::color::Color;
use crate::primitives::image::{Image, ImageDownsample, ImageFilter, ImageFit};
use crate::renderer::image_registry::ImageHandle;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::{Configure, Sizing, Vec2};
use criterion::{Criterion, Throughput};
use std::hint::black_box;
use std::time::Duration;

const PHYSICAL: glam::UVec2 = glam::UVec2::new(1024, 1024);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
/// Source texture edge for the filter workloads. Smaller than the paint rect,
/// so both run the magnification side of the filter choice.
const TEXEL: u32 = 256;
/// Source texture edge for the tap workloads: 5× the paint rect, so each
/// fragment's footprint is 5 texels and `footprint_taps` runs its middle tier
/// (`ceil(5 / 2) = 3`, so 9 taps). 5 rather than 4 or 6 because those land on
/// a `ceil` boundary, where float noise would split fragments across two tap
/// counts and widen the distribution for no reason.
const MINIFY_TEXEL: u32 = PHYSICAL.x * 5;
const LAYERS: u32 = 24;
const FRAGMENTS: u64 = (LAYERS * PHYSICAL.x * PHYSICAL.y) as u64;
/// Frames rendered before sampling starts. An integrated GPU needs a
/// long ramp before its clocks settle; a short warmup reads as noise
/// several times larger than the effect under test.
const WARMUP_FRAMES: usize = 128;
const EVIDENCE_FRAMES: usize = 256;

#[derive(Clone, Copy, Debug)]
enum Workload {
    Bilinear,
    Nearest,
    MinifiedSingle,
    MinifiedMean,
    MinifiedPeak,
}

impl Workload {
    const fn label(self) -> &'static str {
        match self {
            Self::Bilinear => "bilinear",
            Self::Nearest => "nearest",
            Self::MinifiedSingle => "minified_single",
            Self::MinifiedMean => "minified_mean",
            Self::MinifiedPeak => "minified_peak",
        }
    }

    const fn filter(self) -> ImageFilter {
        match self {
            Self::Nearest => ImageFilter::Nearest,
            _ => ImageFilter::Linear,
        }
    }

    const fn downsample(self) -> ImageDownsample {
        match self {
            Self::MinifiedMean => ImageDownsample::Mean,
            Self::MinifiedPeak => ImageDownsample::Peak,
            _ => ImageDownsample::Single,
        }
    }

    /// Source edge. The first pair magnifies a small texture, which is what
    /// the filter comparison needs; the tap workloads need the opposite, so
    /// they share a source large enough to minify. `MinifiedSingle` is their
    /// control — same texels, same footprint, taps off — so the difference
    /// between it and the other two is the tap loop and nothing else.
    const fn texel(self) -> u32 {
        match self {
            Self::Bilinear | Self::Nearest => TEXEL,
            _ => MINIFY_TEXEL,
        }
    }
}

fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::Instrumented)
}

fn target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.image_pipeline_bench.target"),
        size: wgpu::Extent3d {
            width: PHYSICAL.x,
            height: PHYSICAL.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn host(gpu: &BenchGpu) -> OffscreenHost {
    let mut host = OffscreenHost::builder(gpu.device.clone(), gpu.queue.clone())
        .collect_gpu_stats(true)
        .build();
    host.ui().theme.panel_background = None;
    host
}

fn poll(device: &wgpu::Device) {
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("device poll");
}

/// Deterministic high-frequency content: per-texel colour varies at the
/// texel level so neither filter can be short-circuited by flat regions.
fn texels(edge: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((edge * edge * 4) as usize);
    for y in 0..edge {
        for x in 0..edge {
            let checker = if (x / 4 + y / 4).is_multiple_of(2) {
                40
            } else {
                0
            };
            pixels.extend_from_slice(&[
                (x as u8).wrapping_add(checker),
                (y as u8).wrapping_add(checker),
                ((x ^ y) as u8).wrapping_add(checker),
                255,
            ]);
        }
    }
    pixels
}

fn record(ui: &mut Ui, handle: &mut Option<ImageHandle>, workload: Workload, phase: bool) {
    // One `Fixture` per workload, so this registers that workload's own source
    // edge on its first frame and reuses it after.
    let edge = workload.texel();
    let image = handle
        .get_or_insert_with(|| {
            ui.register_image(Image::from_rgba8(edge, edge, texels(edge)))
                .expect("benchmark image fits every supported GPU")
        })
        .clone();
    // Paint-only toggle: a tint the shader multiplies anyway, so damage
    // repaints every layer without changing geometry or flags.
    let tint = if phase {
        Color::WHITE
    } else {
        Color::rgb(0.98, 0.99, 1.0)
    };
    let filter = workload.filter();
    Panel::canvas()
        .id_salt("image-pipeline-bench-root")
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            for layer in 0..LAYERS {
                Panel::zstack()
                    .id_salt(("image-pipeline-bench-layer", layer))
                    .position(Vec2::ZERO)
                    .size((
                        Sizing::fixed(PHYSICAL.x as f32),
                        Sizing::fixed(PHYSICAL.y as f32),
                    ))
                    .show(ui, |ui| {
                        ui.add_shape(
                            Shape::image(image.clone())
                                .fit(ImageFit::Fill)
                                .min_filter(filter)
                                .mag_filter(filter)
                                .downsample(workload.downsample())
                                .tint(tint),
                        );
                    });
            }
        });
}

#[derive(Debug)]
struct Fixture {
    host: OffscreenHost,
    target: wgpu::Texture,
    handle: Option<ImageHandle>,
    phase: bool,
}

impl Fixture {
    fn new(gpu: &BenchGpu) -> Self {
        Self {
            host: host(gpu),
            target: target(&gpu.device),
            handle: None,
            phase: false,
        }
    }

    fn render(&mut self, gpu: &BenchGpu, workload: Workload) {
        self.phase = !self.phase;
        let Self {
            host,
            target,
            handle,
            phase,
        } = self;
        let phase = *phase;
        let mut app = RecordApp::new(|ui| record(ui, handle, workload, phase));
        host.frame_offscreen(target, 1.0, &mut app);
        poll(&gpu.device);
    }
}

/// Sorted-sample summary. The minimum is the keep-or-revert signal: an
/// integrated GPU shares power and memory bandwidth with the rest of the
/// machine, so the upper half of the distribution measures interference,
/// not the shader.
#[derive(Clone, Copy, Debug)]
struct Summary {
    min: f32,
    median: f32,
}

fn summarize(values: &mut [f32]) -> Option<Summary> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    Some(Summary {
        min: values[0],
        median: values[values.len() / 2],
    })
}

fn report_evidence(gpu: &BenchGpu, workload: Workload) {
    let mut fixture = Fixture::new(gpu);
    let mut image_ms = Vec::with_capacity(EVIDENCE_FRAMES);
    for frame in 0..EVIDENCE_FRAMES + WARMUP_FRAMES {
        fixture.render(gpu, workload);
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        if frame >= WARMUP_FRAMES
            && let Some(ms) = fixture.host.gpu_pass_stats().last_kind_ms(BatchKind::Image)
        {
            image_ms.push(ms);
        }
    }
    let stats = fixture.host.gpu_pass_stats().last_pipeline_stats();
    let fragments = stats
        .map(|pipeline| pipeline.fragment_shader_invocations.to_string())
        .unwrap_or_else(|| "n/a".to_owned());
    let summary = summarize(&mut image_ms);
    eprintln!(
        "[image_pipeline] {} layers={LAYERS} fragments={fragments} \
         image_min_ms={} image_median_ms={} pipeline={stats:?}",
        workload.label(),
        summary
            .map(|s| format!("{:.4}", s.min))
            .unwrap_or_else(|| "n/a".to_owned()),
        summary
            .map(|s| format!("{:.4}", s.median))
            .unwrap_or_else(|| "n/a".to_owned()),
    );
}

pub(crate) fn bench(c: &mut Criterion, _: crate::bench::Run<'_>) {
    let gpu = gpu();
    eprintln!(
        "[image_pipeline] adapter={} backend={:?} timestamp={} inside_pass={} pipeline_stats={}",
        gpu.info.name,
        gpu.info.backend,
        gpu.timing_features
            .contains(wgpu::Features::TIMESTAMP_QUERY),
        gpu.timing_features
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
        gpu.timing_features
            .contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
    );

    let mut group = c.benchmark_group("image_pipeline/frame_wall");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(20);
    for workload in [
        Workload::Bilinear,
        Workload::Nearest,
        Workload::MinifiedSingle,
        Workload::MinifiedMean,
        Workload::MinifiedPeak,
    ] {
        report_evidence(gpu, workload);
        let mut fixture = Fixture::new(gpu);
        for _ in 0..4 {
            fixture.render(gpu, workload);
        }
        group.throughput(Throughput::Elements(FRAGMENTS));
        group.bench_function(workload.label(), |bencher| {
            bencher.iter(|| {
                fixture.render(gpu, workload);
                black_box(fixture.host.gpu_pass_stats());
            });
        });
    }
    group.finish();
}
