//! GPU image-pipeline benchmark: every workload paints the same stack of full-viewport images
//! with different `ImageFlags`:
//!
//! - `bilinear`: zero flags, the common case (plain images and `GpuView` composites).
//! - `nearest`: min+mag nearest, which still pays for the footprint measurement and texel snap.
//! - `minified_single` / `minified_mean` / `minified_peak`: a source 5× larger so the tap loop
//!   engages; `single` is the taps-off control.
//!
//! Fragment-bound by design, so shader cost moves. Criterion records the image batch's GPU
//! timestamp, not the frame's wall time.

use crate::bench::fixture::{Backends, Kind, Scale};
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::bench_gpu::{BenchGpu, BenchTarget, Timing};
use crate::host::offscreen::OffscreenHost;
use crate::internals::record_app::RecordApp;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::image::{Image, ImageDownsample, ImageFilter, ImageFit};
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::panel::Panel;
use crate::{Configure, Sizing, Vec2};
use criterion::{Criterion, Throughput};
use glam::UVec2;

const PHYSICAL: UVec2 = UVec2::new(1024, 1024);
const TEXEL: u32 = 256;
/// Tap-workload source edge: 5× the paint rect gives a 5-texel footprint and 9 taps. Not 4 or 6,
/// which sit on a `ceil` boundary where float noise would split fragments across tap counts.
const MINIFY_TEXEL: u32 = PHYSICAL.x * 5;
const LAYERS: u32 = 24;
const FRAGMENTS: u64 = (LAYERS * PHYSICAL.x * PHYSICAL.y) as u64;

#[derive(Clone, Copy, Debug)]
enum Workload {
    Bilinear,
    Nearest,
    MinifiedSingle,
    MinifiedMean,
    MinifiedPeak,
}

impl Workload {
    const ALL: [Self; 5] = [
        Self::Bilinear,
        Self::Nearest,
        Self::MinifiedSingle,
        Self::MinifiedMean,
        Self::MinifiedPeak,
    ];

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

    /// Source edge: a small texture magnifies for the filter pair; the tap workloads share a large one.
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
    let edge = workload.texel();
    let image = handle
        .get_or_insert_with(|| {
            ui.load_image(&Image::from_srgba8(UVec2::new(edge, edge), texels(edge)).unwrap())
                .expect("benchmark image fits every supported GPU")
        })
        .clone();
    let tint = if phase {
        RgbaF32::WHITE
    } else {
        RgbaF32::srgb(0.98, 0.99, 1.0)
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
    target: BenchTarget,
    handle: Option<ImageHandle>,
    phase: bool,
}

impl Fixture {
    fn new(gpu: &BenchGpu) -> Self {
        Self {
            host: gpu.plain_host(true),
            target: gpu.target("palantir.image_pipeline_bench.target", PHYSICAL),
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
        host.frame(target.as_target(), 1.0, &mut app);
        gpu.wait();
    }
}

/// `atomic/image_pipeline/gpu/<workload>`: the image batch of one frame on the device, from its
/// timestamps. Per fragment.
pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "image_pipeline", Scale::Frame);
    group.throughput(Throughput::Elements(FRAGMENTS));
    for workload in Workload::ALL {
        group.bench_function(backend.case(workload.label()), |bencher| {
            let gpu = gpu();
            let mut fixture = Fixture::new(gpu);
            // The timestamp readback lags a frame, so a few frames run before the first reads.
            let timed = (0..8).any(|_| {
                fixture.render(gpu, workload);
                gpu.poll();
                fixture
                    .host
                    .gpu_pass_stats()
                    .last_kind(BatchKind::Image)
                    .is_some()
            });
            assert!(
                timed,
                "no image batch timestamp: the adapter needs in-pass timestamps"
            );
            bencher.iter_custom(|iters| {
                (0..iters)
                    .map(|_| {
                        fixture.render(gpu, workload);
                        gpu.poll();
                        fixture
                            .host
                            .gpu_pass_stats()
                            .last_kind(BatchKind::Image)
                            .expect("an image batch timestamp every frame once one arrived")
                    })
                    .sum()
            });
        });
    }
    group.finish();
}
