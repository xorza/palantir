//! A frame's panes on the device: their blurs, cached across frames, and the results their quads
//! composite.

use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob, LEVEL_FORMAT};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::raster::raster_atlas::shelf_packer::ShelfPacker;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target::{self, TargetFormat};
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::shape::paint::blur_plan::BlurPlan;
use glam::{U16Vec2, UVec2, Vec2};
use rustc_hash::FxHashMap;

/// How a pane's quad finds its result, as `quad_pipeline/shader.wgsl` declares `BackdropSlot`:
/// the result's texel coordinate of the lattice's first texel, the physical px of a texel, and
/// the pane's lens, `(bezel, thickness, refractive_index, dispersion)` with its highlight, zero for
/// a pane of no glass.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct BackdropSlot {
    offset: Vec2,
    lattice: f32,
    highlight: f32,
    lens: [f32; 4],
}

/// The side the results atlas starts at; it doubles when a result finds no room.
const FIRST_SIDE: u32 = 1024;

/// A pane's blurred result kept in the atlas: what it blurred, and where it lies.
#[derive(Clone, Copy, Debug)]
struct Cached {
    plan: BlurPlan,
    surface: UVec2,
    /// The region blurred, in texels of the plan's last level.
    region: URect,
    /// The atlas texels the result and its apron hold.
    slot: URect,
}

/// What [`Backdrops::prepare`] reads of a frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BackdropFrame<'a> {
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) owner: RenderOwnerId,
    pub(crate) format: TargetFormat,
}

/// Every window's panes blurred into one atlas, a slot each, kept while a pane's backdrop holds,
/// and the group their quads read it through. Buffers and textures grow to the largest frame seen
/// and are kept.
#[derive(Debug)]
pub(crate) struct Backdrops {
    blur: BackdropBlur,
    cache: FxHashMap<(RenderOwnerId, PaneKey), Cached>,
    packer: ShelfPacker,
    atlas: wgpu::Texture,
    atlas_view: wgpu::TextureView,
    /// This frame's blurs, and for each pane the blur it runs, if its result went stale.
    jobs: Vec<BlurJob>,
    pane_jobs: Vec<Option<u32>>,
    slots: Vec<BackdropSlot>,
    slot_buffer: DynamicBuffer<BackdropSlot>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    group: wgpu::BindGroup,
    /// The `slot_buffer` capacity `group` binds.
    group_capacity: usize,
}

impl Backdrops {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palantir.backdrop.composite.bgl"),
            entries: &[
                TextureBinding::texture_entry(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("palantir.backdrop.composite.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let slot_buffer = DynamicBuffer::storage(device, "palantir.backdrop.slots", 4);
        let atlas = atlas_texture(device, FIRST_SIDE);
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let group = composite_group(device, &layout, &atlas_view, &sampler, &slot_buffer);
        let group_capacity = slot_buffer.capacity();
        Self {
            blur: BackdropBlur::new(device),
            cache: FxHashMap::default(),
            packer: ShelfPacker::new(FIRST_SIDE as u16),
            atlas,
            atlas_view,
            jobs: Vec::new(),
            pane_jobs: Vec::new(),
            slots: Vec::new(),
            slot_buffer,
            layout,
            sampler,
            group,
            group_capacity,
        }
    }

    /// The group a pane's quad reads its result through, at group 2.
    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    pub(crate) const fn group(&self) -> &wgpu::BindGroup {
        &self.group
    }

    /// Give each of the frame's panes its result: a blur into a slot of the atlas for one that
    /// captures, as its row says, else the result it holds. Drops the results of the owner's
    /// panes that left, and writes what the blurs and the quads read.
    pub(crate) fn prepare(&mut self, ctx: &mut GpuCtx<'_>, frame: BackdropFrame<'_>) {
        let BackdropFrame {
            buffer,
            owner,
            format,
        } = frame;
        self.evict(owner, &buffer.live_panes);
        self.jobs.clear();
        self.pane_jobs.clear();
        self.slots.clear();
        let surface = buffer.display.physical;
        for row in &buffer.backdrops {
            let region = row
                .plan
                .region(row.footprint, surface)
                .expect("a pane's footprint lies on the surface");
            let at = (owner, row.key);
            let cached = if row.capture {
                let size = region.size + UVec2::splat(2 * row.plan.apron());
                let slot = self.slot_for(ctx, at, size);
                let cached = Cached {
                    plan: row.plan,
                    surface,
                    region,
                    slot,
                };
                self.cache.insert(at, cached);
                self.pane_jobs.push(Some(self.jobs.len() as u32));
                self.jobs.push(BlurJob {
                    plan: row.plan,
                    surface,
                    format,
                    region,
                    dest_origin: slot.min,
                });
                cached
            } else {
                let cached = *self
                    .cache
                    .get(&at)
                    .expect("a pane that keeps its capture holds a result");
                debug_assert!(
                    cached.plan == row.plan
                        && cached.surface == surface
                        && cached.region.union(region) == cached.region,
                    "a kept result blurred what the pane shows"
                );
                self.pane_jobs.push(None);
                cached
            };
            let apron = UVec2::splat(cached.plan.apron());
            let lens = row.lens.map_or([0.0; 4], |lens| {
                [
                    lens.bezel,
                    lens.thickness,
                    lens.refractive_index,
                    lens.dispersion,
                ]
            });
            self.slots.push(BackdropSlot {
                offset: (cached.slot.min + apron).as_vec2() - cached.region.min.as_vec2(),
                lattice: cached.plan.lattice() as f32,
                highlight: row.lens.map_or(0.0, |lens| lens.highlight),
                lens,
            });
        }
        self.blur.prepare(ctx, &self.jobs);
        self.slot_buffer.upload_instances(ctx, &self.slots);
        if self.slot_buffer.capacity() != self.group_capacity {
            self.rebind(ctx.device);
        }
    }

    /// Drop the results of `owner`'s panes not in `live`, the panes its frame painted or kept.
    fn evict(&mut self, owner: RenderOwnerId, live: &[PaneKey]) {
        let packer = &mut self.packer;
        self.cache.retain(|&(at, key), cached| {
            let keep = at != owner || live.contains(&key);
            if !keep {
                packer.deallocate(slot_origin(cached.slot), cached.slot.size.x as u16);
            }
            keep
        });
    }

    /// Drop every result of `owner`, whose window closed.
    pub(crate) fn retire(&mut self, owner: RenderOwnerId) {
        self.evict(owner, &[]);
    }

    /// A slot of `size` for pane `at`: its cached one when that holds it, else a new one, the
    /// atlas grown until it has room.
    fn slot_for(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        at: (RenderOwnerId, PaneKey),
        size: UVec2,
    ) -> URect {
        if let Some(cached) = self.cache.get(&at) {
            if cached.slot.size.x >= size.x && cached.slot.size.y >= size.y {
                return cached.slot;
            }
            self.packer
                .deallocate(slot_origin(cached.slot), cached.slot.size.x as u16);
        }
        let want = U16Vec2::new(
            u16::try_from(size.x).expect("a result fits a texture"),
            u16::try_from(size.y).expect("a result fits a texture"),
        );
        loop {
            if let Some(origin) = self.packer.allocate(want) {
                return URect {
                    min: origin.as_uvec2(),
                    size,
                };
            }
            self.grow(ctx);
        }
    }

    /// Double the atlas, copying every result over.
    fn grow(&mut self, ctx: &mut GpuCtx<'_>) {
        let side = self.atlas.width() * 2;
        let max = ctx
            .device
            .limits()
            .max_texture_dimension_2d
            .min(u32::from(u16::MAX));
        assert!(
            side <= max,
            "the backdrop results outgrow a {max} px texture"
        );
        let atlas = atlas_texture(ctx.device, side);
        ctx.encoder.copy_texture_to_texture(
            self.atlas.as_image_copy(),
            atlas.as_image_copy(),
            render_target::extent(UVec2::splat(self.atlas.width())),
        );
        self.packer.grow(side as u16);
        self.atlas = atlas;
        self.atlas_view = self
            .atlas
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.rebind(ctx.device);
    }

    fn rebind(&mut self, device: &wgpu::Device) {
        self.group = composite_group(
            device,
            &self.layout,
            &self.atlas_view,
            &self.sampler,
            &self.slot_buffer,
        );
        self.group_capacity = self.slot_buffer.capacity();
    }

    /// The blur pane `pane` runs this frame, `None` when it keeps its result.
    pub(crate) fn job(&self, pane: usize) -> Option<usize> {
        self.pane_jobs[pane].map(|job| job as usize)
    }

    /// Copy blur `job`'s `footprint` out of `color`, the main pass's target, into its capture.
    pub(crate) fn copy_capture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        job: usize,
        color: &wgpu::Texture,
        footprint: URect,
    ) {
        self.blur.copy_capture(encoder, job, color, footprint);
    }

    /// Record blur `job`, from its capture into its slot of the atlas.
    pub(crate) fn encode_blur(&self, encoder: &mut wgpu::CommandEncoder, job: usize) {
        self.blur.encode(encoder, job, &self.atlas_view);
    }
}

const fn slot_origin(slot: URect) -> U16Vec2 {
    U16Vec2::new(slot.min.x as u16, slot.min.y as u16)
}

fn atlas_texture(device: &wgpu::Device, side: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("palantir.backdrop.results"),
        size: render_target::extent(UVec2::splat(side)),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: LEVEL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn composite_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    results: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    slots: &DynamicBuffer<BackdropSlot>,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("palantir.backdrop.composite"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(results),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: slots.buffer.as_entire_binding(),
            },
        ],
    })
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::backdrop::backdrops::Backdrops;

    impl Backdrops {
        /// The blurs the last prepared frame ran.
        pub(crate) const fn blurs(&self) -> usize {
            self.jobs.len()
        }
    }
}
