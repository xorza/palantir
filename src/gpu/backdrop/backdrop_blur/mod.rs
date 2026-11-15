//! The backdrop blur's passes on the device: each job's capture through its tents to the last
//! level, then the residual Gaussian across and down into the job's slot of the destination.

use crate::cascade::blur_plan::BlurPlan;
use crate::common::expiry_window::ExpiryWindow;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::device::tiler_family::TilerFamily;
use crate::gpu::pipeline::effect_pipeline::backdrop_groups::ResultTexture;
use crate::gpu::resource::decaying_texture::{DecayingTexture, Extent};
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::pipeline_recipe::PipelineRecipe;
use crate::gpu::resource::shader_body::{ShaderBody, ShaderConstant};
use crate::gpu::resource::shelf_packer::ShelfPacker;
use crate::gpu::resource::staging_target::StagingTarget;
use crate::gpu::resource::texture_binding::{self, Filtering};
use crate::gpu::texel;
use crate::gpu::texel::TargetFormat;
use crate::gpu::texel::TargetSpec;
use crate::primitives::geometry::urect::URect;
use glam::{IVec2, UVec2};
use std::ops::Range;

#[cfg(feature = "internals")]
pub(crate) mod bench;

/// The format of every level past the capture and of the blurred result: float, so a blur keeps
/// the energy of bright detail and a 16-bit mantissa leaves no band an 8-bit target can show.
pub(crate) const LEVEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The shape of the residual's loop, which only speed tells apart: one loop that mirrors each
/// read where the window crosses the level's edge, or one loop for a window inside the level and
/// one that mirrors every read. At σ 4 over 2048² (`atomic/backdrop_blur/gpu/4/2048`) a VideoCore GPU takes 153 ms on one loop and 189 ms on two, the Apple laptop 14.3 ms and 5.0 ms, and a mobile AMD iGPU 3.24 ms
/// either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResidualLoop {
    One,
    Windowed,
}

impl ResidualLoop {
    /// One loop on Broadcom's VideoCore, two on every other device.
    pub(crate) fn of(info: &wgpu::AdapterInfo) -> Self {
        if TilerFamily::of(info) == Some(TilerFamily::VideoCore) {
            Self::One
        } else {
            Self::Windowed
        }
    }
}

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
    /// The texture the result lands in.
    pub(crate) dest: ResultTexture,
    /// Where the result's first texel lands in [`Self::dest`]: the region grown by the plan's
    /// apron on every side.
    pub(crate) dest_origin: UVec2,
}

/// A prepared job and where [`BackdropBlur::prepare`] laid it out, which the blur keeps for the
/// frame's copies and passes, so a job a caller hands in is input alone.
#[derive(Clone, Copy, Debug)]
struct LaidJob {
    job: BlurJob,
    layout: JobLayout,
}

/// Where [`BackdropBlur::prepare`] lays a job out: its parameters, the origins of the capture
/// and the levels the jobs of its batch share, each just large enough for what they read, and its
/// place in the scratch.
#[derive(Clone, Copy, Debug, Default)]
struct JobLayout {
    /// The parameter slot of the job's first pass.
    first_slot: usize,
    /// The surface px the capture's first texel holds.
    capture_origin: UVec2,
    /// The level-1 texel the levels' first texel holds: a multiple of `2^(deepest − 1)`, of the
    /// batch's deepest level, so each mip's first texel holds a whole texel of its level.
    levels_origin: UVec2,
    /// The round of its batch's residual the job runs in: each round packs the scratch from
    /// empty, so a batch whose regions overflow one scratch takes more rounds, never a larger
    /// scratch.
    scratch_round: u32,
    /// Where the residual's first axis writes the region's first texel in the scratch, apart from
    /// every job of its round, in a place as large as its region, or a staged job's as its
    /// [`BlurJob::dest_rect`], which its second axis takes in the staging target.
    scratch_origin: UVec2,
    /// Whether the job's second axis draws into the staging target, copied onto its page after:
    /// a result on a page, on a device whose passes move their whole target.
    staged: bool,
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

/// A texture a job's result lands in: its view, which a pass draws into, and the texture, which a
/// staged job's copy lands in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResultTarget<'v> {
    pub(crate) texture: &'v wgpu::Texture,
    pub(crate) view: &'v wgpu::TextureView,
}

/// A retained texture, its view, and the group binding the view as a pass's source.
#[derive(Debug)]
struct Sampled {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    group: wgpu::BindGroup,
}

/// The surface's pixels behind a batch's backdrops, the blur's level 0, in the surface's format,
/// from the batch's capture origin; grown to the largest batch seen, so every window shares one.
#[derive(Debug)]
struct CaptureTarget {
    format: TargetFormat,
    /// How the passes that read the capture fetch it: bilinearly where the format filters.
    filtering: Filtering,
    /// Held while a frame of the current expiry window read it; the step-down drops a capture
    /// whose texture went.
    texture: DecayingTexture<Sampled>,
}

impl CaptureTarget {
    /// The capture's texture, which a kept capture holds.
    fn target(&self) -> &Sampled {
        self.texture
            .get()
            .expect("a kept capture holds its texture")
    }
}

/// What the levels are sized by: the texels of level 1, and the count of levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LevelsExtent {
    first: UVec2,
    mips: u8,
}

impl Extent for LevelsExtent {
    const NONE: Self = Self {
        first: UVec2::ZERO,
        mips: 0,
    };

    fn union(self, other: Self) -> Self {
        Self {
            first: self.first.max(other.first),
            mips: self.mips.max(other.mips),
        }
    }

    fn holds(self, want: Self) -> bool {
        self.first.holds(want.first) && self.mips >= want.mips
    }

    /// [`levels_size`] at the larger count of levels.
    fn grown(self, want: Self, ceiling: u32) -> Self {
        let mips = self.mips.max(want.mips);
        Self {
            first: levels_size(self.first, want.first, mips, ceiling),
            mips,
        }
    }
}

/// What a frame's batches read of the textures they share, each at its largest: the capture of
/// the frame's format, the first level's texels and the levels' count, and the scratch.
#[derive(Clone, Copy, Debug, Default)]
struct BlurNeeds {
    capture: Option<(TargetFormat, UVec2)>,
    levels: LevelsExtent,
    scratch: UVec2,
    /// The staging target's, the scratch's where a job is staged, else none.
    staging: UVec2,
}

impl BlurNeeds {
    /// The larger of `self` and `other`, texture by texture.
    fn max(self, other: Self) -> Self {
        let capture = match (self.capture, other.capture) {
            (Some((format, a)), Some((other_format, b))) => {
                debug_assert_eq!(format, other_format, "a frame's captures share its format");
                Some((format, a.max(b)))
            }
            (a, b) => a.or(b),
        };
        Self {
            capture,
            levels: self.levels.union(other.levels),
            scratch: self.scratch.max(other.scratch),
            staging: self.staging.max(other.staging),
        }
    }
}

/// A target the frame's blurs read their level 0 from where it is sampled, in place of a capture:
/// the group a pass reads it through, its extent, and how its format fetches.
#[derive(Clone, Debug)]
pub(crate) struct TargetSource {
    group: wgpu::BindGroup,
    size: UVec2,
    filtering: Filtering,
}

/// What a pass reads as level 0: its group, and how it fetches.
#[derive(Clone, Copy, Debug)]
struct LevelZero<'a> {
    group: &'a wgpu::BindGroup,
    filtering: Filtering,
}

/// The passes that read their source through one bind layout: a filtering one for the levels
/// and every capture whose format filters, a loading one for a capture whose format does not
/// (`Rgba32Float` without `FLOAT32_FILTERABLE`).
#[derive(Debug)]
struct SourcePasses {
    tent: wgpu::RenderPipeline,
    horizontal: wgpu::RenderPipeline,
}

/// The bind layouts a pass reads its source through, one for each [`Filtering`], and the sampler
/// the filtered one binds.
#[derive(Debug)]
struct SourceLayouts {
    filtered: wgpu::BindGroupLayout,
    loaded: wgpu::BindGroupLayout,
    /// The backend's one sampler ([`TextureBinding::sampler`]).
    ///
    /// [`TextureBinding::sampler`]: crate::gpu::resource::texture_binding::TextureBinding::sampler
    sampler: wgpu::Sampler,
}

impl SourceLayouts {
    /// The group a pass reads `view` through, with the sampler where it is `Filtered`.
    fn group(&self, gpu: &Gpu, view: &wgpu::TextureView, filtering: Filtering) -> wgpu::BindGroup {
        let texture = wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let sampler = wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        };
        let (layout, entries) = match filtering {
            Filtering::Filtered => (&self.filtered, &[texture, sampler][..]),
            Filtering::Loaded => (&self.loaded, &[texture][..]),
        };
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("palantir.backdrop.blur.source"),
            layout,
            entries,
        })
    }

    /// A texture a blur pass renders and another samples, as `spec` describes it.
    fn sampled(&self, gpu: &Gpu, spec: SampledSpec) -> Sampled {
        let SampledSpec {
            label,
            format,
            size,
            filtering,
        } = spec;
        let texture = TargetSpec {
            label,
            size: size.max(UVec2::ONE),
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
        }
        .texture(gpu);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = self.group(gpu, &view, filtering);
        Sampled {
            texture,
            view,
            group,
        }
    }
}

/// How `format`'s texels are fetched on `gpu`: bilinearly where it filters, else one by one.
fn filtering_of(gpu: &Gpu, format: TargetFormat) -> Filtering {
    if format
        .get()
        .guaranteed_format_features(gpu.device.features())
        .flags
        .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
    {
        Filtering::Filtered
    } else {
        Filtering::Loaded
    }
}

/// Levels `1..` in one texture, mip `k − 1` holding level `k` of a batch's backdrops from the
/// batch's levels origin, grown to the largest batch seen; with a view and a source group per mip.
#[derive(Debug)]
struct Levels {
    texture: wgpu::Texture,
    views: Vec<wgpu::TextureView>,
    groups: Vec<wgpu::BindGroup>,
}

impl Levels {
    /// Levels at `extent`, a grown one, with a view and a source group per mip.
    fn new(gpu: &Gpu, sources: &SourceLayouts, extent: LevelsExtent, ceiling: u32) -> Self {
        let LevelsExtent { first: size, mips } = extent;
        debug_assert!(
            size.max_element() <= ceiling,
            "levels of {size} pass the device's {ceiling} edge"
        );
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.backdrop.levels"),
            size: texel::extent(size),
            mip_level_count: u32::from(mips),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: LEVEL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let views: Vec<_> = (0..u32::from(mips))
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
            .map(|view| sources.group(gpu, view, Filtering::Filtered))
            .collect();
        Self {
            texture,
            views,
            groups,
        }
    }
}

/// The capture of `format` at `size`, fetched by `filtering`.
const fn capture_spec(format: TargetFormat, size: UVec2, filtering: Filtering) -> SampledSpec {
    SampledSpec {
        label: "palantir.backdrop.capture",
        format: format.get(),
        size,
        filtering,
    }
}

/// The scratch at `size`.
const fn scratch_spec(size: UVec2) -> SampledSpec {
    SampledSpec {
        label: "palantir.backdrop.scratch",
        format: LEVEL_FORMAT,
        size,
        filtering: Filtering::Filtered,
    }
}

/// The device side of the backdrop blur: its pipelines, the capture, the levels and the scratch
/// the passes write, all grown to the largest batch seen, with slack, so a steady frame allocates
/// nothing and a resize drag makes them again only now and then. Each texture steps down to what
/// the last [`EXPIRY_FRAMES`](crate::common::expiry_window::EXPIRY_FRAMES) display frames read, and goes where none read it, so one full-screen backdrop
/// does not hold its textures for the session.
///
/// None of them passes the device's edge. The capture and the levels hold what lies on one
/// surface, which a target's edge bounds; the scratch holds regions packed side by side, which a
/// batch can have more of than one edge holds, so its residual runs in rounds.
#[derive(Debug)]
pub(crate) struct BackdropBlur {
    filtered: SourcePasses,
    loaded: SourcePasses,
    vertical: wgpu::RenderPipeline,
    sources: SourceLayouts,
    params_layout: wgpu::BindGroupLayout,
    params: DynamicBuffer<ParamsSlot>,
    params_group: wgpu::BindGroup,
    /// The `params` capacity `params_group` binds, which a grow or a step down replaces.
    params_bound: usize,
    /// This run's slots, one per pass, in pass order.
    slots: Vec<ParamsSlot>,
    /// The jobs the last prepare laid out, in its order, which the copies and the passes name by
    /// index.
    laid: Vec<LaidJob>,
    captures: Vec<CaptureTarget>,
    levels: DecayingTexture<Levels, LevelsExtent>,
    /// The residual's first axis, sized to the largest round seen.
    scratch: DecayingTexture<Sampled>,
    /// What the device's passes move: where a pass moves its whole target, the second axis of a
    /// result on a page draws into [`Self::staging`], copied onto the page after.
    traffic: PassTraffic,
    /// The second axis of a frame's staged jobs, at their places in the scratch.
    staging: StagingTarget,
    /// The current expiry window over the display frames, which every texture of the blur steps
    /// down at the end of.
    window: ExpiryWindow,
    /// Places a round's regions in the scratch, an edge of `ceiling` square.
    scratch_packer: ShelfPacker,
    /// The device's largest texture edge, which no texture of the blur passes.
    ceiling: u32,
    /// The target the prepared jobs read as level 0, or `None` for their captures.
    target: Option<TargetSource>,
}

/// A texture a blur pass renders and another samples, as [`SourceLayouts::sampled`] makes it.
#[derive(Clone, Copy, Debug)]
struct SampledSpec {
    label: &'static str,
    format: wgpu::TextureFormat,
    size: UVec2,
    /// How its format is fetched, which picks the group's layout.
    filtering: Filtering,
}

impl BackdropBlur {
    /// This family's shader body.
    pub(crate) const SHADER: ShaderBody = ShaderBody {
        label: "palantir.backdrop.blur.shader",
        wgsl: include_str!("blur.wgsl"),
        includes: &[],
        constants: SHADER_CONSTANTS,
    };

    /// The blur on `gpu`, its filtered passes binding `sampler`, the backend's one.
    pub(crate) fn new(gpu: &Gpu, sampler: &wgpu::Sampler) -> Self {
        Self::with_loop(gpu, sampler, ResidualLoop::of(&gpu.device.adapter_info()))
    }

    /// The blur on `gpu`, its residual's loop shaped `residual`.
    pub(crate) fn with_loop(gpu: &Gpu, sampler: &wgpu::Sampler, residual: ResidualLoop) -> Self {
        let one_loop = [(
            "ONE_LOOP",
            f64::from(u8::from(residual == ResidualLoop::One)),
        )];
        let filtered_layout =
            gpu.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("palantir.backdrop.blur.source.filtered.bgl"),
                    entries: &[
                        texture_binding::texture_entry(0, Filtering::Filtered),
                        texture_binding::sampler_entry(1),
                    ],
                });
        let loaded_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("palantir.backdrop.blur.source.loaded.bgl"),
                entries: &[texture_binding::texture_entry(0, Filtering::Loaded)],
            });
        let params_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        let shader = Self::SHADER.module(gpu);
        let pipeline_layout = |source_layout: &wgpu::BindGroupLayout| {
            gpu.pipeline_layout(
                "palantir.backdrop.blur.pl",
                &[Some(source_layout), Some(&params_layout)],
            )
        };
        let filtered_pl = pipeline_layout(&filtered_layout);
        let passes = |layout: &wgpu::PipelineLayout, tent_entry| {
            let pipeline = |label, fragment_entry, constants| {
                PipelineRecipe {
                    fragment_entry,
                    constants,
                    blend: None,
                    ..PipelineRecipe::color(label, &shader, layout, LEVEL_FORMAT)
                }
                .build(gpu)
                .pipeline
            };
            SourcePasses {
                tent: pipeline("palantir.backdrop.blur.tent", tent_entry, &[]),
                horizontal: pipeline(
                    "palantir.backdrop.blur.horizontal",
                    "fs_horizontal",
                    &one_loop,
                ),
            }
        };
        let filtered = passes(&filtered_pl, "fs_tent");
        let loaded = passes(&pipeline_layout(&loaded_layout), "fs_tent_loads");
        let vertical = PipelineRecipe {
            fragment_entry: "fs_vertical",
            constants: &one_loop,
            blend: None,
            ..PipelineRecipe::color(
                "palantir.backdrop.blur.vertical",
                &shader,
                &filtered_pl,
                LEVEL_FORMAT,
            )
        }
        .build(gpu)
        .pipeline;
        let params = DynamicBuffer::uniform(gpu, "palantir.backdrop.blur.params", 16);
        let params_group = params_group(gpu, &params_layout, &params.buffer);
        let ceiling = gpu.max_texture_dimension().get();
        Self {
            filtered,
            loaded,
            vertical,
            sources: SourceLayouts {
                filtered: filtered_layout,
                loaded: loaded_layout,
                sampler: sampler.clone(),
            },
            params_layout,
            params_bound: params.capacity(),
            params,
            params_group,
            slots: Vec::new(),
            laid: Vec::new(),
            captures: Vec::new(),
            levels: DecayingTexture::new(),
            scratch: DecayingTexture::new(),
            traffic: gpu.pass_traffic(),
            staging: StagingTarget::new("palantir.backdrop.staging", LEVEL_FORMAT),
            window: ExpiryWindow::default(),
            scratch_packer: ShelfPacker::within(ceiling),
            ceiling,
            target: None,
        }
    }

    /// The capture of `format`, made where none is and grown to hold `size`.
    fn ensure_capture(&mut self, gpu: &Gpu, format: TargetFormat, size: UVec2) -> usize {
        let at = self
            .captures
            .iter()
            .position(|capture| capture.format == format)
            .unwrap_or_else(|| {
                self.captures.push(CaptureTarget {
                    format,
                    filtering: filtering_of(gpu, format),
                    texture: DecayingTexture::new(),
                });
                self.captures.len() - 1
            });
        let sources = &self.sources;
        let capture = &mut self.captures[at];
        let filtering = capture.filtering;
        capture.texture.reserve(size, self.ceiling, |size| {
            sources.sampled(gpu, capture_spec(format, size, filtering))
        });
        at
    }

    /// Lay out `jobs` in `batches`, each the jobs [`Self::encode`] records together, make room for
    /// them once, at the most a batch reads, end an expiry window every
    /// [`EXPIRY_FRAMES`](crate::common::expiry_window::EXPIRY_FRAMES) display frames, and write
    /// their parameters, ahead of the copies that fill their captures and the passes.
    pub(crate) fn prepare(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        jobs: &[BlurJob],
        batches: impl Iterator<Item = Range<usize>>,
        target: Option<TargetSource>,
    ) {
        self.target = target;
        self.laid.clear();
        self.laid.extend(jobs.iter().map(|&job| LaidJob {
            job,
            layout: JobLayout::default(),
        }));
        let mut needs = BlurNeeds::default();
        let mut laid_out = 0;
        for batch in batches {
            debug_assert_eq!(batch.start, laid_out, "batches cover the jobs in order");
            laid_out = batch.end;
            needs = needs.max(self.lay_out(batch));
        }
        debug_assert_eq!(laid_out, jobs.len(), "batches cover the jobs");
        self.make_room(ctx.gpu, needs);
        self.staging.decay(ctx.gpu, ctx.frame);
        self.expire(ctx);
        // After the step-down, which may remake a texture smaller, at the window's peak, which
        // holds this frame's needs: each pass's `source_max` is then the bound of the texture it
        // binds.
        self.write_params(ctx);
    }

    /// Grow the capture, the levels and the scratch to hold `needs`.
    fn make_room(&mut self, gpu: &Gpu, needs: BlurNeeds) {
        if let Some((format, size)) = needs.capture {
            self.ensure_capture(gpu, format, size);
        }
        let (sources, ceiling) = (&self.sources, self.ceiling);
        if needs.levels != LevelsExtent::NONE {
            self.levels.reserve(needs.levels, ceiling, |extent| {
                Levels::new(gpu, sources, extent, ceiling)
            });
        }
        if needs.scratch != UVec2::ZERO {
            self.scratch.reserve(needs.scratch, ceiling, |size| {
                sources.sampled(gpu, scratch_spec(size))
            });
        }
        if needs.staging != UVec2::ZERO {
            self.staging.reserve(gpu, needs.staging);
        }
    }

    /// At the end of an expiry window, make each shared texture again at what the window's frames
    /// read at most, or drop it where none read it; the reserves of [`Self::make_room`] note each
    /// frame's needs.
    fn expire(&mut self, ctx: &GpuCtx<'_>) {
        if !self.window.ends(ctx.frame) {
            return;
        }
        let gpu = ctx.gpu;
        let (sources, ceiling) = (&self.sources, self.ceiling);
        for capture in &mut self.captures {
            let (format, filtering) = (capture.format, capture.filtering);
            capture.texture.step_down(ceiling, |size| {
                sources.sampled(gpu, capture_spec(format, size, filtering))
            });
        }
        self.captures
            .retain(|capture| capture.texture.get().is_some());
        self.levels
            .step_down(ceiling, |extent| Levels::new(gpu, sources, extent, ceiling));
        self.scratch
            .step_down(ceiling, |size| sources.sampled(gpu, scratch_spec(size)));
    }

    /// Give the jobs of `batch`, laid out in order, their capture and levels origins, at the first
    /// texel the batch reads of each, and each job its round and place in the scratch, and report
    /// what of the capture, the levels and the scratch it reads.
    fn lay_out(&mut self, batch: Range<usize>) -> BlurNeeds {
        let Self {
            laid,
            target,
            traffic,
            scratch_packer,
            ..
        } = self;
        let batch = &mut laid[batch];
        let first = batch[0].job;
        let mut needs = BlurNeeds::default();
        let capture_origin = if target.is_some() {
            UVec2::ZERO
        } else {
            let footprints = batch
                .iter()
                .map(|laid| laid.job.footprint)
                .reduce(URect::union)
                .expect("a batch holds a job");
            needs.capture = Some((first.format, footprints.size));
            footprints.min
        };
        let deepest = batch
            .iter()
            .map(|laid| laid.job.plan.levels())
            .max()
            .unwrap_or(0);
        let levels_origin = batch
            .iter()
            .map(|laid| laid.job)
            .filter(|job| job.plan.levels() > 0)
            .map(|job| level_region(job.region, job.plan.levels(), 1, job.surface))
            .reduce(URect::union)
            .map_or(UVec2::ZERO, |window| {
                let align = 1 << (deepest - 1);
                let origin = window.min / align * align;
                needs.levels = LevelsExtent {
                    first: window.max() - origin,
                    mips: deepest,
                };
                origin
            });
        let mut round = 0;
        scratch_packer.clear();
        for LaidJob { job, layout } in batch {
            layout.capture_origin = capture_origin;
            layout.levels_origin = levels_origin;
            layout.staged =
                *traffic == PassTraffic::WholeTarget && matches!(job.dest, ResultTexture::Page(_));
            // A staged job's place holds its result, which a page holds, so within the device's
            // edge; every other job's only its region, as a larger scratch costs a device whose
            // passes move their whole target each pass into it.
            let place = if layout.staged {
                job.dest_rect().size
            } else {
                job.region.size
            };
            let size = place.as_u16vec2();
            let origin = scratch_packer.allocate(size).unwrap_or_else(|| {
                round += 1;
                scratch_packer.clear();
                scratch_packer
                    .allocate(size)
                    .expect("a region lies on its surface, which the device's edge holds")
            });
            layout.scratch_round = round;
            layout.scratch_origin = origin.as_uvec2();
            let end = layout.scratch_origin + place;
            needs.scratch = needs.scratch.max(end);
            if layout.staged {
                needs.staging = needs.staging.max(end);
            }
        }
        needs
    }

    /// Write the parameters of the laid out jobs, and each one's first slot.
    fn write_params(&mut self, ctx: &mut GpuCtx<'_>) {
        self.slots.clear();
        for at in 0..self.laid.len() {
            self.laid[at].layout.first_slot = self.slots.len();
            self.push_params(self.laid[at]);
        }
        self.params.upload_instances(ctx, &self.slots);
        if self.params.capacity() != self.params_bound {
            self.params_group = params_group(ctx.gpu, &self.params_layout, &self.params.buffer);
            self.params_bound = self.params.capacity();
        }
    }

    /// Copy the footprint of prepared job `at`, by its place in the jobs the last prepare took, of
    /// `color`, the target the main pass paints, into its capture, past the batch's capture
    /// origin. Nothing where the jobs read the target itself.
    pub(crate) fn copy_capture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        at: usize,
        color: &wgpu::Texture,
    ) {
        if self.target.is_some() {
            return;
        }
        let LaidJob { job, layout } = self.laid[at];
        let footprint = job.footprint;
        let capture = self.capture_of(job.format);
        debug_assert!(
            footprint.max().x <= color.width() && footprint.max().y <= color.height(),
            "a footprint lies on its surface"
        );
        let into = footprint.min - layout.capture_origin;
        debug_assert!(
            into.x + footprint.size.x <= capture.target().texture.width()
                && into.y + footprint.size.y <= capture.target().texture.height(),
            "the capture holds its batch's footprints"
        );
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: color,
                mip_level: 0,
                origin: texel::origin(footprint.min),
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &capture.target().texture,
                mip_level: 0,
                origin: texel::origin(into),
                aspect: wgpu::TextureAspect::All,
            },
            texel::extent(footprint.size),
        );
    }

    /// What prepared job `job` reads as level 0: the target where the frame samples it, else
    /// the capture of its format.
    fn level_zero(&self, job: &BlurJob) -> LevelZero<'_> {
        if let Some(target) = &self.target {
            return LevelZero {
                group: &target.group,
                filtering: target.filtering,
            };
        }
        let capture = self.capture_of(job.format);
        LevelZero {
            group: &capture.target().group,
            filtering: capture.filtering,
        }
    }

    /// The group `view`, a target of `format` the frame paints, is read through as level 0, in
    /// place of a capture.
    pub(crate) fn target_source(
        &self,
        gpu: &Gpu,
        view: &wgpu::TextureView,
        format: TargetFormat,
        size: UVec2,
    ) -> TargetSource {
        let filtering = filtering_of(gpu, format);
        TargetSource {
            group: self.sources.group(gpu, view, filtering),
            size,
            filtering,
        }
    }

    /// The capture of `format`, which a prepared job of it has.
    fn capture_of(&self, format: TargetFormat) -> &CaptureTarget {
        self.captures
            .iter()
            .find(|c| c.format == format)
            .expect("a prepared job has its capture")
    }

    /// Record the passes of prepared jobs `batch`, by their places in the jobs the last prepare
    /// took, which read one capture: their tents a level to a pass,
    /// then for each round of the scratch the residual across into it and down into each job's
    /// texels of its [`BlurJob::dest`], which `dests` gives, a pass a destination; a staged job's
    /// down into its place of the staging target, one pass for the round, and a copy onto its
    /// destination. Every pass draws each job under its own scissor, so the jobs pay one pass a
    /// stage and destination.
    pub(crate) fn encode<'v>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        batch: Range<usize>,
        dests: impl Fn(ResultTexture) -> ResultTarget<'v>,
    ) {
        let jobs = &self.laid[batch];
        let first = &jobs[0];
        debug_assert!(
            jobs.iter().all(|laid| laid.job.format == first.job.format
                && laid.job.surface == first.job.surface
                && laid.layout.capture_origin == first.layout.capture_origin
                && laid.layout.levels_origin == first.layout.levels_origin),
            "jobs encoded together are one batch, which reads one capture and one set of levels"
        );
        let zero = self.level_zero(&first.job);
        let level_groups = self.levels.get().map(|l| &*l.groups);
        let level_views = self.levels.get().map(|l| &*l.views);
        // Level `k` reads from the capture at `k = 1`, and from mip `k − 2` above it.
        let source_of = |level: u8| match level {
            0 => zero.group,
            k => &level_groups.expect("a blurred level has its levels")[usize::from(k) - 1],
        };
        let passes_of = |level: u8| {
            if level == 0 && zero.filtering == Filtering::Loaded {
                &self.loaded
            } else {
                &self.filtered
            }
        };
        let deepest = jobs
            .iter()
            .map(|laid| laid.job.plan.levels())
            .max()
            .unwrap_or(0);
        for k in 1..=deepest {
            let view = &level_views.expect("a blurred level has its levels")[usize::from(k) - 1];
            let draws = jobs.iter().filter(|laid| laid.job.plan.levels() >= k).map(
                |&LaidJob { job, layout }| {
                    let written = level_region(job.region, job.plan.levels(), k, job.surface);
                    Draw {
                        pipeline: &passes_of(k - 1).tent,
                        source: source_of(k - 1),
                        slot: layout.first_slot + usize::from(k) - 1,
                        scissor: URect {
                            min: written.min - layout.origin(k),
                            size: written.size,
                        },
                    }
                },
            );
            self.pass(encoder, view, draws);
        }
        let residual =
            |laid: &LaidJob| laid.layout.first_slot + usize::from(laid.job.plan.levels());
        let scratch = self.scratch.get().expect("prepared");
        for round in jobs.chunk_by(|a, b| a.layout.scratch_round == b.layout.scratch_round) {
            let across = round.iter().map(|laid| Draw {
                pipeline: &passes_of(laid.job.plan.levels()).horizontal,
                source: source_of(laid.job.plan.levels()),
                slot: residual(laid),
                scissor: URect {
                    min: laid.layout.scratch_origin,
                    size: laid.job.region.size,
                },
            });
            self.pass(encoder, &scratch.view, across);
            let staged_rect = |laid: &LaidJob| URect {
                min: laid.layout.scratch_origin,
                size: laid.job.dest_rect().size,
            };
            if round.iter().any(|laid| laid.layout.staged) {
                {
                    let mut rp = self.staging.begin_pass(encoder);
                    let down = round
                        .iter()
                        .filter(|laid| laid.layout.staged)
                        .map(|laid| Draw {
                            pipeline: &self.vertical,
                            source: &scratch.group,
                            slot: residual(laid) + 1,
                            scissor: staged_rect(laid),
                        });
                    self.draw(&mut rp, down);
                }
                for laid in round.iter().filter(|laid| laid.layout.staged) {
                    let dest = dests(laid.job.dest);
                    self.staging.copy_to(
                        encoder,
                        staged_rect(laid),
                        dest.texture,
                        laid.job.dest_origin,
                    );
                }
            }
            // A pass for each destination the round writes unstaged, at its first job: rounds
            // are few jobs, so the scan beats a sort that would reorder the slots.
            for (at, laid) in round.iter().enumerate() {
                let dest = laid.job.dest;
                if laid.layout.staged || round[..at].iter().any(|before| before.job.dest == dest) {
                    continue;
                }
                let down = round[at..]
                    .iter()
                    .filter(|other| other.job.dest == dest)
                    .map(|other| Draw {
                        pipeline: &self.vertical,
                        source: &scratch.group,
                        slot: residual(other) + 1,
                        scissor: other.job.dest_rect(),
                    });
                self.pass(encoder, dests(dest).view, down);
            }
        }
    }

    /// The parameters of `job`'s passes, in the order [`Self::encode`] records them.
    fn push_params(&mut self, laid: LaidJob) {
        let LaidJob {
            job:
                BlurJob {
                    plan,
                    surface,
                    format,
                    region,
                    dest_origin,
                    ..
                },
            layout,
        } = laid;
        let scratch_origin = layout.scratch_origin;
        let levels = plan.levels();
        let capture_max = self.target.as_ref().map_or_else(
            || texel::size(&self.capture_of(format).target().texture),
            |target| target.size,
        );
        let mip_max = |level: u8| {
            let levels = self.levels.get().expect("a blurred level has its levels");
            texel::size(&levels.texture) >> u32::from(level - 1)
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
        // A read past the region lands on another job's texels of the scratch, or on none,
        // outside the support of every texel the backdrop shows.
        let lands = if layout.staged {
            scratch_origin
        } else {
            dest_origin
        };
        slot(BlurParams {
            target_to_lattice: region.min.as_ivec2() - IVec2::splat(apron) - lands.as_ivec2(),
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
        self.draw(&mut rp, draws);
    }

    /// Draw each of `draws` in `rp`.
    fn draw<'a>(&'a self, rp: &mut wgpu::RenderPass<'_>, draws: impl Iterator<Item = Draw<'a>>) {
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

fn params_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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

/// The first mip of levels that hold `have` and must hold `want` texels of level 1 across `mips`
/// levels: [`Extent::grown`], each edge a whole number of the last mip's texels, as a mip floors
/// its halving where a level's extent rounds up, so every mip stays as wide as its level.
fn levels_size(have: UVec2, want: UVec2, mips: u8, ceiling: u32) -> UVec2 {
    let last = 1 << (mips - 1);
    have.grown(want, ceiling)
        .map(|side| side.next_multiple_of(last))
}

/// The numbers [`BackdropBlur::SHADER`] takes from the Rust types that own them.
const SHADER_CONSTANTS: &[ShaderConstant] = &[ShaderConstant::uint(
    "TAP_VEC4S",
    BlurPlan::MAX_TAPS.div_ceil(4) as u32,
)];

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::atlas_memory::AtlasUse;
    use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob, ResultTarget};
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::resource::gpu_ctx::GpuCtx;
    #[cfg(test)]
    use crate::gpu::resource::shelf_packer::ShelfPacker;
    use crate::gpu::resource::staging_target::StagingTarget;
    use crate::gpu::texel::TargetFormat;
    use glam::UVec2;
    use std::iter;

    impl BackdropBlur {
        /// The memory of the blur's captures, levels and scratch, named `name`.
        pub(crate) fn memory(&self, name: &'static str) -> AtlasUse {
            let captures = self
                .captures
                .iter()
                .map(|capture| &capture.target().texture);
            let levels = self.levels.get().map(|levels| &levels.texture);
            let scratch = self.scratch.get().map(|scratch| &scratch.texture);
            AtlasUse::scratch(name, captures.chain(levels).chain(scratch))
        }

        /// The target the blur's staged jobs draw their second axis into.
        pub(crate) const fn staging(&self) -> &StagingTarget {
            &self.staging
        }

        /// Hold every texture of the blur to an edge of `ceiling` texels, as a device of that
        /// limit does.
        #[cfg(test)]
        pub(crate) fn cap(&mut self, ceiling: u32) {
            self.ceiling = ceiling;
            self.scratch_packer = ShelfPacker::within(ceiling);
        }

        /// The capture a surface of `size` and `format` renders the pixels behind its backdrops
        /// into, grown to fit.
        pub(crate) fn capture_view(
            &mut self,
            gpu: &Gpu,
            format: TargetFormat,
            size: UVec2,
        ) -> &wgpu::TextureView {
            let at = self.ensure_capture(gpu, format, size);
            &self.captures[at].target().view
        }

        /// Blur each of `jobs` from its capture into its slot of `dest`, a `LEVEL_FORMAT` target
        /// every job's destination names. The capture must hold the pixels of each job's region,
        /// grown by its plan's reach.
        pub(crate) fn run(
            &mut self,
            ctx: &mut GpuCtx<'_>,
            jobs: &[BlurJob],
            dest: ResultTarget<'_>,
        ) {
            let all = 0..jobs.len();
            self.prepare(ctx, jobs, iter::once(all.clone()), None);
            self.encode(ctx.encoder, all, |_| dest);
        }
    }
}

#[cfg(test)]
mod tests;
