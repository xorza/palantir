use crate::gpu::backdrop::backdrop_blur::{
    BackdropBlur, BlurJob, BlurParams, JobLayout, LEVEL_FORMAT, ResidualLoop, grown,
};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
use crate::primitives::geometry::urect::URect;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::blur_plan::internals::{mirror, reference_tent};
use glam::UVec2;
use half::f16;
use std::iter;
use std::mem::offset_of;
use std::ops::Range;
use wgpu::util::StagingBelt;

/// Odd on one side, so a level's last texel straddles the surface's edge.
const SURFACE: UVec2 = UVec2::new(97, 64);
const FORMAT: TargetFormat = TargetFormat::RGBA8_SRGB;

/// A capture format that filters, whose tents fetch bilinearly, and one that does not without
/// `FLOAT32_FILTERABLE`, whose tents load.
fn formats() -> [TargetFormat; 2] {
    [FORMAT, TargetFormat::new(wgpu::TextureFormat::Rgba32Float)]
}
/// One past each level boundary, and either side of the first: full resolution, then one, two
/// and three halvings.
const SIGMAS: [f32; 5] = [1.5, 4.0, 4.01, 10.0, 20.0];

/// Bytes of a capture with every kind of detail a blur meets: a lone bright pixel, a hard edge
/// and a thin line on a field of pseudo-random noise, each channel its own.
fn capture_pixels() -> Vec<[u8; 4]> {
    pixels_of(SURFACE)
}

/// [`capture_pixels`] over a surface of `surface` px.
fn pixels_of(surface: UVec2) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((surface.x * surface.y) as usize);
    for y in 0..surface.y {
        for x in 0..surface.x {
            let noise = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) >> 7;
            let mut px = [
                (noise & 0x7f) as u8,
                ((noise >> 8) & 0x7f) as u8,
                ((noise >> 16) & 0x7f) as u8,
                255,
            ];
            if x >= 60 {
                px[0] = 255;
            }
            if y == 20 {
                px[1] = 255;
            }
            if (x, y) == (30, 40) {
                px = [255; 4];
            }
            pixels.push(px);
        }
    }
    pixels
}

fn srgb_decode(byte: u8) -> f64 {
    let c = f64::from(byte) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// One channel of a 2D image, row-major.
#[derive(Clone, Debug)]
struct Plane {
    size: UVec2,
    values: Vec<f64>,
}

impl Plane {
    fn at(&self, x: u32, y: u32) -> f64 {
        self.values[(y * self.size.x + x) as usize]
    }

    /// `op` along every row, then along every column.
    fn separable(&self, op: impl Fn(&[f64]) -> Vec<f64>) -> Self {
        let rows: Vec<Vec<f64>> = self.values.chunks(self.size.x as usize).map(&op).collect();
        let width = rows[0].len();
        let columns: Vec<Vec<f64>> = (0..width)
            .map(|x| op(&rows.iter().map(|row| row[x]).collect::<Vec<_>>()))
            .collect();
        let height = columns[0].len();
        Self {
            size: UVec2::new(width as u32, height as u32),
            values: (0..height)
                .flat_map(|y| columns.iter().map(move |column| column[y]))
                .collect(),
        }
    }
}

/// The blurred last level, channel by channel, from the reference arithmetic.
fn reference(plan: BlurPlan, pixels: &[[u8; 4]]) -> [Plane; 4] {
    [0, 1, 2, 3].map(|c| {
        let capture = Plane {
            size: SURFACE,
            values: pixels
                .iter()
                .map(|px| {
                    if c == 3 {
                        f64::from(px[3]) / 255.0
                    } else {
                        srgb_decode(px[c])
                    }
                })
                .collect(),
        };
        let mut level = capture;
        for _ in 0..plan.levels() {
            level = level.separable(reference_tent);
        }
        level.separable(|line| plan.reference_residual(line))
    })
}

/// Run `jobs` over the capture `pixels`, into a `dest_size` destination read back whole. The
/// capture takes the jobs' format: `Rgba32Float` holds each byte's linear value.
fn blur(
    gpu: &HeadlessTestGpuLease,
    pixels: &[[u8; 4]],
    jobs: &mut [BlurJob],
    dest_size: UVec2,
) -> Vec<[f32; 4]> {
    let residual = ResidualLoop::of(&gpu.device.adapter_info());
    blur_with(gpu, residual, pixels, jobs, dest_size)
}

/// [`blur`] with the residual's loop shaped `residual`, whatever the device takes.
fn blur_with(
    gpu: &HeadlessTestGpuLease,
    residual: ResidualLoop,
    pixels: &[[u8; 4]],
    jobs: &mut [BlurJob],
    dest_size: UVec2,
) -> Vec<[f32; 4]> {
    let device = &gpu.device;
    let mut engine = BackdropBlur::with_loop(device, residual);
    let format = jobs[0].format;
    engine.capture_view(device, format, SURFACE);
    let capture = &engine.captures[0].target.texture;
    let linear: Vec<[f32; 4]> = pixels
        .iter()
        .map(|px| {
            [
                srgb_decode(px[0]) as f32,
                srgb_decode(px[1]) as f32,
                srgb_decode(px[2]) as f32,
                f32::from(px[3]) / 255.0,
            ]
        })
        .collect();
    let (bytes, texel): (&[u8], u32) = if format == FORMAT {
        (bytemuck::cast_slice(pixels), 4)
    } else {
        (bytemuck::cast_slice(&linear), 16)
    };
    gpu.queue.write_texture(
        capture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SURFACE.x * texel),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: SURFACE.x,
            height: SURFACE.y,
            depth_or_array_layers: 1,
        },
    );
    let dest = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test.backdrop.dest"),
        size: wgpu::Extent3d {
            width: dest_size.x,
            height: dest_size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LEVEL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = dest.create_view(&wgpu::TextureViewDescriptor::default());
    let mut belt = StagingBelt::new(device.clone(), 1 << 12);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut ctx = GpuCtx::new(gpu, &mut belt, &mut encoder);
        engine.run(&mut ctx, jobs, &view);
    }
    belt.finish();
    gpu.queue.submit(iter::once(encoder.finish()));
    belt.recall();
    read_f16(gpu, &dest)
}

fn read_f16(gpu: &HeadlessTestGpuLease, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    let bytes = gpu.read_texture(texture);
    let halves: &[f16] = bytemuck::cast_slice(&bytes);
    halves
        .as_chunks::<4>()
        .0
        .iter()
        .map(|t| t.map(f16::to_f32))
        .collect()
}

/// The whole surface as one job, from a capture of `format`.
fn whole(plan: BlurPlan, dest_origin: UVec2, format: TargetFormat) -> BlurJob {
    BlurJob {
        plan,
        surface: SURFACE,
        format,
        region: plan
            .region(URect::new(0, 0, SURFACE.x, SURFACE.y), SURFACE)
            .unwrap(),
        dest_origin,
        scratch_origin: UVec2::ZERO,
        footprint: URect::new(0, 0, SURFACE.x, SURFACE.y),
        layout: JobLayout::default(),
    }
}

/// The GPU passes match the reference arithmetic on every texel the job writes, its apron
/// mirrored past the surface's edges included. Each stored stage rounds to `f16`, half an ulp
/// relative, 2⁻¹¹: the levels, the scratch and the result, `levels + 2` stores of values no
/// larger than 1. The capture's sRGB decode is exact to `f32`, far inside that. A capture that
/// filters and one that does not run the tents' two paths.
#[test]
fn the_passes_match_the_reference_arithmetic() {
    let gpu = headless_test_gpu();
    let pixels = capture_pixels();
    for (format, sigma) in formats()
        .into_iter()
        .flat_map(|format| SIGMAS.map(|sigma| (format, sigma)))
    {
        let plan = BlurPlan::new(sigma);
        let job = whole(plan, UVec2::new(3, 5), format);
        let dest = job.dest_rect();
        let got = blur(&gpu, &pixels, &mut [job], dest.max());
        let want = reference(plan, &pixels);
        let tolerance = f64::from(plan.levels() + 2) * 2f64.powi(-11);
        let apron = i64::from(plan.apron());
        let level = want[0].size;
        for y in 0..dest.size.y {
            for x in 0..dest.size.x {
                let lx = mirror(i64::from(x) - apron, level.x as usize) as u32;
                let ly = mirror(i64::from(y) - apron, level.y as usize) as u32;
                let texel = got[((dest.min.y + y) * dest.max().x + dest.min.x + x) as usize];
                for (c, plane) in want.iter().enumerate() {
                    let (g, w) = (f64::from(texel[c]), plane.at(lx, ly));
                    assert!(
                        (g - w).abs() <= tolerance,
                        "{format:?} σ {sigma}: texel ({x}, {y}) channel {c} is {g}, want {w}"
                    );
                }
            }
        }
    }
}

/// The residual's two loops write the same texels bit for bit, inside the level and across its
/// mirrored edges, from a capture that filters and one that does not. Broadcom takes one loop,
/// every other vendor two.
#[test]
fn the_residuals_loops_write_the_same_texels() {
    let gpu = headless_test_gpu();
    let pixels = capture_pixels();
    for (format, sigma) in formats()
        .into_iter()
        .flat_map(|format| SIGMAS.map(|sigma| (format, sigma)))
    {
        let job = whole(BlurPlan::new(sigma), UVec2::new(3, 5), format);
        let size = job.dest_rect().max();
        let bits = |residual| {
            blur_with(&gpu, residual, &pixels, &mut [job], size)
                .into_iter()
                .map(|texel| texel.map(f32::to_bits))
                .collect::<Vec<_>>()
        };
        assert!(
            bits(ResidualLoop::One) == bits(ResidualLoop::Windowed),
            "{format:?} σ {sigma}"
        );
    }
    let info = |vendor| wgpu::AdapterInfo {
        vendor,
        ..wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Vulkan)
    };
    assert_eq!(ResidualLoop::of(&info(0x14E4)), ResidualLoop::One);
    for vendor in [0x1002, 0x10DE, 0x8086, 0x13B5, 0x5143, 0] {
        assert_eq!(ResidualLoop::of(&info(vendor)), ResidualLoop::Windowed);
    }
}

/// A region inside another gives the texels of its own pane, the region shrunk by the reach, the
/// same values bit for bit, wherever the two results and their scratch rows land, the two drawn
/// in the same passes: every read lands on the lattice's texels at a fixed phase, so the
/// arithmetic does not depend on where a pass's target sits.
#[test]
fn a_larger_region_gives_a_panes_texels_bit_for_bit() {
    let gpu = headless_test_gpu();
    let pixels = capture_pixels();
    for sigma in [1.5f32, 4.01] {
        let plan = BlurPlan::new(sigma);
        let lattice = plan.lattice();
        let reach = plan.reach().div_ceil(lattice);
        let big = whole(plan, UVec2::ZERO, FORMAT);
        let pane = URect::new(2 * reach, reach, 4, 3);
        let small = BlurJob {
            region: URect::from_min_max(
                pane.min - UVec2::splat(reach),
                pane.max() + UVec2::splat(reach),
            ),
            dest_origin: UVec2::new(1, big.dest_rect().max().y + 2),
            scratch_origin: UVec2::new(3, big.region.size.y + 1),
            ..big
        };
        let dest = big.dest_rect().union(small.dest_rect());
        let got = blur(&gpu, &pixels, &mut [big, small], dest.max());
        let width = dest.max().x;
        let apron = plan.apron();
        for y in pane.min.y..pane.max().y {
            for x in pane.min.x..pane.max().x {
                let in_big = got[((y + apron) * width + x + apron) as usize];
                let local = UVec2::new(x, y) - small.region.min + UVec2::splat(apron);
                let at = small.dest_origin + local;
                let in_small = got[(at.y * width + at.x) as usize];
                assert_eq!(in_big, in_small, "σ {sigma}: texel ({x}, {y})");
            }
        }
    }
}

/// `BlurParams` lies as `blur.wgsl` declares it, which a uniform binding reads unchecked: its
/// taps start on the 16 bytes a `vec4` array aligns to, past the radius and its pad.
#[test]
fn blur_params_lie_as_the_shader_reads_them() {
    BackdropBlur::SHADER.assert_struct_layout(
        "BlurParams",
        size_of::<BlurParams>(),
        &[
            (
                "target_to_lattice",
                offset_of!(BlurParams, target_to_lattice),
            ),
            ("level_size", offset_of!(BlurParams, level_size)),
            ("source_origin", offset_of!(BlurParams, source_origin)),
            ("source_max", offset_of!(BlurParams, source_max)),
            ("radius", offset_of!(BlurParams, radius)),
            ("taps", offset_of!(BlurParams, taps)),
        ],
    );
}

/// A texture grows each short edge to the next whole granule, 300 → 512 and 10 → 256, keeps an
/// edge it already holds, 512 for 520 × 10 grows to 768 × 256, never shrinks, and stops at the
/// ceiling, 16 300 → 16 384 rather than 16 384's next granule, or at an edge already past it.
#[test]
fn a_texture_grows_by_whole_granules_up_to_the_ceiling() {
    let ceiling = 16_384;
    for (have, want, grows_to) in [
        (UVec2::ZERO, UVec2::new(300, 10), UVec2::new(512, 256)),
        (
            UVec2::new(512, 256),
            UVec2::new(520, 10),
            UVec2::new(768, 256),
        ),
        (UVec2::new(768, 768), UVec2::new(1, 1), UVec2::new(768, 768)),
        (
            UVec2::ZERO,
            UVec2::new(16_300, 256),
            UVec2::new(16_384, 256),
        ),
        (UVec2::ZERO, UVec2::new(20_000, 1), UVec2::new(20_000, 256)),
    ] {
        assert_eq!(grown(have, want, ceiling), grows_to, "{have} for {want}");
    }
}

/// Blur `jobs` over `color` in `batches`, each batch's captures copied before its passes, as the
/// main pass breaks; every texel of the destination, rows of `width` texels.
fn blur_batches(
    gpu: &HeadlessTestGpuLease,
    color: &wgpu::Texture,
    jobs: &mut [BlurJob],
    batches: &[Range<usize>],
) -> Vec<[f32; 4]> {
    let device = &gpu.device;
    let dest_size = jobs
        .iter()
        .fold(URect::ZERO, |all, job| all.union(job.dest_rect()))
        .max();
    let dest = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test.backdrop.dest"),
        size: wgpu::Extent3d {
            width: dest_size.x,
            height: dest_size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LEVEL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = dest.create_view(&wgpu::TextureViewDescriptor::default());
    let mut engine = BackdropBlur::new(device);
    let mut belt = StagingBelt::new(device.clone(), 1 << 12);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut ctx = GpuCtx::new(gpu, &mut belt, &mut encoder);
        engine.prepare(&mut ctx, jobs, batches.iter().cloned(), None);
    }
    for batch in batches {
        for job in &jobs[batch.clone()] {
            engine.copy_capture(&mut encoder, job, color);
        }
        engine.encode(&mut encoder, &jobs[batch.clone()], &view);
    }
    belt.finish();
    gpu.queue.submit(iter::once(encoder.finish()));
    belt.recall();
    read_f16(gpu, &dest)
}

/// A pane's job copies only its footprint, into a capture and levels that start at the first
/// texel it reads, and gives the pane's texels of the whole surface's job bit for bit: the origins
/// move every read by whole texels of its level. The capture starts at the footprint, and the
/// levels at the region's first texel of level 1, `region.min · 2^(levels − 1)`, both inside a
/// surface large enough that each misses its origin. Two panes of 1 and 3 levels in one batch
/// share levels that start at a multiple of 4 of level 1, so each mip starts at a whole texel of
/// its level, and give the texels each gives alone.
#[test]
fn a_pane_blurs_from_its_footprint_alone() {
    const LARGE: UVec2 = UVec2::new(320, 256);
    let gpu = headless_test_gpu();
    let color = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test.backdrop.color"),
        size: wgpu::Extent3d {
            width: LARGE.x,
            height: LARGE.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT.get(),
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        color.as_image_copy(),
        bytemuck::cast_slice(&pixels_of(LARGE)),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(LARGE.x * 4),
            rows_per_image: None,
        },
        color.size(),
    );
    let surface = URect::new(0, 0, LARGE.x, LARGE.y);
    let pane = URect::new(150, 120, 9, 7);
    let job_of = |plan: BlurPlan, footprint: URect| BlurJob {
        plan,
        surface: LARGE,
        format: FORMAT,
        footprint,
        region: plan.region(footprint, LARGE).unwrap(),
        dest_origin: UVec2::ZERO,
        scratch_origin: UVec2::ZERO,
        layout: JobLayout::default(),
    };
    let pane_job = |plan: BlurPlan, pane: URect| {
        let reach = UVec2::splat(plan.reach());
        let footprint = URect::from_min_max(pane.min - reach, pane.max() + reach);
        job_of(plan, footprint.clamp_to(surface))
    };
    // The texels of `pane` at the last level, in the result of `job` read from `got`.
    let pane_texels = |got: &[[f32; 4]], width: u32, job: BlurJob, pane: URect| {
        let (lattice, apron) = (job.plan.lattice(), job.plan.apron());
        let mut texels = Vec::new();
        for y in pane.min.y / lattice..pane.max().y.div_ceil(lattice) {
            for x in pane.min.x / lattice..pane.max().x.div_ceil(lattice) {
                let at = job.dest_origin + UVec2::new(x, y) - job.region.min + UVec2::splat(apron);
                texels.push(got[(at.y * width + at.x) as usize]);
            }
        }
        texels
    };
    let width = |jobs: &[BlurJob]| {
        jobs.iter()
            .map(|job| job.dest_rect().max().x)
            .max()
            .unwrap()
    };
    for sigma in SIGMAS {
        let plan = BlurPlan::new(sigma);
        let whole = job_of(plan, surface);
        let alone = BlurJob {
            dest_origin: UVec2::new(0, whole.dest_rect().max().y),
            ..pane_job(plan, pane)
        };
        let mut jobs = [whole, alone];
        let got = blur_batches(&gpu, &color, &mut jobs, &[0..1, 1..2]);
        let [whole, alone] = jobs;
        assert_eq!(
            alone.layout.capture_origin, alone.footprint.min,
            "σ {sigma}"
        );
        assert_ne!(alone.layout.capture_origin, UVec2::ZERO, "σ {sigma}");
        if let Some(past_first) = plan.levels().checked_sub(1) {
            let first = alone.region.min << u32::from(past_first);
            assert_eq!(alone.layout.levels_origin, first, "σ {sigma}");
            assert_ne!(first.min_element(), 0, "σ {sigma}");
        }
        let w = width(&jobs);
        assert_eq!(
            pane_texels(&got, w, whole, pane),
            pane_texels(&got, w, alone, pane),
            "σ {sigma}"
        );
    }

    let left = URect::new(50, 40, 9, 7);
    let (shallow, deep) = (
        pane_job(BlurPlan::new(4.01), left),
        pane_job(BlurPlan::new(20.0), pane),
    );
    assert_eq!((shallow.plan.levels(), deep.plan.levels()), (1, 3));
    let deep = BlurJob {
        dest_origin: UVec2::new(0, shallow.dest_rect().max().y),
        scratch_origin: UVec2::new(0, shallow.region.size.y),
        ..deep
    };
    let mut apart = [shallow, deep];
    let got_apart = blur_batches(&gpu, &color, &mut apart, &[0..1, 1..2]);
    let mut together = [shallow, deep];
    #[expect(
        clippy::single_range_in_vec_init,
        reason = "one batch of both jobs, not the jobs' indices"
    )]
    let got_together = blur_batches(&gpu, &color, &mut together, &[0..2]);
    let levels_origin = together[0].layout.levels_origin;
    assert_eq!(levels_origin, together[1].layout.levels_origin, "one batch");
    // Level 1 starts at the shallow pane's region, (50, 40) − 22 halved: (14, 9); and at the
    // deep one's times 4: (5, 1) · 4 = (20, 4). The batch's levels start at the smaller, (14, 4),
    // floored to a multiple of 4: (12, 4).
    assert_eq!(shallow.region.min, UVec2::new(14, 9));
    assert_eq!(deep.region.min, UVec2::new(5, 1));
    assert_eq!(levels_origin, UVec2::new(12, 4));
    let w = width(&apart);
    for (job, pane) in [(0, left), (1, pane)] {
        assert_eq!(
            pane_texels(&got_apart, w, apart[job], pane),
            pane_texels(&got_together, w, together[job], pane),
            "job {job}"
        );
    }
}
