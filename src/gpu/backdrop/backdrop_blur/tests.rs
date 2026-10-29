use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob, BlurParams, LEVEL_FORMAT};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::shader_body::ShaderBody;
use crate::gpu::surface::render_target::TargetFormat;
use crate::gpu::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
use crate::primitives::geometry::urect::URect;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::blur_plan::internals::{mirror, reference_tent};
use glam::UVec2;
use half::f16;
use std::iter;
use std::mem::offset_of;
use std::sync::mpsc;
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
    let mut pixels = Vec::with_capacity((SURFACE.x * SURFACE.y) as usize);
    for y in 0..SURFACE.y {
        for x in 0..SURFACE.x {
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
    jobs: &[BlurJob],
    dest_size: UVec2,
) -> Vec<[f32; 4]> {
    let device = &gpu.device;
    let mut engine = BackdropBlur::new(device);
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
    let handles = gpu.handles();
    {
        let mut ctx = GpuCtx::new(&handles, &mut belt, &mut encoder);
        engine.run(&mut ctx, jobs, &view);
    }
    belt.finish();
    gpu.queue.submit(iter::once(encoder.finish()));
    belt.recall();
    read_f16(gpu, &dest)
}

fn read_f16(gpu: &HeadlessTestGpuLease, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    let (w, h) = (texture.width(), texture.height());
    let row = w * 8;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test.backdrop.read"),
        size: u64::from(padded * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        texture.size(),
    );
    gpu.queue.submit(iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |mapped| {
        tx.send(mapped).ok();
    });
    gpu.wait();
    rx.recv().unwrap().unwrap();
    let data = slice.get_mapped_range().unwrap();
    let mut out = Vec::with_capacity((w * h) as usize);
    for line in data.chunks_exact(padded as usize) {
        let halves: &[f16] = bytemuck::cast_slice(&line[..row as usize]);
        out.extend(halves.as_chunks::<4>().0.iter().map(|t| t.map(f16::to_f32)));
    }
    out
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
        let got = blur(&gpu, &pixels, &[job], dest.max());
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
        let got = blur(&gpu, &pixels, &[big, small], dest.max());
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
    ShaderBody::BackdropBlur.assert_struct_layout(
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
