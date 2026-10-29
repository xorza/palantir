//! The backdrop blur's passes on the device: each job's capture through its tents to the last
//! level, then the residual Gaussian across and down into the job's slot of the destination.

use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::pipeline::pipeline_recipe::{self, PipelineRecipe};
use crate::gpu::pipeline::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::surface::render_target::{self, TargetFormat};
use crate::primitives::geometry::urect::URect;
use crate::shape::paint::blur_plan::BlurPlan;
use glam::{IVec2, UVec2};
use std::ops::Range;

#[cfg(feature = "internals")]
pub(crate) mod bench;

/// The format of every level past the capture and of the blurred result: float, so a blur keeps
/// the energy of bright detail and a 16-bit mantissa leaves no band an 8-bit target can show.
pub(crate) const LEVEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Vectors of four the residual's taps fill in [`BlurParams`].
const TAP_VEC4S: usize = BlurPlan::MAX_TAPS.div_ceil(4);

/// One pass's parameters, as `blur.wgsl` declares `BlurParams`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurParams {
    target_to_lattice: IVec2,
    level_size: IVec2,
    source_origin: IVec2,
    source_max: IVec2,
    radius: i32,
    pad: [i32; 3],
    taps: [[f32; 4]; TAP_VEC4S],
}

/// A [`BlurParams`] at its dynamic offset: WebGPU caps the offset alignment at 256 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct ParamsSlot {
    params: BlurParams,
    pad: [u8; PARAMS_SLOT_BYTES - size_of::<BlurParams>()],
}

const PARAMS_SLOT_BYTES: usize = 256;
const _: () = assert!(size_of::<ParamsSlot>() == PARAMS_SLOT_BYTES);

/// One blur: what it blurs, where, and where its result lands.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BlurJob {
    pub(crate) plan: BlurPlan,
    /// The surface the capture belongs to, in physical px.
    pub(crate) surface: UVec2,
    /// The capture's format, the surface's.
    pub(crate) format: TargetFormat,
    /// The physical px of the surface the capture copies, which the blur reads.
    pub(crate) footprint: URect,
    /// The footprint in texels of the plan's last level ([`BlurPlan::region`]).
    pub(crate) region: URect,
    /// Where the result's first texel lands in the destination: the region grown by the plan's
    /// apron on every side.
    pub(crate) dest_origin: UVec2,
    /// Where the residual's first axis writes the region's first texel in the scratch: apart from
    /// every job encoded with it.
    pub(crate) scratch_origin: UVec2,
    /// Where the job lies in the textures it shares, which [`BackdropBlur::prepare`] writes.
    pub(crate) layout: JobLayout,
}

/// Where [`BackdropBlur::prepare`] lays a job out: its parameters, and the origins of the capture
/// and the levels the jobs of its batch share, each just large enough for what they read.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct JobLayout {
    /// The parameter slot of the job's first pass.
    first_slot: usize,
    /// The surface px the capture's first texel holds.
    capture_origin: UVec2,
    /// The level-1 texel the levels' first texel holds: a multiple of `2^(deepest − 1)`, of the
    /// batch's deepest level, so each mip's first texel holds a whole texel of its level.
    levels_origin: UVec2,
}

impl JobLayout {
    /// The texel of level `level` the source a pass reads at that level starts at.
    const fn origin(self, level: u8) -> UVec2 {
        match level {
            0 => self.capture_origin,
            k => UVec2::new(
                self.levels_origin.x >> (k - 1),
                self.levels_origin.y >> (k - 1),
            ),
        }
    }
}

impl BlurJob {
    /// The texels the job writes in the destination.
    pub(crate) const fn dest_rect(self) -> URect {
        let apron = self.plan.apron();
        URect {
            min: self.dest_origin,
            size: UVec2::new(
                self.region.size.x + 2 * apron,
                self.region.size.y + 2 * apron,
            ),
        }
    }
}

/// A retained texture, its view, and the group binding the view as a pass's source.
#[derive(Debug)]
struct Sampled {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

/// The surface's pixels behind a batch's panes, the blur's level 0, in the surface's format, from
/// the batch's capture origin; grown to the largest batch seen, so every window shares one.
#[derive(Debug)]
struct CaptureTarget {
    format: TargetFormat,
    /// Whether the format filters, so the passes that read the capture fetch it bilinearly.
    filters: bool,
    target: Sampled,
}

/// A target the frame's blurs read their level 0 from where it is sampled, in place of a capture:
/// the group a pass reads it through, its extent, and whether its format filters.
#[derive(Clone, Debug)]
pub(crate) struct TargetSource {
    group: wgpu::BindGroup,
    size: UVec2,
    filters: bool,
}

/// What a pass reads as level 0: its group, and whether it filters.
#[derive(Clone, Copy, Debug)]
struct LevelZero<'a> {
    group: &'a wgpu::BindGroup,
    filters: bool,
}

/// The passes that read their source through one bind layout: a filtering one for the levels
/// and every capture whose format filters, a loading one for a capture whose format does not
/// (`Rgba32Float` without `FLOAT32_FILTERABLE`).
#[derive(Debug)]
struct SourcePasses {
    layout: wgpu::BindGroupLayout,
    tent: wgpu::RenderPipeline,
    horizontal: wgpu::RenderPipeline,
}

/// Levels `1..` in one texture, mip `k − 1` holding level `k` of a batch's panes from the batch's
/// levels origin, grown to the largest batch seen; with a view and a source group per mip.
#[derive(Debug)]
struct Levels {
    texture: wgpu::Texture,
    views: Vec<wgpu::TextureView>,
    groups: Vec<wgpu::BindGroup>,
}

/// The device side of the backdrop blur: its pipelines, the capture, the levels and the scratch
/// the passes write, all grown to the largest batch seen, with slack, and kept, so a steady frame
/// allocates nothing and a resize drag makes them again only now and then.
#[derive(Debug)]
pub(crate) struct BackdropBlur {
    filtered: SourcePasses,
    loaded: SourcePasses,
    vertical: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    params_layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    params_group: wgpu::BindGroup,
    /// The slots `params` holds.
    params_capacity: usize,
    /// This run's slots, one per pass, in pass order.
    slots: Vec<ParamsSlot>,
    captures: Vec<CaptureTarget>,
    levels: Option<Levels>,
    /// The residual's first axis, sized to the largest region seen.
    scratch: Option<Sampled>,
    /// The target the prepared jobs read as level 0, or `None` for their captures.
    target: Option<TargetSource>,
}

impl BackdropBlur {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.backdrop.blur.shader",
        wgsl: include_str!("blur.wgsl"),
        includes: &[],
        constants: shader_constants,
    };

    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let texture_entry = |filterable| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let filtered_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.backdrop.blur.source.filtered.bgl"),
            entries: &[
                texture_entry(true),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let loaded_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.backdrop.blur.source.loaded.bgl"),
            entries: &[texture_entry(false)],
        });
        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.backdrop.blur.params.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(size_of::<BlurParams>() as u64),
                },
                count: None,
            }],
        });
        let shader = Self::SHADER.module(device);
        let passes = |source_layout: wgpu::BindGroupLayout, tent_entry| {
            let layout = pipeline_recipe::pipeline_layout(
                device,
                "palantir.backdrop.blur.pl",
                &[Some(&source_layout), Some(&params_layout)],
            );
            let pipeline = |label, fragment_entry| {
                PipelineRecipe {
                    fragment_entry,
                    blend: None,
                    ..PipelineRecipe::color(label, &shader, &layout, LEVEL_FORMAT)
                }
                .build(device)
            };
            SourcePasses {
                tent: pipeline("palantir.backdrop.blur.tent", tent_entry),
                horizontal: pipeline("palantir.backdrop.blur.horizontal", "fs_horizontal"),
                layout: source_layout,
            }
        };
        let filtered = passes(filtered_layout, "fs_tent");
        let loaded = passes(loaded_layout, "fs_tent_loads");
        let vertical = PipelineRecipe {
            fragment_entry: "fs_vertical",
            blend: None,
            ..PipelineRecipe::color(
                "palantir.backdrop.blur.vertical",
                &shader,
                &pipeline_recipe::pipeline_layout(
                    device,
                    "palantir.backdrop.blur.pl",
                    &[Some(&filtered.layout), Some(&params_layout)],
                ),
                LEVEL_FORMAT,
            )
        }
        .build(device);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.backdrop.blur.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params_capacity = 16;
        let params = params_buffer(device, params_capacity);
        let params_group = params_group(device, &params_layout, &params);
        Self {
            filtered,
            loaded,
            vertical,
            sampler,
            params_layout,
            params,
            params_group,
            params_capacity,
            slots: Vec::new(),
            captures: Vec::new(),
            levels: None,
            scratch: None,
            target: None,
        }
    }

    fn ensure_capture(
        &mut self,
        device: &wgpu::Device,
        format: TargetFormat,
        size: UVec2,
    ) -> usize {
        let at = self
            .captures
            .iter()
            .position(|c| c.format == format)
            .unwrap_or_else(|| {
                let filters = format
                    .get()
                    .guaranteed_format_features(device.features())
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE);
                self.captures.push(CaptureTarget {
                    format,
                    filters,
                    target: self.sampled(
                        device,
                        "palantir.backdrop.capture",
                        format.get(),
                        grown(UVec2::ZERO, size, ceiling(device)),
                        filters,
                    ),
                });
                self.captures.len() - 1
            });
        let have = extent_of(&self.captures[at].target.texture);
        if have.x < size.x || have.y < size.y {
            self.captures[at].target = self.sampled(
                device,
                "palantir.backdrop.capture",
                format.get(),
                grown(have, size, ceiling(device)),
                self.captures[at].filters,
            );
        }
        at
    }

    /// Levels that hold `first` texels of level 1 and `levels` levels.
    fn ensure_levels(&mut self, device: &wgpu::Device, first: UVec2, levels: u8) {
        let mips = u32::from(levels);
        let (have, have_mips) = self.levels.as_ref().map_or((UVec2::ZERO, 0), |l| {
            (extent_of(&l.texture), l.views.len() as u32)
        });
        if have.x >= first.x && have.y >= first.y && have_mips >= mips {
            return;
        }
        let mips = mips.max(have_mips);
        // A mip floors its halving where a level's extent rounds up: a first mip a whole number of
        // the last one's texels keeps every mip as wide as its level.
        let last = 1 << (mips - 1);
        let size = grown(have, first, ceiling(device)).map(|side| side.next_multiple_of(last));
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.backdrop.levels"),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LEVEL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let views: Vec<_> = (0..mips)
            .map(|mip| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("palantir.backdrop.level"),
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let groups = views
            .iter()
            .map(|view| self.source_group(device, view, true))
            .collect();
        self.levels = Some(Levels {
            texture,
            views,
            groups,
        });
    }

    fn ensure_scratch(&mut self, device: &wgpu::Device, size: UVec2) {
        let have = self
            .scratch
            .as_ref()
            .map_or(UVec2::ZERO, |s| extent_of(&s.texture));
        if have.x >= size.x && have.y >= size.y {
            return;
        }
        self.scratch = Some(self.sampled(
            device,
            "palantir.backdrop.scratch",
            LEVEL_FORMAT,
            grown(have, size, ceiling(device)),
            true,
        ));
    }

    fn ensure_params(&mut self, device: &wgpu::Device) {
        if self.slots.len() <= self.params_capacity {
            return;
        }
        self.params_capacity = self.slots.len().next_power_of_two();
        self.params = params_buffer(device, self.params_capacity);
        self.params_group = params_group(device, &self.params_layout, &self.params);
    }

    fn sampled(
        &self,
        device: &wgpu::Device,
        label: &'static str,
        format: wgpu::TextureFormat,
        size: UVec2,
        filters: bool,
    ) -> Sampled {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.x.max(1),
                height: size.y.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = self.source_group(device, &view, filters);
        Sampled {
            texture,
            view,
            group,
        }
    }

    /// The group a pass reads `view` through, with the sampler where its format `filters`.
    fn source_group(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        filters: bool,
    ) -> wgpu::BindGroup {
        let texture = wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let sampler = wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        };
        let (layout, entries) = if filters {
            (&self.filtered.layout, &[texture, sampler][..])
        } else {
            (&self.loaded.layout, &[texture][..])
        };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.backdrop.blur.source"),
            layout,
            entries,
        })
    }

    /// Lay out `jobs` in `batches`, each the jobs [`Self::encode`] records together, make room for
    /// them and write their parameters, ahead of the copies that fill their captures and the
    /// passes.
    pub(crate) fn prepare(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        jobs: &mut [BlurJob],
        batches: impl Iterator<Item = Range<usize>>,
        target: Option<TargetSource>,
    ) {
        self.target = target;
        let mut laid_out = 0;
        for batch in batches {
            debug_assert_eq!(batch.start, laid_out, "batches cover the jobs in order");
            laid_out = batch.end;
            self.lay_out(ctx.device, &mut jobs[batch]);
        }
        debug_assert_eq!(laid_out, jobs.len(), "batches cover the jobs");
        self.write_params(ctx, jobs);
    }

    /// Give `batch` its capture and levels origins, at the first texel it reads of each, and grow
    /// the capture, the levels and the scratch to hold what it reads.
    fn lay_out(&mut self, device: &wgpu::Device, batch: &mut [BlurJob]) {
        let first = batch[0];
        let capture_origin = if self.target.is_some() {
            UVec2::ZERO
        } else {
            let footprints = batch
                .iter()
                .map(|job| job.footprint)
                .reduce(URect::union)
                .expect("a batch holds a job");
            self.ensure_capture(device, first.format, footprints.size);
            footprints.min
        };
        let deepest = batch.iter().map(|job| job.plan.levels()).max().unwrap_or(0);
        let levels_origin = batch
            .iter()
            .filter(|job| job.plan.levels() > 0)
            .map(|job| level_region(job.region, job.plan.levels(), 1, job.surface))
            .reduce(URect::union)
            .map_or(UVec2::ZERO, |window| {
                let align = 1 << (deepest - 1);
                let origin = window.min / align * align;
                self.ensure_levels(device, window.max() - origin, deepest);
                origin
            });
        let scratch = batch
            .iter()
            .map(|job| job.scratch_origin + job.region.size)
            .fold(UVec2::ZERO, UVec2::max);
        self.ensure_scratch(device, scratch);
        for job in batch {
            job.layout.capture_origin = capture_origin;
            job.layout.levels_origin = levels_origin;
        }
    }

    /// Write the parameters of laid out `jobs`, and each one's first slot.
    fn write_params(&mut self, ctx: &mut GpuCtx<'_>, jobs: &mut [BlurJob]) {
        self.slots.clear();
        if jobs.is_empty() {
            return;
        }
        for job in jobs {
            job.layout.first_slot = self.slots.len();
            self.push_params(*job);
        }
        self.ensure_params(ctx.device);
        ctx.write(&self.params, 0, bytemuck::cast_slice(&self.slots));
    }

    /// Copy prepared `job`'s footprint of `color`, the target the main pass paints, into its
    /// capture, past the batch's capture origin. Nothing where the jobs read the target itself.
    pub(crate) fn copy_capture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        job: &BlurJob,
        color: &wgpu::Texture,
    ) {
        if self.target.is_some() {
            return;
        }
        let footprint = job.footprint;
        let capture = self.capture_of(job);
        debug_assert!(
            footprint.max().x <= color.width() && footprint.max().y <= color.height(),
            "a footprint lies on its surface"
        );
        let into = footprint.min - job.layout.capture_origin;
        debug_assert!(
            into.x + footprint.size.x <= capture.target.texture.width()
                && into.y + footprint.size.y <= capture.target.texture.height(),
            "the capture holds its batch's footprints"
        );
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: color,
                mip_level: 0,
                origin: render_target::origin(footprint.min),
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &capture.target.texture,
                mip_level: 0,
                origin: render_target::origin(into),
                aspect: wgpu::TextureAspect::All,
            },
            render_target::extent(footprint.size),
        );
    }

    /// What prepared job `job` reads as level 0: the target where the frame samples it, else
    /// the capture of its format.
    fn level_zero(&self, job: &BlurJob) -> LevelZero<'_> {
        if let Some(target) = &self.target {
            return LevelZero {
                group: &target.group,
                filters: target.filters,
            };
        }
        let capture = self.capture_of(job);
        LevelZero {
            group: &capture.target.group,
            filters: capture.filters,
        }
    }

    /// The group `view`, a target of `format` the frame paints, is read through as level 0, in
    /// place of a capture.
    pub(crate) fn target_source(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        format: TargetFormat,
        size: UVec2,
    ) -> TargetSource {
        let filters = format
            .get()
            .guaranteed_format_features(device.features())
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE);
        TargetSource {
            group: self.source_group(device, view, filters),
            size,
            filters,
        }
    }

    /// Prepared job `job`'s capture, the one of its format.
    fn capture_of(&self, job: &BlurJob) -> &CaptureTarget {
        let format = job.format;
        self.captures
            .iter()
            .find(|c| c.format == format)
            .expect("a prepared job has its capture")
    }

    /// Record prepared jobs `jobs`' passes, which read one capture: their tents a level to a pass,
    /// then the residual across into the scratch and down into each job's slot of `dest`, a pass
    /// each. Every pass draws each job under its own scissor, so the jobs pay one pass a stage.
    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        jobs: &[BlurJob],
        dest: &wgpu::TextureView,
    ) {
        let first = &jobs[0];
        debug_assert!(
            jobs.iter().all(|job| job.format == first.format
                && job.surface == first.surface
                && job.layout.capture_origin == first.layout.capture_origin
                && job.layout.levels_origin == first.layout.levels_origin),
            "jobs encoded together are one batch, which reads one capture and one set of levels"
        );
        let zero = self.level_zero(first);
        let level_groups = self.levels.as_ref().map(|l| &*l.groups);
        let level_views = self.levels.as_ref().map(|l| &*l.views);
        // Level `k` reads from the capture at `k = 1`, and from mip `k − 2` above it.
        let source_of = |level: u8| match level {
            0 => zero.group,
            k => &level_groups.expect("a blurred level has its levels")[usize::from(k) - 1],
        };
        let passes_of = |level: u8| {
            if level == 0 && !zero.filters {
                &self.loaded
            } else {
                &self.filtered
            }
        };
        let deepest = jobs.iter().map(|job| job.plan.levels()).max().unwrap_or(0);
        for k in 1..=deepest {
            let view = &level_views.expect("a blurred level has its levels")[usize::from(k) - 1];
            let draws = jobs.iter().filter(|job| job.plan.levels() >= k).map(|job| {
                let written = level_region(job.region, job.plan.levels(), k, job.surface);
                Draw {
                    pipeline: &passes_of(k - 1).tent,
                    source: source_of(k - 1),
                    slot: job.layout.first_slot + usize::from(k) - 1,
                    scissor: URect {
                        min: written.min - job.layout.origin(k),
                        size: written.size,
                    },
                }
            });
            self.pass(encoder, view, draws);
        }
        let residual = |job: &BlurJob| job.layout.first_slot + usize::from(job.plan.levels());
        let scratch = self.scratch.as_ref().expect("prepared");
        let across = jobs.iter().map(|job| Draw {
            pipeline: &passes_of(job.plan.levels()).horizontal,
            source: source_of(job.plan.levels()),
            slot: residual(job),
            scissor: URect {
                min: job.scratch_origin,
                size: job.region.size,
            },
        });
        self.pass(encoder, &scratch.view, across);
        let down = jobs.iter().map(|job| Draw {
            pipeline: &self.vertical,
            source: &scratch.group,
            slot: residual(job) + 1,
            scissor: job.dest_rect(),
        });
        self.pass(encoder, dest, down);
    }

    /// The parameters of `job`'s passes, in the order [`Self::encode`] records them.
    fn push_params(&mut self, job: BlurJob) {
        let BlurJob {
            plan,
            surface,
            format,
            region,
            dest_origin,
            scratch_origin,
            layout,
            ..
        } = job;
        let levels = plan.levels();
        let capture_max = self.target.as_ref().map_or_else(
            || {
                let capture = self
                    .captures
                    .iter()
                    .find(|c| c.format == format)
                    .expect("ensured before the parameters");
                extent_of(&capture.target.texture)
            },
            |target| target.size,
        );
        let mip_max = |level: u8| {
            let levels = self
                .levels
                .as_ref()
                .expect("a blurred level has its levels");
            extent_of(&levels.texture) >> u32::from(level - 1)
        };
        let source_max = |level: u8| match level {
            0 => capture_max,
            k => mip_max(k).max(UVec2::ONE),
        };
        let mut slot = |params: BlurParams| {
            self.slots.push(ParamsSlot {
                params,
                pad: [0; PARAMS_SLOT_BYTES - size_of::<BlurParams>()],
            });
        };
        for k in 1..=levels {
            slot(BlurParams {
                target_to_lattice: layout.origin(k).as_ivec2(),
                level_size: level_extent(surface, k - 1).as_ivec2(),
                source_origin: layout.origin(k - 1).as_ivec2(),
                source_max: last_texel(source_max(k - 1)),
                ..BlurParams::EMPTY
            });
        }
        let taps = taps_of(plan);
        let radius = i32::from(plan.radius());
        let level_size = level_extent(surface, levels).as_ivec2();
        let to_scratch = region.min.as_ivec2() - scratch_origin.as_ivec2();
        slot(BlurParams {
            target_to_lattice: to_scratch,
            level_size,
            source_origin: layout.origin(levels).as_ivec2(),
            source_max: last_texel(source_max(levels)),
            radius,
            taps,
            ..BlurParams::EMPTY
        });
        let apron = plan.apron() as i32;
        // A read past the region lands in another job's rows of the scratch, outside the support
        // of every texel the pane shows.
        slot(BlurParams {
            target_to_lattice: region.min.as_ivec2() - IVec2::splat(apron) - dest_origin.as_ivec2(),
            level_size,
            source_origin: to_scratch,
            source_max: last_texel(scratch_origin + region.size),
            radius,
            taps,
            pad: [0; 3],
        });
    }

    /// One pass into `target` drawing each of `draws`.
    fn pass<'a>(
        &'a self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        draws: impl Iterator<Item = Draw<'a>>,
    ) {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.backdrop.blur"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        for draw in draws {
            let Draw {
                pipeline,
                source,
                slot,
                scissor,
            } = draw;
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, source, &[]);
            rp.set_bind_group(
                1,
                &self.params_group,
                &[(slot * PARAMS_SLOT_BYTES) as wgpu::DynamicOffset],
            );
            rp.set_scissor_rect(scissor.min.x, scissor.min.y, scissor.size.x, scissor.size.y);
            rp.draw(0..3, 0..1);
        }
    }
}

/// One job's draw in a pass.
#[derive(Clone, Copy, Debug)]
struct Draw<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    source: &'a wgpu::BindGroup,
    slot: usize,
    scissor: URect,
}

impl BlurParams {
    const EMPTY: Self = Self {
        target_to_lattice: IVec2::ZERO,
        level_size: IVec2::ONE,
        source_origin: IVec2::ZERO,
        source_max: IVec2::ZERO,
        radius: 0,
        pad: [0; 3],
        taps: [[0.0; 4]; TAP_VEC4S],
    };
}

fn taps_of(plan: BlurPlan) -> [[f32; 4]; TAP_VEC4S] {
    let mut taps = [[0.0; 4]; TAP_VEC4S];
    for (i, w) in plan.taps().into_iter().enumerate() {
        taps[i / 4][i % 4] = w;
    }
    taps
}

const fn last_texel(size: UVec2) -> IVec2 {
    IVec2::new(size.x as i32 - 1, size.y as i32 - 1)
}

/// The extent of level `level` over a surface of `surface` physical px.
const fn level_extent(surface: UVec2, level: u8) -> UVec2 {
    let lattice = 1u32 << level;
    UVec2::new(surface.x.div_ceil(lattice), surface.y.div_ceil(lattice))
}

/// `region`, in texels of level `levels`, at level `level` below it, inside that level.
fn level_region(region: URect, levels: u8, level: u8, surface: UVec2) -> URect {
    let up = u32::from(levels - level);
    URect::from_min_max(region.min << up, region.max() << up).clamp_to(URect::from_min_max(
        UVec2::ZERO,
        level_extent(surface, level),
    ))
}

fn extent_of(texture: &wgpu::Texture) -> UVec2 {
    UVec2::new(texture.width(), texture.height())
}

fn params_buffer(device: &wgpu::Device, slots: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("palantir.backdrop.blur.params"),
        size: (slots * PARAMS_SLOT_BYTES) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn params_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("palantir.backdrop.blur.params"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: params,
                offset: 0,
                size: wgpu::BufferSize::new(size_of::<BlurParams>() as u64),
            }),
        }],
    })
}

/// The texels a texture grows to that holds `have` and must hold `want`: each edge rounded up
/// to [`GROWTH_GRANULE`], held to the `ceiling` edge, so a surface that grows a few px a frame,
/// as a resize drag does, makes a new texture only every so often.
fn grown(have: UVec2, want: UVec2, ceiling: u32) -> UVec2 {
    have.max(want.map(|edge| edge.next_multiple_of(GROWTH_GRANULE).min(ceiling).max(edge)))
}

/// The largest edge of a texture `device` makes.
fn ceiling(device: &wgpu::Device) -> u32 {
    device.limits().max_texture_dimension_2d
}

/// The texels [`grown`] rounds an edge up to.
const GROWTH_GRANULE: u32 = 256;

/// The numbers [`BackdropBlur::SHADER`] takes from the Rust types that own them.
fn shader_constants() -> Vec<ShaderConstant> {
    vec![ShaderConstant::uint(
        "TAP_VEC4S",
        BlurPlan::MAX_TAPS.div_ceil(4) as u32,
    )]
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob};
    use crate::gpu::device::gpu_ctx::GpuCtx;
    use crate::gpu::surface::render_target::TargetFormat;
    use glam::UVec2;
    use std::iter;

    impl BackdropBlur {
        /// The capture a surface of `size` and `format` renders the pixels behind its panes into,
        /// grown to fit.
        pub(crate) fn capture_view(
            &mut self,
            device: &wgpu::Device,
            format: TargetFormat,
            size: UVec2,
        ) -> &wgpu::TextureView {
            let at = self.ensure_capture(device, format, size);
            &self.captures[at].target.view
        }

        /// Blur each of `jobs` from its capture into its slot of `dest`, a `LEVEL_FORMAT` target.
        /// The capture must hold the pixels of each job's region, grown by its plan's reach.
        pub(crate) fn run(
            &mut self,
            ctx: &mut GpuCtx<'_>,
            jobs: &mut [BlurJob],
            dest: &wgpu::TextureView,
        ) {
            let all = 0..jobs.len();
            self.prepare(ctx, jobs, iter::once(all), None);
            self.encode(ctx.encoder, jobs, dest);
        }
    }
}

#[cfg(test)]
mod tests;
