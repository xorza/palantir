//! A frame's panes on the device: their blurs, cached across frames, and the results their quads
//! composite.

use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob, LEVEL_FORMAT, TargetSource};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::raster::raster_atlas::shelf_packer::ShelfPacker;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::render_target::{self, TargetFormat};
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::PaneSource;
use crate::renderer::render_owner_id::RenderOwnerId;
use glam::{U16Vec2, UVec2, Vec2};
use rustc_hash::FxHashMap;
use std::ops::Range;

/// How a pane's quad finds its result, as `quad_pipeline/shader.wgsl` declares `BackdropSlot`:
/// the result's texel coordinate of the lattice's first texel, the physical px of a texel, or 0
/// for a pane that shows `clear`, the pane's lens, `(bezel, thickness, refractive_index,
/// dispersion)` with its highlight, zero for a pane of no glass, and its luminance ceiling.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct BackdropSlot {
    offset: Vec2,
    lattice: f32,
    highlight: f32,
    lens: [f32; 4],
    clear: [f32; 3],
    max_luminance: f32,
}

/// The side the results atlas starts at; it doubles when a result finds no room.
const FIRST_SIDE: u32 = 1024;

/// A pane's result kept across frames, under the plan and region its row names: the pane ledger
/// says what it blurred.
#[derive(Clone, Copy, Debug)]
enum Kept {
    /// The atlas texels the result and its apron hold.
    Slot(URect),
    /// The linear clear the pane shows, as the target holds it.
    Clear([f32; 3]),
}

/// What [`Backdrops::prepare`] reads of a frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BackdropFrame<'a> {
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) owner: RenderOwnerId,
    pub(crate) format: TargetFormat,
    /// The color the frame clears to.
    pub(crate) clear: RgbaF16,
    /// The texture the main pass paints and its view, where a pass can sample it: the blurs then
    /// read it in place of a capture.
    pub(crate) target: Option<(&'a wgpu::Texture, &'a wgpu::TextureView)>,
}

/// The panes that capture at one break of the main pass: the barrier it breaks at, and their
/// blurs, which run in one set of passes.
#[derive(Clone, Debug)]
pub(crate) struct BlurBreak {
    pub(crate) barrier: u32,
    pub(crate) jobs: Range<usize>,
}

/// Every window's panes blurred into one atlas, a slot each, kept while a pane's backdrop holds,
/// and the group their quads read it through. Buffers and textures grow to the largest frame seen
/// and are kept.
#[derive(Debug)]
pub(crate) struct Backdrops {
    blur: BackdropBlur,
    cache: FxHashMap<(RenderOwnerId, PaneKey), Kept>,
    packer: ShelfPacker,
    atlas: wgpu::Texture,
    atlas_view: wgpu::TextureView,
    /// This frame's blurs, the footprint each copies, and the breaks they run at, in paint order.
    jobs: Vec<BlurJob>,
    footprints: Vec<URect>,
    breaks: Vec<BlurBreak>,
    slots: Vec<BackdropSlot>,
    slot_buffer: DynamicBuffer<BackdropSlot>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    group: wgpu::BindGroup,
    /// The `slot_buffer` capacity `group` binds.
    group_capacity: usize,
    /// Each window's last sampled target and the group its blurs read it through, made again
    /// when the target is: one a window, so a resize frees the old target.
    targets: Vec<(RenderOwnerId, wgpu::Texture, TargetSource)>,
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
            footprints: Vec::new(),
            breaks: Vec::new(),
            slots: Vec::new(),
            slot_buffer,
            layout,
            sampler,
            group,
            group_capacity,
            targets: Vec::new(),
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
            clear,
            target,
        } = frame;
        self.evict(owner, &buffer.live_panes);
        self.jobs.clear();
        self.footprints.clear();
        self.breaks.clear();
        self.slots.clear();
        let surface = buffer.display.physical;
        // The jobs of one break blur in the same passes, so each takes rows of the scratch apart
        // from the others'.
        let mut scratch_row = 0;
        for row in &buffer.backdrops {
            let at = (owner, row.key);
            let kept = match row.source {
                PaneSource::Capture { barrier } => {
                    let size = row.region.size + UVec2::splat(2 * row.plan.apron());
                    let slot = self.slot_for(ctx, at, size);
                    let job = self.jobs.len();
                    match self.breaks.last_mut() {
                        Some(last) if last.barrier == barrier => last.jobs.end = job + 1,
                        last => {
                            debug_assert!(
                                last.is_none_or(|last| last.barrier < barrier),
                                "panes stand in paint order"
                            );
                            self.breaks.push(BlurBreak {
                                barrier,
                                jobs: job..job + 1,
                            });
                            scratch_row = 0;
                        }
                    }
                    self.jobs.push(BlurJob {
                        plan: row.plan,
                        surface,
                        format,
                        region: row.region,
                        dest_origin: slot.min,
                        scratch_origin: UVec2::new(0, scratch_row),
                    });
                    self.footprints.push(row.footprint);
                    scratch_row += row.region.size.y;
                    self.cache.insert(at, Kept::Slot(slot));
                    Kept::Slot(slot)
                }
                PaneSource::Clear => {
                    self.drop_result(at);
                    let kept = Kept::Clear(held_clear(clear, format));
                    self.cache.insert(at, kept);
                    kept
                }
                PaneSource::Hidden => {
                    // Its quad was pruned, so no fragment reads its slot.
                    self.slots.push(BackdropSlot::default());
                    continue;
                }
                PaneSource::Held => *self
                    .cache
                    .get(&at)
                    .expect("a pane that keeps its capture holds a result"),
            };
            let (offset, lattice, clear) = match kept {
                Kept::Slot(slot) => {
                    let apron = UVec2::splat(row.plan.apron());
                    let offset = (slot.min + apron).as_vec2() - row.region.min.as_vec2();
                    (offset, row.plan.lattice() as f32, [0.0; 3])
                }
                Kept::Clear(clear) => (Vec2::ZERO, 0.0, clear),
            };
            let lens = row.lens.map_or([0.0; 4], |lens| {
                [
                    lens.bezel,
                    lens.thickness,
                    lens.refractive_index,
                    lens.dispersion,
                ]
            });
            self.slots.push(BackdropSlot {
                offset,
                lattice,
                highlight: row.lens.map_or(0.0, |lens| lens.highlight),
                lens,
                clear,
                max_luminance: row.max_luminance,
            });
        }
        let target = target
            .filter(|_| !self.jobs.is_empty())
            .map(|(texture, view)| self.target_source(ctx.device, owner, texture, view, format));
        self.blur.prepare(ctx, &self.jobs, target);
        self.slot_buffer.upload_instances(ctx, &self.slots);
        if self.slot_buffer.capacity() != self.group_capacity {
            self.rebind(ctx.device);
        }
    }

    /// Drop the results of `owner`'s panes not in `live`, the panes its frame painted or kept.
    fn evict(&mut self, owner: RenderOwnerId, live: &[PaneKey]) {
        let packer = &mut self.packer;
        self.cache.retain(|&(at, key), kept| {
            let keep = at != owner || live.contains(&key);
            if !keep && let Kept::Slot(slot) = *kept {
                packer.deallocate(slot_origin(slot), slot.size.x as u16);
            }
            keep
        });
    }

    /// Drop every result of `owner`, whose window closed, and its target's group.
    pub(crate) fn retire(&mut self, owner: RenderOwnerId) {
        self.evict(owner, &[]);
        self.targets.retain(|(at, ..)| *at != owner);
    }

    /// The source `owner`'s blurs read `texture` through, made when the window's target is new.
    fn target_source(
        &mut self,
        device: &wgpu::Device,
        owner: RenderOwnerId,
        texture: &wgpu::Texture,
        view: &wgpu::TextureView,
        format: TargetFormat,
    ) -> TargetSource {
        if let Some((_, held, source)) = self.targets.iter().find(|(at, ..)| *at == owner)
            && held == texture
        {
            return source.clone();
        }
        let size = UVec2::new(texture.width(), texture.height());
        let source = self.blur.target_source(device, view, format, size);
        self.targets.retain(|(at, ..)| *at != owner);
        self.targets.push((owner, texture.clone(), source.clone()));
        source
    }

    /// Whether this frame's blurs read the target itself, so no capture is copied.
    pub(crate) const fn reads_target(&self) -> bool {
        self.blur.reads_target()
    }

    /// A slot of `size` for pane `at`: its cached one when that holds it, else a new one, the
    /// atlas grown until it has room.
    fn slot_for(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        at: (RenderOwnerId, PaneKey),
        size: UVec2,
    ) -> URect {
        if let Some(Kept::Slot(slot)) = self.cache.get(&at)
            && slot.size.x >= size.x
            && slot.size.y >= size.y
        {
            return *slot;
        }
        self.drop_result(at);
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

    /// Drop pane `at`'s cached result, freeing its slot of the atlas if it holds one.
    fn drop_result(&mut self, at: (RenderOwnerId, PaneKey)) {
        if let Some(Kept::Slot(slot)) = self.cache.remove(&at) {
            self.packer
                .deallocate(slot_origin(slot), slot.size.x as u16);
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

    /// The breaks of the main pass this frame's blurs run at, in paint order.
    pub(crate) fn breaks(&self) -> &[BlurBreak] {
        &self.breaks
    }

    /// Copy blur `job`'s footprint out of `color`, the main pass's target, into its capture.
    pub(crate) fn copy_capture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        job: usize,
        color: &wgpu::Texture,
    ) {
        self.blur
            .copy_capture(encoder, job, color, self.footprints[job]);
    }

    /// Record blurs `jobs`, of one break, from their capture into their slots of the atlas.
    pub(crate) fn encode_blurs(&self, encoder: &mut wgpu::CommandEncoder, jobs: Range<usize>) {
        self.blur.encode(encoder, jobs, &self.atlas_view);
    }
}

/// The linear color a target of `format` holds where it is cleared to `clear`, as a pane's capture
/// reads it back, and rounded to `f16` as its blur stores it: an 8-bit sRGB target rounds to its
/// byte, so the pane matches the clear around it. A float target keeps the `f16` clear; one
/// narrower, `Rg11b10Ufloat`, rounds again where the pane writes.
fn held_clear(clear: RgbaF16, format: TargetFormat) -> [f32; 3] {
    let clear = RgbaF32::from(clear);
    let held = if format.get().is_srgb() {
        RgbaF32::from(RgbaF32::from(clear.to_srgba()).to_f16())
    } else {
        clear
    };
    [held.r, held.g, held.b]
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

#[cfg(test)]
mod tests;
