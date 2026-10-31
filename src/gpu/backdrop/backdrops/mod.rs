//! A frame's panes on the device: their blurs, cached across frames, and the results their quads
//! composite.

use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob, JobLayout, LEVEL_FORMAT};
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::packed_atlas::{AtlasSpec, PackedAtlas};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::render_target::TargetFormat;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, PaneSource};
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

impl BackdropSlot {
    /// How `row`'s quad finds `kept`, its result.
    fn of(row: &BackdropRow, kept: Kept) -> Self {
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
        Self {
            offset,
            lattice,
            highlight: row.lens.map_or(0.0, |lens| lens.highlight),
            lens,
            clear,
            max_luminance: row.max_luminance,
        }
    }
}

/// A new slot's edges round up to this many texels, so a pane that grows a few px a frame keeps
/// its slot rather than taking a new one each frame.
const SLOT_GRANULE: u32 = 16;

/// The side the results atlas starts at; it doubles when a result finds no room.
const FIRST_SIDE: u32 = 1024;

/// A pane's result kept across frames, and the [`Backdrops::prepare`] call that last used it.
#[derive(Clone, Copy, Debug)]
struct Cached {
    kept: Kept,
    used: u64,
}

/// What a kept result holds, under the plan and region its row names: the pane ledger says what
/// it blurred.
#[derive(Clone, Copy, Debug)]
enum Kept {
    /// The atlas texels the result and its apron hold.
    Slot(URect),
    /// The linear clear the pane shows, as the target holds it.
    Clear([f32; 3]),
}

/// What [`Backdrops::prepare`] reads of a frame.
#[derive(Debug)]
pub(crate) struct BackdropFrame<'a> {
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) owner: RenderOwnerId,
    pub(crate) format: TargetFormat,
    /// The color the frame clears to.
    pub(crate) clear: RgbaF16,
    /// The backbuffer the main pass paints, where the frame paints through one: a pass can
    /// sample it, so the blurs read it in place of a capture.
    pub(crate) backbuffer: Option<&'a mut Backbuffer>,
}

/// The panes that capture at one break of the main pass: the barrier it breaks at, and their
/// blurs, which run in one set of passes.
#[derive(Clone, Debug)]
pub(crate) struct BlurBreak {
    pub(crate) barrier: u32,
    pub(crate) jobs: Range<usize>,
}

/// Every window's panes blurred into one atlas, a slot each, kept while a pane's backdrop holds,
/// and the group their quads read it through. What the panes hold on the device is made on the
/// first frame with a pane, so an app with none pays for no pipeline and no atlas; its buffers and
/// textures grow to the largest frame seen and are kept.
///
/// At its ceiling the atlas evicts the least recently used results no pane of the frame reads,
/// from any window, and lists them ([`Self::evicted`]) for each window's pane ledger to drop; a
/// result that finds no room even then shows its tint over the clear and captures again.
#[derive(Debug)]
pub(crate) struct Backdrops {
    cache: FxHashMap<(RenderOwnerId, PaneKey), Cached>,
    /// How many times [`Self::prepare`] ran, the stamp of the results its frame reads.
    prepared: u64,
    /// The results evicted from a window's panes, until its pane ledger drops them.
    evicted: Vec<(RenderOwnerId, PaneKey)>,
    /// This frame's blurs and the breaks they run at, in paint order.
    jobs: Vec<BlurJob>,
    breaks: Vec<BlurBreak>,
    slots: Vec<BackdropSlot>,
    /// The layout the quad pipeline names at group 2, made with it.
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The edge the results atlas grows to.
    ceiling: u32,
    /// What the panes hold on the device, from the first frame with a pane.
    resources: Option<PaneResources>,
}

/// What [`Backdrops`] holds on the device once a frame has a pane: the blur, the results atlas, the
/// slot table and the group their quads read them through.
#[derive(Debug)]
struct PaneResources {
    blur: BackdropBlur,
    atlas: PackedAtlas,
    slot_buffer: DynamicBuffer<BackdropSlot>,
    group: wgpu::BindGroup,
    /// The `slot_buffer` capacity `group` binds.
    group_capacity: usize,
}

impl PaneResources {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        ceiling: u32,
    ) -> Self {
        let slot_buffer = DynamicBuffer::storage(device, "palantir.backdrop.slots", 4);
        let atlas = PackedAtlas::new(
            device,
            AtlasSpec {
                label: "palantir.backdrop.results".to_owned(),
                format: LEVEL_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                ceiling,
            },
            FIRST_SIDE,
        );
        let group = composite_group(device, layout, atlas.view(), sampler, &slot_buffer);
        Self {
            blur: BackdropBlur::new(device),
            atlas,
            group_capacity: slot_buffer.capacity(),
            slot_buffer,
            group,
        }
    }
}

impl Backdrops {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let ceiling = device
            .limits()
            .max_texture_dimension_2d
            .min(u32::from(u16::MAX));
        Self::with_ceiling(device, ceiling)
    }

    /// [`Self::new`] with a results atlas that grows to an edge of `ceiling` texels.
    fn with_ceiling(device: &wgpu::Device, ceiling: u32) -> Self {
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
        Self {
            cache: FxHashMap::default(),
            prepared: 0,
            evicted: Vec::new(),
            jobs: Vec::new(),
            breaks: Vec::new(),
            slots: Vec::new(),
            layout,
            sampler,
            ceiling,
            resources: None,
        }
    }

    /// The group a pane's quad reads its result through, at group 2.
    pub(crate) const fn layout(&self) -> &wgpu::BindGroupLayout {
        &self.layout
    }

    /// The group the last prepared frame's panes read their results through; `None` before the
    /// first frame with a pane.
    pub(crate) fn group(&self) -> Option<&wgpu::BindGroup> {
        self.resources.as_ref().map(|resources| &resources.group)
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
            backbuffer,
        } = frame;
        self.evict(owner, &buffer.live_panes);
        self.prepared += 1;
        self.jobs.clear();
        self.breaks.clear();
        self.slots.clear();
        if buffer.backdrops.is_empty() && self.resources.is_none() {
            return;
        }
        self.resources.get_or_insert_with(|| {
            PaneResources::new(ctx.device, &self.layout, &self.sampler, self.ceiling)
        });
        // Every result the frame reads is stamped first, so making room for one never evicts
        // another.
        for row in &buffer.backdrops {
            if let Some(cached) = self.cache.get_mut(&(owner, row.key)) {
                cached.used = self.prepared;
            }
        }
        let surface = buffer.display.physical;
        // The jobs of one break blur in the same passes, so each takes rows of the scratch apart
        // from the others'.
        let mut scratch_row = 0;
        for row in &buffer.backdrops {
            let at = (owner, row.key);
            let kept = match row.source {
                PaneSource::Capture { barrier } => {
                    let size = row.region.size + UVec2::splat(2 * row.plan.apron());
                    let Some(slot) = self.slot_for(at, size) else {
                        self.evicted.push(at);
                        self.slots.push(BackdropSlot::of(
                            row,
                            Kept::Clear(held_clear(clear, format)),
                        ));
                        continue;
                    };
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
                        footprint: row.footprint,
                        region: row.region,
                        dest_origin: slot.min,
                        scratch_origin: UVec2::new(0, scratch_row),
                        layout: JobLayout::default(),
                    });
                    scratch_row += row.region.size.y;
                    self.keep(at, Kept::Slot(slot))
                }
                PaneSource::Clear => {
                    self.drop_result(at);
                    self.keep(at, Kept::Clear(held_clear(clear, format)))
                }
                PaneSource::Hidden => {
                    // Its quad was pruned, so no fragment reads its slot.
                    self.slots.push(BackdropSlot::default());
                    continue;
                }
                PaneSource::Held => {
                    if let Some(cached) = self.cache.get(&at) {
                        cached.kept
                    } else {
                        // Another window's frame evicted it after this one decided.
                        debug_assert!(
                            self.evicted.contains(&at),
                            "a pane that keeps its capture holds a result"
                        );
                        self.slots.push(BackdropSlot::of(
                            row,
                            Kept::Clear(held_clear(clear, format)),
                        ));
                        continue;
                    }
                }
            };
            self.slots.push(BackdropSlot::of(row, kept));
        }
        let blur = &self.resources.as_ref().expect("made above").blur;
        let target = backbuffer
            .filter(|_| !self.jobs.is_empty())
            .map(|backbuffer| {
                backbuffer
                    .blur_source(|view, size| blur.target_source(ctx.device, view, format, size))
            });
        let resources = self.resources.as_mut().expect("made above");
        resources.blur.prepare(
            ctx,
            &mut self.jobs,
            self.breaks.iter().map(|at| at.jobs.clone()),
            target,
        );
        resources.slot_buffer.upload_instances(ctx, &self.slots);
        let grew = resources.atlas.commit(ctx.device, ctx.encoder);
        if grew || resources.slot_buffer.capacity() != resources.group_capacity {
            resources.group = composite_group(
                ctx.device,
                &self.layout,
                resources.atlas.view(),
                &self.sampler,
                &resources.slot_buffer,
            );
            resources.group_capacity = resources.slot_buffer.capacity();
        }
    }

    /// Drop the results of `owner`'s panes not in `live`, the panes its frame painted or kept.
    fn evict(&mut self, owner: RenderOwnerId, live: &[PaneKey]) {
        let resources = &mut self.resources;
        self.cache.retain(|&(at, key), cached| {
            let keep = at != owner || live.contains(&key);
            if !keep && let Kept::Slot(slot) = cached.kept {
                release(resources, slot);
            }
            keep
        });
    }

    /// Drop every result of `owner`, whose window closed.
    pub(crate) fn retire(&mut self, owner: RenderOwnerId) {
        self.evict(owner, &[]);
        self.forget_evicted(owner);
    }

    /// A slot of `size` for pane `at`: its cached one when that holds it, else a new one rounded
    /// up to [`SLOT_GRANULE`], the atlas grown until it has room, and past its ceiling the least
    /// recently used results no pane of this frame reads evicted; `None` when nothing more can go.
    fn slot_for(&mut self, at: (RenderOwnerId, PaneKey), size: UVec2) -> Option<URect> {
        if let Some(Cached {
            kept: Kept::Slot(slot),
            ..
        }) = self.cache.get(&at)
            && slot.size.x >= size.x
            && slot.size.y >= size.y
        {
            return Some(*slot);
        }
        self.drop_result(at);
        let size = size.map(|edge| edge.next_multiple_of(SLOT_GRANULE));
        let want = U16Vec2::new(u16::try_from(size.x).ok()?, u16::try_from(size.y).ok()?);
        if !self.atlas().fits_ceiling(want) {
            return None;
        }
        loop {
            if let Some(origin) = self.atlas().allocate(want) {
                return Some(URect {
                    min: origin.as_uvec2(),
                    size,
                });
            }
            if !self.atlas().grow() && !self.evict_lru() {
                return None;
            }
        }
    }

    /// Keep `kept` as pane `at`'s result, read by this frame.
    fn keep(&mut self, at: (RenderOwnerId, PaneKey), kept: Kept) -> Kept {
        self.cache.insert(
            at,
            Cached {
                kept,
                used: self.prepared,
            },
        );
        kept
    }

    /// Evict the least recently used result in a slot that no pane of this frame reads, and list
    /// it for its window's ledger; `false` when there is none.
    fn evict_lru(&mut self) -> bool {
        let prepared = self.prepared;
        let Some((&at, _)) = self
            .cache
            .iter()
            .filter(|(_, cached)| cached.used != prepared && matches!(cached.kept, Kept::Slot(_)))
            .min_by_key(|(_, cached)| cached.used)
        else {
            return false;
        };
        self.drop_result(at);
        self.evicted.push(at);
        true
    }

    /// The panes of `owner` whose results went, which its pane ledger must stop holding.
    pub(crate) fn evicted(&self, owner: RenderOwnerId) -> impl Iterator<Item = PaneKey> {
        self.evicted
            .iter()
            .filter(move |(at, _)| *at == owner)
            .map(|&(_, key)| key)
    }

    /// Forget the evictions [`Self::evicted`] listed for `owner`, which its ledger took.
    pub(crate) fn forget_evicted(&mut self, owner: RenderOwnerId) {
        self.evicted.retain(|(at, _)| *at != owner);
    }

    /// Drop pane `at`'s cached result, freeing its slot of the atlas if it holds one.
    fn drop_result(&mut self, at: (RenderOwnerId, PaneKey)) {
        if let Some(Cached {
            kept: Kept::Slot(slot),
            ..
        }) = self.cache.remove(&at)
        {
            release(&mut self.resources, slot);
        }
    }

    /// The results atlas, made with the resources before any slot is asked of it.
    const fn atlas(&mut self) -> &mut PackedAtlas {
        &mut self.resources.as_mut().expect("a frame with a pane").atlas
    }

    /// The breaks of the main pass this frame's blurs run at, in paint order.
    pub(crate) fn breaks(&self) -> &[BlurBreak] {
        &self.breaks
    }

    /// Copy the footprints of break `at`'s panes out of `color`, the main pass's target, into
    /// their capture; nothing where the blurs read the target itself.
    pub(crate) fn copy_break(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        at: &BlurBreak,
        color: &wgpu::Texture,
    ) {
        let blur = &self.resources.as_ref().expect("a frame that blurs").blur;
        for job in &self.jobs[at.jobs.clone()] {
            blur.copy_capture(encoder, job, color);
        }
    }

    /// Record the blurs of break `at` from their capture into their slots of the atlas.
    pub(crate) fn blur_break(&self, encoder: &mut wgpu::CommandEncoder, at: &BlurBreak) {
        let resources = self.resources.as_ref().expect("a frame that blurs");
        resources
            .blur
            .encode(encoder, &self.jobs[at.jobs.clone()], resources.atlas.view());
    }
}

/// Free `slot` of the results atlas, which holds it.
fn release(resources: &mut Option<PaneResources>, slot: URect) {
    resources
        .as_mut()
        .expect("a slot lives in the atlas")
        .atlas
        .deallocate(slot_origin(slot), slot.size.x as u16);
}

/// The linear color a target of `format` holds where it is cleared to `clear`, as a pane's capture
/// reads it back, and rounded to `f16` as its blur stores it: an 8-bit sRGB target rounds to its
/// byte, so the pane matches the clear around it. A float target keeps the `f16` clear; one
/// narrower, `Rg11b10Ufloat`, rounds again where the pane writes.
fn held_clear(clear: RgbaF16, format: TargetFormat) -> [f32; 3] {
    let clear = RgbaF32::from(clear);
    let held = if format.get().is_srgb() {
        RgbaF32::from(RgbaF32::from(clear.to_srgba8()).to_f16())
    } else {
        clear
    };
    [held.r, held.g, held.b]
}

const fn slot_origin(slot: URect) -> U16Vec2 {
    U16Vec2::new(slot.min.x as u16, slot.min.y as u16)
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
