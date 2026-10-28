//! The frame that grows an atlas side: the doubled texture, the old one blitted into it, submitted
//! and drained. Against `submit`, an empty frame on the same device, it is the one-time cost a
//! grow adds to its frame, which fixed-size pages would replace with one page's allocation.

use crate::bench::backends::Backends;
use crate::bench::kind::{Kind, Scale};
use crate::gpu::bench_gpu::{BenchGpu, Timing};
use crate::gpu::raster::raster_atlas::RasterAtlasConfig;
use crate::gpu::raster::raster_atlas::side::Side;
use crate::primitives::paint::content_type::ContentType;
use criterion::Criterion;
use std::time::{Duration, Instant};

/// The grows a tenant takes: text's eager mask grow, the mask grow only eviction pressure reaches,
/// and the largest color grow, each from its side length.
const GROWS: [(ContentType, u32); 3] = [
    (ContentType::Mask, 1024),
    (ContentType::Mask, 2048),
    (ContentType::Color, 1024),
];

fn gpu() -> &'static BenchGpu {
    BenchGpu::shared(Timing::Bare)
}

/// A `content` side at `size`, every texel written and drained, so the timed frame copies
/// initialized texels and wgpu clears only the grown texture.
fn primed_side(gpu: &BenchGpu, content: ContentType, size: u32, zeros: &[u8]) -> Side {
    let config = RasterAtlasConfig {
        label: "palantir.bench.atlas",
        initial_mask_px: size,
        initial_color_px: size,
        max_bytes: 16 << 20,
        eager_growth_bytes: 16 << 20,
    };
    let side = Side::new(&gpu.gpu.device, content, config);
    assert_eq!(side.size, size, "the device caps no side this bench builds");
    let bytes_per_row = size * content.bytes_per_pixel();
    gpu.gpu.queue.write_texture(
        side.texture.as_image_copy(),
        &zeros[..(bytes_per_row * size) as usize],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
    );
    gpu.gpu.queue.submit([]);
    gpu.wait();
    side
}

/// An encoder holding `record`'s commands, submitted and drained.
fn submit(gpu: &BenchGpu, record: impl FnOnce(&mut wgpu::CommandEncoder)) {
    let mut encoder = gpu
        .gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("palantir.bench.atlas.encoder"),
        });
    record(&mut encoder);
    gpu.gpu.queue.submit([encoder.finish()]);
    gpu.wait();
}

/// `atomic/raster_atlas/gpu/<grow_<side>/<from>|submit/empty>`: wall time from the grow to the drained
/// submit that blits it, and the same submit with nothing recorded.
pub(crate) fn atomic(c: &mut Criterion, backends: Backends) {
    let Some(backend) = backends.gpu() else {
        return;
    };
    let mut group = Kind::Atomic.group(c, "raster_atlas", Scale::Frame);
    group.bench_function(backend.case("submit/empty"), |b| {
        let gpu = gpu();
        b.iter(|| submit(gpu, |_| {}));
    });
    for (content, from) in GROWS {
        let id = format!("grow_{}/{from}", content.side_name());
        group.bench_function(backend.case(&id), |b| {
            let gpu = gpu();
            let zeros = vec![0; (from * from * content.bytes_per_pixel()) as usize];
            b.iter_custom(|iters| {
                (0..iters)
                    .map(|_| {
                        let mut side = primed_side(gpu, content, from, &zeros);
                        let start = Instant::now();
                        assert!(side.grow(&gpu.gpu.device), "a side below its ceiling grows");
                        submit(gpu, |encoder| side.record_grow_blit(encoder));
                        let elapsed = start.elapsed();
                        assert_eq!(side.size, from * 2, "the side doubled");
                        elapsed
                    })
                    .sum::<Duration>()
            });
        });
    }
    group.finish();
}
