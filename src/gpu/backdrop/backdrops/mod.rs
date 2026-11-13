//! A frame's backdrops on the device: their blurs, cached across frames, and the results their
//! quads composite.
//!
//! # Decisions
//!
//! - **Results live in the pool's half-float list.** A result within half a page shares the pages,
//!   one larger takes a scratch texture of its own, rounded on the scratch ladder, reused and
//!   expired as the pool's scratch is (`src/gpu/AGENTS.md`, Atlases). The pool evicts the results
//!   no frame read this display frame or the last, past its soft budget. Refused: the results
//!   atlas of its own, which doubled to the device's edge for one full-window glass and copied
//!   every result back when it shrank.
//! - **A composite binds the one texture its result lies in**, a page or a scratch texture, so a
//!   draw splits where its result's texture changes, as WebRender binds each batch's textures.
//!   Refused: every page and the scratch texture in one group, a choice among five in the shader,
//!   which V3D compiles in full, the B-spline five times: 619 → 1340 instructions, and the board's
//!   `modal/scrolling` +6.9 % (`.notes/research/2026-11-12-atlas-pool-measurements.md`).

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::AtlasHandle;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::scratch_cache::ScratchId;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::backdrop::backdrop_blur::{BackdropBlur, BlurJob, JobLayout, TargetSource};
use crate::gpu::device::gpu::{FilledBuffer, Gpu};
use crate::gpu::pipeline::effect_pipeline::backdrop_groups::{
    BackdropGroups, CompositeGroups, ResultTexture,
};
use crate::gpu::render_owner_id::RenderOwnerId;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::gpu_ctx::GpuCtx;
use crate::gpu::resource::texture_binding::{self, TextureBinding};
use crate::gpu::spectrum_tables::SpectrumTables;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::texel::TargetFormat;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::backdrop_keys::BackdropKeys;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::material::lens_optics::SPECTRUM_SAMPLES;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::backdrop_row::{BackdropRow, BackdropSource, MaterialLook};
use glam::{U16Vec2, UVec2, Vec2};
use rustc_hash::FxHashMap;
use std::ops::Range;

/// How a backdrop's composite finds its result and, for a material, holds and covers it, as
/// `effect/backdrop.wgsl` declares `PalantirBackdrop`: the result's texel coordinate of the
/// lattice's first texel, the physical px of a texel, or 0 for a backdrop that shows `clear`; the
/// rect it reads, `(min, max)`; and the material's look ([`MaterialLook`]).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct BackdropSlot {
    offset: Vec2,
    lattice: f32,
    max_luminance: f32,
    clear: [f32; 3],
    min_luminance: f32,
    reads: [f32; 4],
    fill: [f32; 4],
    fill_axis: [f32; 4],
    border_color: [f32; 4],
    border_width: f32,
    fill_kind: u32,
    fill_lut_row: u32,
    /// WGSL rounds the struct's size up to its 16-byte alignment.
    pad: f32,
}

impl BackdropSlot {
    /// How `row`'s composite finds `shown`, its result where it lies this frame.
    fn of(row: &BackdropRow, shown: Shown) -> Self {
        let (offset, lattice, clear) = match shown {
            Shown::Result(placed) => {
                let capture = row.capture;
                let apron = UVec2::splat(capture.plan.apron());
                let offset = (placed.origin + apron).as_vec2() - capture.region.min.as_vec2();
                (offset, capture.plan.lattice() as f32, [0.0; 3])
            }
            Shown::Clear(clear) => (Vec2::ZERO, 0.0, clear),
        };
        let look = row.look.unwrap_or(MaterialLook::PLAIN);
        let lanes = |color: RgbaF16| {
            let color = RgbaF32::from(color);
            [color.r, color.g, color.b, color.a]
        };
        Self {
            offset,
            lattice,
            max_luminance: look.max_luminance,
            clear,
            min_luminance: look.min_luminance,
            reads: [
                row.reads.min.x,
                row.reads.min.y,
                row.reads.max().x,
                row.reads.max().y,
            ],
            fill: lanes(look.fill.color),
            fill_axis: look.fill_axis.lanes(),
            border_color: lanes(look.border_color),
            border_width: look.border_width,
            fill_kind: look.fill.kind.0,
            fill_lut_row: look.fill.lut_row.0,
            pad: 0.0,
        }
    }
}

/// One cell of [`SpectrumTables`] as `effect/backdrop.wgsl` declares `PalantirSpectrumCell`: each
/// `vec3` padded to WGSL's 16-byte alignment.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct SpectrumCell {
    weight: [f32; 4],
    cumulative: [f32; 4],
    moment: [f32; 4],
}

impl SpectrumCell {
    /// The spectrum a lens spreads, cell by cell.
    fn table() -> [Self; SPECTRUM_SAMPLES] {
        let tables = SpectrumTables::new();
        let pad = |[x, y, z]: [f32; 3]| [x, y, z, 0.0];
        std::array::from_fn(|j| Self {
            weight: pad(tables.weights[j]),
            cumulative: pad(tables.cumulative[j]),
            moment: pad(tables.moments[j]),
        })
    }
}

/// A new slot's edges round up to this many texels, so a backdrop that grows a few px a frame keeps
/// its slot rather than taking a new one each frame.
const SLOT_GRANULE: u32 = 16;

/// The list the results live in.
const RESULTS: AtlasFormat = AtlasFormat::Rgba16F;

/// Where a kept result's texels are: an item of the pool's pages, or a scratch texture of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    Item(AtlasHandle),
    Scratch(ScratchId),
}

/// A result kept across frames: where its texels are, and the texels its blur may write, the
/// result and its apron rounded up to [`SLOT_GRANULE`].
#[derive(Clone, Copy, Debug)]
struct Slot {
    held: Held,
    size: UVec2,
}

/// What a kept result holds, under the plan and region its row names: the backdrop ledger says what
/// it blurred.
#[derive(Clone, Copy, Debug)]
enum Kept {
    Slot(Slot),
    /// The linear clear the backdrop shows, as the target holds it.
    Clear([f32; 3]),
}

/// Where a slot's texels lie this frame.
#[derive(Clone, Copy, Debug)]
struct Placed {
    texture: ResultTexture,
    origin: UVec2,
}

/// What a backdrop's composite shows this frame.
#[derive(Clone, Copy, Debug)]
enum Shown {
    Result(Placed),
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
    pub(crate) backbuffer: Option<FrameBackbuffer<'a>>,
}

/// A backbuffer the blurs read, and the group they read it through, which its render stream keeps
/// beside it, made on the first frame that blurs from it and dropped when the backbuffer is made
/// again.
#[derive(Debug)]
pub(crate) struct FrameBackbuffer<'a> {
    pub(crate) backbuffer: &'a Backbuffer,
    pub(crate) source: &'a mut Option<TargetSource>,
}

/// The backdrops that capture at one break of the main pass: the barrier it breaks at, and their
/// blurs, which run in one set of passes.
#[derive(Clone, Debug)]
pub(crate) struct BlurBreak {
    pub(crate) barrier: u32,
    pub(crate) jobs: Range<usize>,
}

/// Every window's backdrops blurred into the pool's half-float list, a slot each, kept while what
/// lies under each holds, and the groups their quads read them through. What the backdrops hold on
/// the device is made on the first frame with a backdrop, so an app with none pays for no pipeline;
/// its buffers grow to the largest frame seen and are kept, and the blur's textures expire after
/// idle.
///
/// Past its soft budget the pool evicts results no frame read this display frame or the last, from
/// any window, and each submit's end lists them ([`Self::evicted`]) for each window's backdrop
/// ledger to drop. A result that finds no room shows its fill over the clear and captures again.
#[derive(Debug)]
pub(crate) struct Backdrops {
    cache: FxHashMap<(RenderOwnerId, BackdropKey), Kept>,
    /// The results evicted from a window's backdrops, until its backdrop ledger drops them.
    evicted: Vec<(RenderOwnerId, BackdropKey)>,
    /// This frame's blurs and the breaks they run at, in paint order.
    jobs: Vec<BlurJob>,
    breaks: Vec<BlurBreak>,
    slots: Vec<BackdropSlot>,
    /// The texture each of [`Self::slots`] reads, whose group its composite binds; `None` for one
    /// that shows its clear.
    slot_reads: Vec<Option<ResultTexture>>,
    /// The handles the pool evicted, as [`Self::end_frame`] drains them.
    drained: Vec<AtlasHandle>,
    /// The layout of the group a backdrop's composite reads its result through
    /// ([`TextureBinding::backdrops`]); the group itself waits for a backdrop.
    layout: wgpu::BindGroupLayout,
    /// What the backdrops hold on the device, from the first frame with a backdrop.
    resources: Option<BackdropResources>,
}

/// What [`Backdrops`] holds on the device once a frame has a backdrop: the slot table, the sampler
/// and the spectrum a lens spreads ([`SpectrumCell`]), written once; the groups over them, made
/// after the pool's commit; and the blur, made at the first frame that blurs, as a frame of held
/// results blurs nothing.
#[derive(Debug)]
struct BackdropResources {
    blur: Option<BackdropBlur>,
    slot_buffer: DynamicBuffer<BackdropSlot>,
    sampler: wgpu::Sampler,
    spectrum: wgpu::Buffer,
    groups: Option<CompositeGroups>,
}

/// What the group a backdrop's composite reads its result through binds.
#[derive(Clone, Copy, Debug)]
struct CompositeParts<'a> {
    result: &'a wgpu::TextureView,
    slots: &'a DynamicBuffer<BackdropSlot>,
    sampler: &'a wgpu::Sampler,
    spectrum: &'a wgpu::Buffer,
}

/// The group over one result texture, the sampler, the slot table and the spectrum, as
/// `effect/backdrop.wgsl` declares them.
fn composite_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    parts: CompositeParts<'_>,
) -> wgpu::BindGroup {
    let CompositeParts {
        result,
        slots,
        sampler,
        spectrum,
    } = parts;
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("palantir.backdrop.composite"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(result),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: slots.buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: spectrum.as_entire_binding(),
            },
        ],
    })
}

impl BackdropResources {
    fn new(gpu: &Gpu) -> Self {
        let spectrum_table = SpectrumCell::table();
        Self {
            blur: None,
            slot_buffer: DynamicBuffer::storage(gpu, "palantir.backdrop.slots", 4),
            sampler: texture_binding::sampler(gpu, "palantir.backdrop.composite.sampler"),
            spectrum: gpu.create_filled_buffer(
                FilledBuffer {
                    label: "palantir.backdrop.spectrum",
                    size: size_of_val(&spectrum_table) as u64,
                    usage: wgpu::BufferUsages::STORAGE,
                },
                &[bytemuck::cast_slice(&spectrum_table)],
            ),
            groups: None,
        }
    }

    /// The group over `result`.
    fn group(
        &self,
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        result: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        composite_group(
            gpu,
            layout,
            CompositeParts {
                result,
                slots: &self.slot_buffer,
                sampler: &self.sampler,
                spectrum: &self.spectrum,
            },
        )
    }
}

impl Backdrops {
    /// Backdrops whose composites read their results through `binding`'s backdrop group.
    pub(crate) fn new(binding: &TextureBinding) -> Self {
        Self {
            cache: FxHashMap::default(),
            evicted: Vec::new(),
            jobs: Vec::new(),
            breaks: Vec::new(),
            slots: Vec::new(),
            slot_reads: Vec::new(),
            drained: Vec::new(),
            layout: binding.backdrops().clone(),
            resources: None,
        }
    }

    /// The groups the last prepared frame's backdrops read their results through; `None` before
    /// the first frame with a backdrop.
    pub(crate) fn groups(&self) -> Option<BackdropGroups<'_>> {
        let groups = self.resources.as_ref()?.groups.as_ref()?;
        Some(BackdropGroups {
            groups,
            slot_reads: &self.slot_reads,
        })
    }

    /// Give each of the frame's backdrops its result: a blur into a slot of `pool`'s half-float
    /// list for one that captures, as its row says, else the result it holds. Drops the results of
    /// the owner's backdrops that left and those the pool evicted, and writes what the blurs and
    /// the quads read.
    pub(crate) fn prepare(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        frame: BackdropFrame<'_>,
        pool: &mut AtlasPool,
    ) {
        let BackdropFrame {
            buffer,
            owner,
            format,
            clear,
            backbuffer,
        } = frame;
        self.evict(owner, &buffer.live_backdrops, pool);
        self.jobs.clear();
        self.breaks.clear();
        self.slots.clear();
        self.slot_reads.clear();
        if buffer.backdrops.is_empty() && self.resources.is_none() {
            return;
        }
        self.resources
            .get_or_insert_with(|| BackdropResources::new(ctx.gpu));
        // Every result the frame reads is stamped first, so making room for one never evicts
        // another.
        for capture in buffer.backdrops.iter().map(|row| row.capture) {
            if let Some(Kept::Slot(Slot {
                held: Held::Item(handle),
                ..
            })) = self.cache.get(&(owner, capture.key))
            {
                // A result evicted since its last frame is dropped below, at its row or the drain.
                let _ = pool.stamp(*handle);
            }
        }
        let surface = buffer.display.physical;
        let held_clear = held_clear(clear, format);
        for row in &buffer.backdrops {
            let BackdropRow {
                capture, source, ..
            } = *row;
            let at = (owner, capture.key);
            let shown = match source {
                BackdropSource::Capture { barrier } => {
                    let size = capture.region.size + UVec2::splat(2 * capture.plan.apron());
                    let Some((slot, placed)) = self.slot_for(at, size, pool) else {
                        self.evicted.push(at);
                        self.push_slot(row, Shown::Clear(held_clear));
                        continue;
                    };
                    let job = self.jobs.len();
                    match self.breaks.last_mut() {
                        Some(last) if last.barrier == barrier => last.jobs.end = job + 1,
                        last => {
                            debug_assert!(
                                last.is_none_or(|last| last.barrier < barrier),
                                "backdrops stand in paint order"
                            );
                            self.breaks.push(BlurBreak {
                                barrier,
                                jobs: job..job + 1,
                            });
                        }
                    }
                    self.jobs.push(BlurJob {
                        plan: capture.plan,
                        surface,
                        format,
                        footprint: capture.footprint,
                        region: capture.region,
                        dest: placed.texture,
                        dest_origin: placed.origin,
                        layout: JobLayout::default(),
                    });
                    self.cache.insert(at, Kept::Slot(slot));
                    Shown::Result(placed)
                }
                BackdropSource::Clear => {
                    self.drop_result(at, pool);
                    self.cache.insert(at, Kept::Clear(held_clear));
                    Shown::Clear(held_clear)
                }
                BackdropSource::Hidden => {
                    // Its quad was pruned, so no fragment reads its slot.
                    self.slots.push(BackdropSlot::default());
                    self.slot_reads.push(None);
                    continue;
                }
                BackdropSource::Held => {
                    let shown = self.cache.get(&at).and_then(|&kept| match kept {
                        Kept::Clear(kept) => Some(Shown::Clear(kept)),
                        Kept::Slot(slot) => place(slot, pool).map(Shown::Result),
                    });
                    if let Some(shown) = shown {
                        shown
                    } else {
                        // Evicted after this window's frame decided, by another window's frame
                        // or by the pool before this one stamped it: the frame shows the clear
                        // and its ledger captures again.
                        self.cache.remove(&at);
                        if !self.evicted.contains(&at) {
                            self.evicted.push(at);
                        }
                        Shown::Clear(held_clear)
                    }
                }
            };
            self.push_slot(row, shown);
        }
        let resources = self.resources.as_mut().expect("made above");
        if !self.jobs.is_empty() {
            resources
                .blur
                .get_or_insert_with(|| BackdropBlur::new(ctx.gpu));
        }
        // A blur made before keeps preparing on a frame with no job, so its scratch decays.
        if let Some(blur) = &mut resources.blur {
            let target = backbuffer.filter(|_| !self.jobs.is_empty()).map(
                |FrameBackbuffer { backbuffer, source }| {
                    source
                        .get_or_insert_with(|| {
                            blur.target_source(
                                ctx.gpu,
                                backbuffer.view(),
                                format,
                                backbuffer.size(),
                            )
                        })
                        .clone()
                },
            );
            blur.prepare(
                ctx,
                &mut self.jobs,
                self.breaks.iter().map(|at| at.jobs.clone()),
                target,
            );
        }
        resources.slot_buffer.upload_instances(ctx, &self.slots);
    }

    /// Make the composite groups follow `textures`, after the pool's commit: again over every
    /// texture where a view of the list or the slot table moved, and one for each scratch texture
    /// this frame's results lie in that has none.
    pub(crate) fn follow(&mut self, gpu: &Gpu, textures: &AtlasTextures) {
        let Some(resources) = &mut self.resources else {
            return;
        };
        let generation = textures.generation(RESULTS);
        let capacity = resources.slot_buffer.capacity();
        let stale = resources
            .groups
            .as_ref()
            .is_none_or(|groups| groups.generation != generation || groups.capacity != capacity);
        if stale {
            let layout = &self.layout;
            resources.groups = Some(CompositeGroups {
                absent: resources.group(gpu, layout, textures.absent_view(RESULTS)),
                pages: std::array::from_fn(|idx| {
                    textures
                        .page_view(RESULTS, idx)
                        .map(|view| resources.group(gpu, layout, view))
                }),
                scratch: Vec::new(),
                generation,
                capacity,
            });
        }
        for read in self.slot_reads.iter().flatten() {
            let ResultTexture::Scratch(id) = *read else {
                continue;
            };
            let slot = id.idx as usize;
            let groups = resources.groups.as_ref().expect("made above");
            if groups.scratch.get(slot).is_none_or(Option::is_none) {
                let view = textures
                    .scratch_view(RESULTS, id.idx)
                    .expect("a held scratch was committed");
                let group = resources.group(gpu, &self.layout, view);
                let scratch = &mut resources.groups.as_mut().expect("made above").scratch;
                if scratch.len() <= slot {
                    scratch.resize_with(slot + 1, || None);
                }
                scratch[slot] = Some(group);
            }
        }
    }

    /// File `row`'s slot of the table, showing `shown`, its composite reading the texture `shown`
    /// lies in.
    fn push_slot(&mut self, row: &BackdropRow, shown: Shown) {
        self.slots.push(BackdropSlot::of(row, shown));
        self.slot_reads.push(match shown {
            Shown::Result(placed) => Some(placed.texture),
            Shown::Clear(_) => None,
        });
    }

    /// Drop the composite groups, as a purge freed textures they bind; the next frame's
    /// [`Self::follow`] makes them again over what its results lie in. Not a follow, which makes
    /// the last frame's scratch groups, over textures the purge may have dropped.
    pub(crate) fn drop_groups(&mut self) {
        if let Some(resources) = &mut self.resources {
            resources.groups = None;
        }
    }

    /// Drop the results the pool evicted since the last drain, and list each for its window's
    /// ledger: at the end of each submit, after the pool's decay, and after a purge, so a window's
    /// next frame captures again what either took, as the other tenants drop their keys. Refused:
    /// the drain in [`Self::prepare`] alone, after the frame's ledger decided, which showed a
    /// result the decay or a purge took as its clear for a frame.
    pub(crate) fn end_frame(&mut self, pool: &mut AtlasPool) {
        self.drained.clear();
        self.drained.extend(pool.drain_evicted(Tenant::Backdrops));
        if self.drained.is_empty() {
            return;
        }
        let Self {
            cache,
            evicted,
            drained,
            ..
        } = self;
        cache.retain(|&at, kept| {
            let gone = matches!(
                kept,
                Kept::Slot(Slot { held: Held::Item(handle), .. }) if drained.contains(handle)
            );
            if gone {
                evicted.push(at);
            }
            !gone
        });
    }

    /// Drop the results of `owner`'s backdrops not in `live`, the backdrops its frame painted or
    /// kept.
    fn evict(&mut self, owner: RenderOwnerId, live: &BackdropKeys, pool: &mut AtlasPool) {
        self.cache.retain(|&(at, key), kept| {
            let keep = at != owner || live.contains(key);
            if !keep && let Kept::Slot(slot) = *kept {
                release(slot, pool);
            }
            keep
        });
    }

    /// Drop every result of `owner`, whose window closed.
    pub(crate) fn retire(&mut self, owner: RenderOwnerId, pool: &mut AtlasPool) {
        self.evict(owner, &BackdropKeys::default(), pool);
        self.forget_evicted(owner);
    }

    /// A slot of `size` for backdrop `at`, and where it lies: its cached one when that holds it,
    /// else a new one rounded up to [`SLOT_GRANULE`], on a page or, past half a page, a scratch
    /// texture of its own. `None` when the pool holds no room for it.
    fn slot_for(
        &mut self,
        at: (RenderOwnerId, BackdropKey),
        size: UVec2,
        pool: &mut AtlasPool,
    ) -> Option<(Slot, Placed)> {
        if let Some(&Kept::Slot(slot)) = self.cache.get(&at)
            && slot.size.cmpge(size).all()
            && let Some(placed) = place(slot, pool)
        {
            return Some((slot, placed));
        }
        self.drop_result(at, pool);
        let size = size.map(|edge| edge.next_multiple_of(SLOT_GRANULE));
        let held = if pool.list(RESULTS).prefers_scratch(size) {
            Held::Scratch(pool.acquire_scratch(RESULTS, size).ok()?)
        } else {
            let edges = U16Vec2::new(u16::try_from(size.x).ok()?, u16::try_from(size.y).ok()?);
            Held::Item(pool.allocate(RESULTS, Tenant::Backdrops, edges).ok()?)
        };
        let slot = Slot { held, size };
        Some((slot, place(slot, pool).expect("a new slot is placed")))
    }

    /// Drop backdrop `at`'s cached result, giving its texels back to `pool`.
    fn drop_result(&mut self, at: (RenderOwnerId, BackdropKey), pool: &mut AtlasPool) {
        if let Some(Kept::Slot(slot)) = self.cache.remove(&at) {
            release(slot, pool);
        }
    }

    /// The backdrops of `owner` whose results went, which its backdrop ledger must stop holding.
    pub(crate) fn evicted(&self, owner: RenderOwnerId) -> impl Iterator<Item = BackdropKey> {
        self.evicted
            .iter()
            .filter(move |(at, _)| *at == owner)
            .map(|&(_, key)| key)
    }

    /// Forget the evictions [`Self::evicted`] listed for `owner`, which its ledger took.
    pub(crate) fn forget_evicted(&mut self, owner: RenderOwnerId) {
        self.evicted.retain(|(at, _)| *at != owner);
    }

    /// The breaks of the main pass this frame's blurs run at, in paint order.
    pub(crate) fn breaks(&self) -> &[BlurBreak] {
        &self.breaks
    }

    /// Copy the footprints of break `at`'s backdrops out of `color`, the main pass's target, into
    /// their capture; nothing where the blurs read the target itself.
    pub(crate) fn copy_break(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        at: &BlurBreak,
        color: &wgpu::Texture,
    ) {
        let blur = self.blur();
        for job in &self.jobs[at.jobs.clone()] {
            blur.copy_capture(encoder, job, color);
        }
    }

    /// Record the blurs of break `at` from their capture into their slots of `textures`.
    pub(crate) fn blur_break(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        at: &BlurBreak,
        textures: &AtlasTextures,
    ) {
        let pages = textures.views(RESULTS);
        self.blur()
            .encode(encoder, &self.jobs[at.jobs.clone()], |dest| match dest {
                ResultTexture::Page(page) => pages[usize::from(page)],
                ResultTexture::Scratch(id) => textures
                    .scratch_view(RESULTS, id.idx)
                    .expect("a held scratch was committed"),
            });
    }

    /// The blur of a frame that blurs, which its prepare made.
    fn blur(&self) -> &BackdropBlur {
        self.resources
            .as_ref()
            .and_then(|resources| resources.blur.as_ref())
            .expect("a frame that blurs made its blur")
    }
}

/// Where `slot`'s texels lie in `pool` now, `None` once the pool evicted them.
fn place(slot: Slot, pool: &AtlasPool) -> Option<Placed> {
    match slot.held {
        Held::Item(handle) => {
            let placement = pool.placement(handle)?;
            Some(Placed {
                texture: ResultTexture::Page(placement.page),
                origin: placement.origin.as_uvec2(),
            })
        }
        Held::Scratch(id) => Some(Placed {
            texture: ResultTexture::Scratch(id),
            origin: UVec2::ZERO,
        }),
    }
}

/// Give `slot`'s texels back to `pool`, unless it evicted them, their handle waiting for the
/// drain, which finds no result holding it.
fn release(slot: Slot, pool: &mut AtlasPool) {
    match slot.held {
        Held::Item(handle) => {
            if pool.placement(handle).is_some() {
                pool.release(handle);
            }
        }
        Held::Scratch(id) => pool.release_scratch(RESULTS, id),
    }
}

/// The linear color a target of `format` holds where it is cleared to `clear`, as a backdrop's
/// capture reads it back, and rounded to `f16` as its blur stores it: an 8-bit sRGB target rounds
/// to its byte, so the backdrop matches the clear around it. A float target keeps the `f16` clear;
/// one narrower, `Rg11b10Ufloat`, rounds again where the backdrop writes.
fn held_clear(clear: RgbaF16, format: TargetFormat) -> [f32; 3] {
    let clear = RgbaF32::from(clear);
    let held = if format.get().is_srgb() {
        RgbaF32::from(RgbaF32::from(clear.to_srgba8()).to_f16())
    } else {
        clear
    };
    [held.r, held.g, held.b]
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::atlas_memory::AtlasUse;
    use crate::gpu::backdrop::backdrops::Backdrops;

    impl Backdrops {
        /// The memory of the blur's textures; none before the first frame that blurs.
        pub(crate) fn blur_memory(&self) -> AtlasUse {
            const BLUR: &str = "backdrop blur";
            self.resources
                .as_ref()
                .and_then(|resources| resources.blur.as_ref())
                .map_or_else(|| AtlasUse::scratch(BLUR, None), |blur| blur.memory(BLUR))
        }

        /// The blurs the last prepared frame ran.
        pub(crate) const fn blurs(&self) -> usize {
            self.jobs.len()
        }
    }
}

#[cfg(test)]
mod tests;
