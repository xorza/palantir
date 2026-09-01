//! The wgpu backend: the one GPU renderer, its per-window attachments,
//! and the pipeline sets it builds per swapchain format.
//!
//! # One frame
//!
//! [`WgpuBackend::submit`] draws a frame in two halves that cannot
//! overlap, because the first holds `&mut self` and the second reads
//! `self.pipelines` through a shared borrow: an upload phase recording
//! every texture and dynamic-buffer write the passes will read, then the
//! render passes themselves.
//!
//! Every instanced pipeline here spells the same six steps under the
//! same names — `new`, `instance_layout`, `build_variants`, `upload`,
//! `bind`, `draw` — so a reader who knows one knows the rest, and the
//! render loop's arms differ only where the pipelines genuinely do. A
//! new pipeline is expected to keep those names. The two raster tenants
//! are the exception: text and icon share one
//! [`RasterPass`](crate::renderer::backend::raster_pass::RasterPass)
//! implementation rather than two copies of the shape.
//!
//! Quads and text interleave per-group in paint order: each group's
//! quads draw first, then its text renders on top, before the next group
//! runs. So a child quad declared *after* a label correctly occludes
//! that label. Without a shared shaper installed (mono fallback) text
//! rendering is silently skipped and the frame still draws its quads.
//!
//! # Damage
//!
//! Two paths, branching on the frame plan's damage region:
//!
//! - [`Damage::Full`](crate::scene::damage::Damage::Full): a single
//!   `LoadOp::Clear(clear)` pass paints every group at its native
//!   scissor. First frame, post-resize, post-format-change, and
//!   coverage-above-threshold all land here.
//! - [`Damage::Partial(region)`](crate::scene::damage::Damage::Partial):
//!   one render pass per rect in the region. Each pass `LoadOp::Load`s
//!   the backbuffer (preserving last frame outside the rect) and the
//!   schedule narrows every group's scissor to that pass's damage rect.
//!   Logical-px in; the backend scales, pads for AA bleed, and clamps to
//!   surface, and rects that clamp to zero area are filtered out. The
//!   rects are pairwise disjoint, so one stencil clear per pass is
//!   enough — no per-rect reset.
//!
//! Both shapes go through one `begin_render_pass`. The `dim_undamaged`
//! debug mode adds a pre-pass on Partial frames: one full-viewport
//! 40%-translucent black quad onto the backbuffer with `LoadOp::Load`,
//! in a render pass of its own because the no-stencil pipeline is
//! incompatible with the main pass's stencil attachment on rounded-clip
//! frames. Undamaged pixels are dimmed once per frame; damaged pixels
//! are dimmed and then immediately overwritten by the fresh repaint. So
//! across many frames the static background fades toward black while
//! moving content stays current — far less jarring than the prior
//! `LoadOp::Clear` flash, and it pins which regions actually repaint.
//!
//! # The staging belt
//!
//! The main encoder opens before the first upload. Every dynamic-buffer
//! upload routes through `staging_belt`, which schedules its
//! `copy_buffer_to_buffer` commands onto that encoder rather than
//! allocating its own `MTLBlitCommandEncoder` per `queue.write_buffer`.
//! The render passes record onto the same encoder later, and wgpu
//! serialises commands in record order, so the copies land before the
//! passes that read from the destination buffers.
//!
//! Closing the belt with `finish_and_recall_on_submit` records a
//! `map_buffer_on_submit` onto the encoder, so the just-used chunks
//! re-map once the submission completes — no explicit `recall()`. It has
//! to precede `encoder.finish()`, which needs the still-live encoder.
//! Chunks come back when the map callback fires off a `device.poll`: a
//! `PollType::Wait` caller sees them next frame, and a `PollType::Poll`
//! caller may allocate one more chunk during the catch-up window, which
//! wgpu's docs flag as harmless.
//!
//! # The clear
//!
//! The surface clear is the bottom-most paint layer of the frame, so its
//! alpha is forced to 1: any sub-1 alpha would let the host's desktop
//! show through the framebuffer's transparent regions. Palantir doesn't
//! support transparent windows, and the occlusion prune assumes the
//! clear is opaque.
//!
//! The composer may have folded a viewport-covering root background quad
//! into the clear (`RenderBuffer::clear_override`). It then replaces the
//! plan's clear for both the Full-pass `LoadOp::Clear` and the Partial
//! pre-clear quad.
//!
//! # GPU timestamps
//!
//! When timing is on, the main-pass timestamps resolve as the last step
//! before `encoder.finish()`: the main pass closed before the backbuffer
//! copy, so the resolve rides in the same command buffer as everything
//! else. After submission, `after_submit` kicks the `map_async` on this
//! frame's staging slot and reads back any prior frame whose map
//! completed — one `device.poll(Poll)` and one memcpy on the ready slot.

pub(crate) mod backbuffer;
pub(crate) mod backend_config;
pub(crate) mod backend_resources;
#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod curve_pipeline;
mod debug_marker;
mod dynamic_buffer;
mod format_pipelines;
mod gpu_ctx;
mod gpu_gradient_atlas;
mod gpu_timings;
pub(crate) mod icon;
pub(crate) mod image_pipeline;
mod image_textures;
mod mesh_pipeline;
mod overlay_pass;
mod pipeline_recipe;
mod quad_pipeline;
pub(crate) mod raster_atlas;
mod raster_pass;
// `pub(crate)` only so `bench::driver` — the crate-root facade the
// external criterion target calls through — can name `schedule::bench`.
pub(crate) mod schedule;
mod shader_template;
pub(crate) mod stencil;
mod stencil_variant;
pub(crate) mod submission;
pub(crate) mod text;
mod texture_binding;
pub(crate) mod texture_region;
mod viewport;

use crate::common::tracy;
use crate::diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats};
use crate::primitives::color::Color;
use crate::primitives::urect::URect;
use crate::renderer::backend::backbuffer::Backbuffer;
use crate::renderer::backend::backend_config::BackendConfig;
use crate::renderer::backend::backend_resources::BackendResources;
use crate::renderer::backend::curve_pipeline::CurvePipeline;
use crate::renderer::backend::format_pipelines::FormatPipelines;
use crate::renderer::backend::format_pipelines::PipelineSources;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::gpu_gradient_atlas::GpuGradientAtlas;
use crate::renderer::backend::gpu_timings::GpuTimings;
use crate::renderer::backend::icon::IconBackend;
use crate::renderer::backend::image_pipeline::{ImageBatch, ImagePipeline};
use crate::renderer::backend::image_textures::ImageTextures;
use crate::renderer::backend::mesh_pipeline::{MeshBatch, MeshPipeline, MeshUpload};
use crate::renderer::backend::overlay_pass::DebugOverlay;
use crate::renderer::backend::quad_pipeline::QuadPipeline;
use crate::renderer::backend::schedule::{RenderStep, for_each_step};
use crate::renderer::backend::stencil::Stencil;
use crate::renderer::backend::submission::{Submission, SubmissionTargets};
use crate::renderer::backend::text::TextBackend;
use crate::renderer::backend::viewport::{RepaintScissors, ViewportPush, build_repaint_scissors};
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_owner_id::RenderOwnerId;
use rustc_hash::FxHashMap;
use std::time::Instant;
use wgpu::util::StagingBelt;

/// Size of the per-pipeline immediate (push-constant) region every
/// palantir shader reads, through the one `var<immediate> imm:
/// Immediates` in `prelude.wgsl`. Locked at the maximum used by any
/// pipeline so a `set_immediates` for one shader stays valid across
/// pipeline switches:
///
/// - offset 0 (8 bytes): [`ViewportPush`] — viewport size, written
///   once per pass by `WgpuBackend`.
/// - offset 8 (8 bytes): `text::Params` — atlas dimensions,
///   written per text batch by `RasterPass::render_batch`.
///
/// Pipelines that don't read the tail (quad/mesh/image/curve) still
/// declare `immediate_size = IMMEDIATES_BYTES` so the immediate-state
/// layout matches and bytes written by other pipelines stay valid
/// after a pipeline switch.
const IMMEDIATES_BYTES: u32 = 16;

/// The two things [`WgpuBackend::submit`] settles about a frame before it
/// opens the encoder. Each combines more than one field, so deriving it
/// again downstream would be a second derivation rather than a second
/// read; anything the upload phase can read straight off the
/// [`Submission`] stays there, and it is handed over whole.
#[derive(Clone, Copy, Debug)]
struct UploadPlan {
    /// Effective clear colour after `RenderBuffer::clear_override`.
    clear: Color,
    /// The debug flag, and only on a frame it can apply to.
    dim_undamaged: bool,
}

/// Wgpu renderer owning its device/queue handles, pipelines, and text
/// backend. The winit adapter retains cloned handles solely for surface
/// configuration and presentation.
///
/// The text side holds the same
/// [`TextShaper`](crate::text::shaper::TextShaper) the `Ui` side
/// measures against (passed in at [`Self::new`]), so layout-time
/// measurement and rasterization hit one buffer cache. No layout, no encode, no compose
/// — those happen elsewhere and arrive here as a `RenderBuffer`.
#[derive(Debug)]
pub(crate) struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// All per-frame dynamic-buffer uploads route through this belt so
    /// the resulting `copy_buffer_to_buffer` commands ride the main
    /// encoder. On Metal that collapses N `queue.write_buffer` calls
    /// (each spinning up a fresh `MTLBlitCommandEncoder`) down to one
    /// blit encoder per submit. Chunk size sized to comfortably hold a
    /// resizing-frame's worth of buffer uploads (~512 KB observed in
    /// the frame bench).
    staging_belt: StagingBelt,
    /// Shared gradient LUT atlas resources (texture + sampler + group-0
    /// bind group), lent to the quad and curve pipelines — both render
    /// gradient brushes off this one allocation.
    gradient: GpuGradientAtlas,
    quad: QuadPipeline,
    mesh: MeshPipeline,
    image: ImagePipeline,
    /// Every GPU texture a draw may sample — registered images and the
    /// framework's `GpuView` targets — plus the group-0 layout and
    /// sampler they share. Owned beside the pipeline rather than inside
    /// it, so `paint_gpu_views` and `retire_owner` are reached without a
    /// forwarder and `draw` is handed the store it binds from.
    image_textures: ImageTextures,
    icon: IconBackend,
    curve: CurvePipeline,
    text: TextBackend,
    debug: DebugOverlay,
    /// Format-dependent render pipelines, keyed by swapchain color format
    /// and built lazily ([`Self::ensure_format`]) the first time a
    /// surface of that format is submitted. Windows on different-format
    /// outputs (e.g. one sRGB, one HDR) each bind their own set while
    /// sharing every format-independent resource above. The only state
    /// that carries the color target; there is no single "current format"
    /// — the surface texture handed to `submit` selects the set.
    pipelines: FxHashMap<wgpu::TextureFormat, FormatPipelines>,
    /// Shared image lifecycle drained each frame for uploads and releases.
    images: ImageRegistry,
    /// Main-pass timestamp queries. `Some` when the host opted into
    /// instrumentation and the device was created with `TIMESTAMP_QUERY`
    /// enabled. Publishes into the host's shared `GpuPassStats` handle;
    /// with one shared backend the published sample reflects the most recently
    /// submitted window.
    gpu_timings: Option<GpuTimings>,
    /// The one handle this backend publishes through — its host-side
    /// record time on every submitted frame, and, when `gpu_timings` is
    /// resolving, that sample too. `GpuTimings` is handed this at
    /// `after_submit` rather than keeping a clone, so the two cannot come
    /// to name different sinks.
    ///
    /// The record time is unconditional rather than behind
    /// [`BackendConfig`]: the measurement is two `Instant::now()` calls
    /// and one `RefCell` write *per frame*, and making it opt-in would
    /// mean the only way to read it is to also enable the in-pass
    /// timestamp writes that perturb the very number it reports.
    pass_stats: GpuPassStats,
}

/// What [`WgpuBackend::run_main_pass`] draws into: the color attachment, the
/// stencil attachment when the frame uses rounded clipping, and the color the
/// pass clears to. One frame's attachments are picked together — backbuffer vs.
/// surface view, stencil or no stencil — so they arrive together.
#[derive(Debug)]
struct PassTarget<'a> {
    color_view: &'a wgpu::TextureView,
    stencil_view: Option<&'a wgpu::TextureView>,
    clear: wgpu::Color,
}

impl WgpuBackend {
    /// Build the one shared GPU renderer from its app-global render handles.
    /// Owns the device/queue and every
    /// format-independent GPU resource (pipelines' shaders + buffers, the
    /// glyph + gradient atlases, the image texture cache). Format-agnostic
    /// at construction: each swapchain format's pipeline set builds lazily
    /// on the first submit that targets it (see [`Self::ensure_format`]).
    pub(crate) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        resources: BackendResources,
        config: BackendConfig,
    ) -> Self {
        // Gradient LUT atlas resources, shared by the quad and curve
        // pipelines (both sample gradient brushes). Owned here so neither
        // pipeline owns the other's input — each composes its layout
        // against `gradient.bgl` and binds `gradient.bg`.
        let gradient = GpuGradientAtlas::new(&device, resources.gradient_atlas);
        let quad = QuadPipeline::new(&device);
        let mesh = MeshPipeline::new(&device);
        let image = ImagePipeline::new(&device);
        let image_textures = ImageTextures::new(&device);
        let curve = CurvePipeline::new(&device);
        let text = TextBackend::new(&device, resources.text);
        let icon = IconBackend::new(&device, resources.icons);
        let debug = DebugOverlay::new(&device);
        // Per-format pipeline sets build lazily on the first submit that
        // targets each format (`ensure_format`); none at construction.
        let pipelines = FxHashMap::default();
        // 1 MiB chunks: comfortably above the resizing-arm's ~500 KB
        // per-frame upload peak, so we land in 1-2 chunks during
        // steady state. wgpu allocates a new chunk only when the
        // active one can't fit a write.
        let staging_belt = StagingBelt::new(device.clone(), 1 << 20);
        let features = device.features();
        let timestamp_period = queue.get_timestamp_period();
        let gpu_timings = (config.collect_gpu_stats
            && features.contains(wgpu::Features::TIMESTAMP_QUERY)
            && timestamp_period > 0.0)
            .then(|| {
                GpuTimings::new(
                    &device,
                    timestamp_period,
                    features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
                    features.contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
                )
            });
        Self {
            device,
            queue,
            staging_belt,
            gradient,
            quad,
            mesh,
            image,
            image_textures,
            icon,
            curve,
            text,
            debug,
            pipelines,
            images: resources.images,
            gpu_timings,
            pass_stats: resources.gpu_pass_stats,
        }
    }

    /// Ensure the pipeline set for `format` exists, building + caching it
    /// on first use. Callers then read it back with `&self.pipelines[&format]`
    /// (a shared field borrow, so it doesn't conflict with the `&mut self`
    /// upload phase). Only the `wgpu::RenderPipeline` objects carry the
    /// color-target format; every format-independent resource (image
    /// textures, glyph + gradient atlases, samplers, buffers) lives on the
    /// shared resource structs, so a new format costs only a handful of
    /// pipeline compiles — **no image re-upload or glyph re-rasterization**.
    /// Windows on different-format outputs each get (and keep) their own set.
    fn ensure_format(&mut self, format: wgpu::TextureFormat) {
        // Split borrow: the resource structs the builder reads are
        // disjoint from `self.pipelines`, but the borrow checker can't see
        // that through `entry().or_insert_with(closure)`, so build first
        // then insert.
        if !self.pipelines.contains_key(&format) {
            let built = FormatPipelines::new(
                &self.device,
                format,
                PipelineSources {
                    gradient_bgl: &self.gradient.bgl,
                    image_bgl: self.image_textures.layout(),
                    quad: &self.quad,
                    mesh: &self.mesh,
                    image: &self.image,
                    icon: &self.icon,
                    curve: &self.curve,
                    text: &self.text,
                },
            );
            self.pipelines.insert(format, built);
        }
    }

    /// Render one frame into the submission's target and present it.
    ///
    /// The module docs carry the frame's shape: the two halves, the two
    /// damage paths, the belt and the timestamp resolve.
    ///
    /// Skip frames never reach this method — `WindowDriver::render_to_texture`
    /// dispatches them to the copy / no-op paths.
    ///
    /// [`SubmissionTargets::backbuffer`] picks the path. `Some` renders
    /// into that backbuffer and copies the result onto
    /// [`surface`](SubmissionTargets::surface), whose texture must carry
    /// `COPY_DST` usage (set in [`wgpu::SurfaceConfiguration::usage`]);
    /// `None` renders straight into the surface (direct present).
    /// [`Submission::plan`] is the *effective* plan — every escalation
    /// (promote / resync) was sealed in `present_mode` *before* the draw
    /// list was built, so the plan and the buffer always agree. The
    /// caller (`WindowDriver`) has also ensured the stencil and the
    /// backbuffer.
    pub(crate) fn submit(&mut self, submission: Submission<'_>) {
        tracy::zone!();
        let SubmissionTargets {
            surface: surface_tex,
            backbuffer: via_backbuffer,
            stencil,
        } = submission.targets;
        let Submission {
            buffer,
            plan,
            debug_overlay,
            ..
        } = submission;
        let clear = buffer.clear_override.unwrap_or(plan.clear);
        let stencil_view = stencil.map(Stencil::view);
        let use_stencil = stencil_view.is_some();
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.display.physical,
            requested_plan = ?plan,
            rounded_clip = use_stencil,
            "wgpu_backend.submit"
        );

        // Build (once) + select the pipeline set for this surface's
        // format. Read back as `&self.pipelines[&format]` after the
        // `&mut self` upload phase so the borrows don't collide.
        let format = surface_tex.format();
        self.ensure_format(format);

        let viewport = ViewportPush::for_buffer(buffer);
        let repaint_scissors = build_repaint_scissors(plan.damage, buffer);
        let is_partial = plan.damage.is_partial();
        let dim_undamaged = debug_overlay.dim_undamaged && is_partial;

        // The stencil texture (rounded-clip masking) is ensured by the
        // caller; `stencil_view` is `Some` exactly when `use_stencil`. The
        // mask quads upload further down, after the encoder is open.

        // One encoder: the belt's copies must land before the passes that
        // read them.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.main"),
            });

        let overlay_count = self.upload_frame(
            &mut encoder,
            &submission,
            UploadPlan {
                clear,
                dim_undamaged,
            },
        );

        // Alpha forced to 1 — the clear is the frame's bottom paint layer.
        let clear_color = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: 1.0,
        };
        // Shared field borrow (the entry was built by `ensure_format`
        // above) — coexists with the `&self` pass methods.
        let fmt = &self.pipelines[&format];
        // One view of the surface at most, and only where a pass reads
        // it: the direct-present path paints into it, and the overlay
        // pass lands on it after the backbuffer copy. Building it
        // unconditionally would add a view per frame to the backbuffer
        // path, which is the path a normal run takes.
        let surface_view = (via_backbuffer.is_none() || overlay_count > 0)
            .then(|| surface_tex.create_view(&wgpu::TextureViewDescriptor::default()));
        let color_view: &wgpu::TextureView = match via_backbuffer {
            Some(bb) => bb.view(),
            None => surface_view
                .as_ref()
                .expect("direct present builds the surface view"),
        };
        if let RepaintScissors::Partial(rects) = &repaint_scissors {
            tracing::trace!(rects = rects.len(), "wgpu_backend.submit.pass.partial");
        } else {
            tracing::trace!("wgpu_backend.submit.pass.full");
        }
        if dim_undamaged {
            tracing::trace!("wgpu_backend.submit.pass.dim");
            self.run_dim_pass(fmt, color_view, &mut encoder, viewport);
        }
        self.run_main_pass(
            fmt,
            PassTarget {
                color_view,
                stencil_view,
                clear: clear_color,
            },
            &mut encoder,
            buffer,
            &repaint_scissors,
        );

        if let Some(bb) = via_backbuffer {
            bb.copy_onto(&mut encoder, surface_tex);
        }

        if overlay_count > 0 {
            let view = surface_view
                .as_ref()
                .expect("a non-empty overlay builds the surface view");
            self.run_overlay_pass(fmt, view, &mut encoder, viewport, overlay_count);
        }

        if let Some(t) = self.gpu_timings.as_mut() {
            t.resolve(&mut encoder);
        }

        self.staging_belt.finish_and_recall_on_submit(&encoder);
        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some(t) = self.gpu_timings.as_mut() {
            t.after_submit(&self.device, &self.pass_stats);
        }

        let frame = self.text.frame();
        self.text.end_frame();
        self.icon.end_frame(frame);
    }

    /// The belt-routed upload phase of one [`Self::submit`]: every
    /// texture and dynamic-buffer write the frame's passes will read,
    /// recorded onto `encoder` before any render pass opens. Returns the
    /// damage-overlay instance count for the post-copy overlay pass.
    fn upload_frame(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        sub: &Submission<'_>,
        uploads: UploadPlan,
    ) -> u32 {
        let Submission {
            owner,
            targets,
            payloads,
            buffer,
            plan,
            debug_overlay,
        } = *sub;
        let UploadPlan {
            clear,
            dim_undamaged,
        } = uploads;
        // Both read off the submission rather than carried beside it: it
        // already says whether there is a stencil attachment and what the
        // plan repaints, and a copy alongside is a second route to one
        // fact.
        let use_stencil = targets.stencil.is_some();
        let is_partial = plan.damage.is_partial();
        let mut ctx = GpuCtx::new(&self.device, &self.queue, &mut self.staging_belt, encoder);

        // Texture-only uploads (the belt is buffer-only). Run
        // first so any draws below see the right pixels:
        // - gradient LUT atlas: idle frames drain an empty dirty
        //   flag and do nothing; first frame uploads row 0's
        //   magenta fallback plus any baked rows composer queued.
        // - image registry: first-frame images need a bind group
        //   ready when the schedule's draw call lands.
        self.gradient.upload(&ctx);
        self.image_textures.drain_registry(&mut ctx, &self.images);

        if dim_undamaged {
            self.debug
                .upload_dim(&mut ctx, buffer.display.physical.as_vec2());
        }
        // Damage-rect overlay quads (debug). Uploaded alongside
        // everything else; the overlay pass itself runs last, after
        // the backbuffer→surface copy — same upload-early /
        // draw-late split as the dim quad above.
        let overlay_count = if debug_overlay.damage_rect {
            self.debug.upload_damage_rects(&mut ctx, plan, buffer)
        } else {
            0
        };
        if use_stencil {
            // After staging, `self.quad.mask_indices` parallels
            // `buffer.groups` / `buffer.text_batches` and
            // `render_groups` reads it directly.
            self.quad.stage_masks(&mut ctx, buffer);
        }

        self.quad.upload(&mut ctx, &buffer.quads);
        self.mesh.upload(
            &mut ctx,
            MeshUpload {
                vertices: &payloads.meshes.vertices,
                indices: &payloads.meshes.indices,
                instances: buffer.meshes.instance(),
            },
        );
        self.image.upload(&mut ctx, buffer.images.instance());
        // Paint every GpuView composited this frame into its off-screen
        // target on this same encoder, before the main pass samples it.
        // The composer listed them in `buffer.frame_targets` (size + scales
        // + paint callback); this allocates each + runs its callback, then
        // frees this submitter's targets absent from `buffer.live_targets`
        // — every view the frame *recorded*, which is a wider set than the
        // ones it painted, so an unchanged view keeps its texture
        // (eviction is owner-scoped — the shared backend serves every
        // window).
        // `submit` itself carries no render-target logic.
        self.image_textures.paint_gpu_views(
            &mut ctx,
            buffer.frame_views(),
            owner,
            buffer.time,
            self.text.shaper(),
        );
        self.curve.upload(&mut ctx, &buffer.curves);

        if is_partial {
            self.quad
                .upload_clear(&mut ctx, buffer.display.physical.as_vec2(), clear);
        }

        // Text prepare: per-batch glyph encoding. Routes its
        // vertex/atlas-staging writes through the same ctx so
        // every text-backend write lands as
        // `copy_buffer_to_buffer` on the main encoder. Viewport
        // and atlas-size params ride the shared immediate region,
        // pushed per batch by `RasterPass::render_batch` — no
        // per-frame sync from here.
        {
            tracy::zone!(
                "text.prepare_batches",
                value = buffer.text_batches.len() as u64
            );
            let interned_text = payloads.interned_text();
            for (i, b) in buffer.text_batches.iter().enumerate() {
                let runs = &buffer.texts[b.texts.range()];
                self.text.prepare_batch(
                    &mut ctx,
                    buffer.display.scale_factor,
                    i,
                    runs,
                    &interned_text,
                );
            }
        }

        // One deferred vbuf write covering every batch prepared
        // above, then the queued glyph-atlas uploads (grow blits +
        // per-glyph copy_buffer_to_texture) on the same encoder so
        // they share the main render submit. The staging side of
        // those copies also routes through the belt — see
        // `RasterPass::flush` / `atlas::flush_pending_uploads`.
        self.text.pass.flush(&mut ctx);

        // Icons: prewarm any filtered icon at this frame's scale (an SVG
        // filter is 10-20x an ordinary raster, so meeting one lazily is a
        // dropped frame), then encode each batch, rasterizing misses
        // inline the way the text prepare does.
        {
            tracy::zone!(
                "icon.prepare_batches",
                value = buffer.batches(PaintTier::Icon).len() as u64
            );
            self.icon.prewarm(&mut ctx, buffer.display.scale_factor);
            for (i, b) in buffer.batches(PaintTier::Icon).iter().enumerate() {
                let rows = &buffer.icons[b.items.range()];
                self.icon.prepare_batch(&mut ctx, i, rows);
            }
        }
        self.icon.pass.flush(&mut ctx);

        overlay_count
    }

    /// Full-viewport pass that draws one 40%-translucent black quad
    /// over the backbuffer with `LoadOp::Load`. Runs before partial
    /// damage passes when the debug `dim_undamaged` flag is on (see
    /// `dim_undamaged` in [`Self::submit`]). No stencil attachment
    /// even when the frame uses rounded clipping — the dim quad
    /// paints uniformly and subsequent partial passes set their own.
    fn run_dim_pass(
        &self,
        fmt: &FormatPipelines,
        color_view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        viewport: ViewportPush,
    ) {
        let mut pass = begin_load_pass(encoder, "palantir.renderer.dim.pass", color_view);
        self.debug.draw_dim(
            &mut pass,
            fmt.quad.color.select(false),
            &self.gradient.bg,
            &viewport,
        );
    }

    /// Open the main render pass against the backbuffer and walk the
    /// schedule once per damage rect (or once with no scissor on Full).
    /// All rects share one pass: one `begin_render_pass`, one stencil
    /// `LoadOp::Clear(0)`, one color load. Per-rect work is just a
    /// `SetScissor` + the schedule's group walk (plus the schedule's
    /// own per-rect `PreClear` quad on Partial).
    ///
    /// Every schedule walk leaves the stencil clean: a walk that ends
    /// with a mask stamped emits a tail clear under the stamp's
    /// scissor. That — not rect disjointness — is what keeps one
    /// rect's stencil writes out of a later rect's reads:
    /// `RenderPlan::AA_PADDING` can make nominally-disjoint rects' padded
    /// scissors overlap, and the stencil clears once per pass. Each
    /// `render_groups` call's fresh `active_mask = None` therefore
    /// always matches the true stencil contents.
    ///
    /// `RepaintScissors::Full` runs one schedule walk with no damage
    /// scissor and clears the whole backbuffer. `Partial` loads the
    /// prior color and runs once per non-empty scissor.
    ///
    /// Host CPU time for the whole of this — pass open, every recorded
    /// draw step, and the end-of-pass command replay that `pass`'s drop
    /// runs — publishes to
    /// [`GpuPassStats::last_main_pass_cpu_ms`]. It is the one frame cost
    /// that scales with draw-step *count* rather than pixel count, so it
    /// is the metric the `record_pass` benchmark reads.
    fn run_main_pass(
        &self,
        fmt: &FormatPipelines,
        target: PassTarget<'_>,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &RenderBuffer,
        repaint_scissors: &RepaintScissors,
    ) {
        tracy::zone!();
        let PassTarget {
            color_view,
            stencil_view,
            clear,
        } = target;
        let use_stencil = stencil_view.is_some();
        let depth_stencil_attachment =
            stencil_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    // One stencil clear per *pass*, not per rect. What
                    // makes that sufficient is the schedule's tail
                    // clear (see the method doc), not rect
                    // disjointness: `RenderPlan::AA_PADDING` can make
                    // nominally-disjoint rects' scissors overlap.
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            });
        let load_op = match repaint_scissors {
            RepaintScissors::Full => wgpu::LoadOp::Clear(clear),
            RepaintScissors::Partial(_) => wgpu::LoadOp::Load,
        };
        // Timestamp writes via the descriptor cover the basic mode
        // (TIMESTAMP_QUERY only — pass begin / end). In per-batch
        // mode (TIMESTAMP_QUERY_INSIDE_PASSES additionally on) we
        // skip the descriptor and write begin/end inline via
        // `pass_begin` / `pass_end` so a single sequential timestamp
        // stream covers begin → midpoints → end without index gaps.
        let timestamp_writes = self.gpu_timings.as_ref().and_then(|t| t.pass_writes());
        let started = Instant::now();
        // Scoped so `pass` drops — replaying its recorded commands into the
        // encoder — inside the measured window rather than after it.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.renderer.main.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment,
                timestamp_writes,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(t) = &self.gpu_timings {
                t.pass_begin(&mut pass);
                t.begin_pipeline_stats(&mut pass);
            }
            match repaint_scissors {
                RepaintScissors::Full => {
                    self.render_groups(fmt, &mut pass, buffer, None, use_stencil)
                }
                RepaintScissors::Partial(rects) => {
                    let rect_count = rects.len();
                    for (i, r) in rects.iter().enumerate() {
                        tracing::trace!(
                            rect = i,
                            of = rect_count,
                            scissor = ?r,
                            "wgpu_backend.submit.pass.partial_rect"
                        );
                        self.render_groups(fmt, &mut pass, buffer, Some(r), use_stencil);
                    }
                }
            }
            if let Some(t) = &self.gpu_timings {
                t.end_pipeline_stats(&mut pass);
                t.pass_end(&mut pass);
            }
        }
        self.pass_stats
            .record_main_pass_cpu_ns(started.elapsed().as_nanos() as u64);
    }

    /// Dispatch every step in the per-frame schedule
    /// ([`schedule::for_each_step`]) to the wgpu render pass. Logic
    /// for *what* runs in *what order* lives in the schedule module;
    /// this method is purely the wgpu translation layer for each
    /// `RenderStep`. Tests reuse the same schedule emitter to assert
    /// on the sequence without GPU.
    fn render_groups<'a>(
        &'a self,
        fmt: &'a FormatPipelines,
        pass: &mut wgpu::RenderPass<'a>,
        buffer: &RenderBuffer,
        damage_scissor: Option<URect>,
        use_stencil: bool,
    ) {
        tracy::zone!();
        // Track what pipeline + vertex buffer is currently bound so we
        // can skip redundant `set_pipeline` / `set_vertex_buffer` calls
        // across consecutive same-kind steps. wgpu records every
        // `set_pipeline` as a real command — drivers don't dedupe.
        // `PreClear` and the text backend's render set their own state,
        // so we reset to `None` after them and re-bind on the next
        // non-text step.
        #[derive(Debug, PartialEq, Eq)]
        enum Bound {
            None,
            QuadInstance,
            Mesh,
            Image,
            Curve,
            MaskStamp,
            MaskClear,
        }
        let mut bound = Bound::None;
        let viewport = ViewportPush::for_buffer(buffer);

        // Helper: thread a `BatchKind` marker through to `GpuTimings`
        // when per-batch timestamps are enabled. Coalesced inside
        // `GpuTimings::mark` — same-kind repeats are free, only true
        // transitions write a `RenderPass::write_timestamp`.
        let mark = |pass: &mut wgpu::RenderPass<'a>, kind: BatchKind| {
            if let Some(t) = self.gpu_timings.as_ref() {
                t.mark(pass, kind);
            }
        };

        // `viewport.push_into(pass)` is called after every (re)bind
        // below. Cheap (register-mapped `set_immediates`, no buffer
        // round-trip) and dodges the immediate-state-survives-pipeline-
        // switch contract entirely — wgpu's IMMEDIATES feature claims
        // it does, but the symptom of a missed push is silent NDC
        // corruption (wrong-scaled quads painting outside their
        // damage scissor). Re-push is the unambiguous fix.
        //
        // `rebind!` bundles the "bind ⇒ re-push viewport ⇒ record bound"
        // triple so no draw arm can bind a pipeline and forget the
        // viewport push. Arms that set their own state and reset `bound`
        // to `None` (PreClear, Text) stay open-coded.
        macro_rules! rebind {
            ($target:expr, $bind:expr) => {
                if bound != $target {
                    $bind;
                    viewport.push_into(pass);
                    bound = $target;
                }
            };
        }

        for_each_step(
            buffer,
            damage_scissor,
            &self.quad.mask_indices,
            use_stencil,
            &mut |step| match step {
                RenderStep::PreClear => {
                    mark(pass, BatchKind::PreClear);
                    debug_marker::push(pass, "preclear");
                    // bind → push viewport → draw. Pushing after the
                    // draw (or skipping it) leaves the clear quad
                    // reading whatever's in the immediate region —
                    // zero on the first PreClear of a partial pass,
                    // which lands the quad at garbage NDC and skips
                    // the damage-region clear.
                    self.quad
                        .bind_clear(pass, &fmt.quad.color, use_stencil, &self.gradient.bg);
                    viewport.push_into(pass);
                    pass.draw(0..4, 0..1);
                    // Distinct vertex buffer (clear_buffer); next
                    // non-clear step re-binds.
                    bound = Bound::None;
                    debug_marker::pop(pass);
                }
                RenderStep::SetScissor(r) => {
                    pass.set_scissor_rect(r.min.x, r.min.y, r.size.x, r.size.y);
                }
                RenderStep::SetStencilRef(v) => {
                    pass.set_stencil_reference(v);
                }
                RenderStep::MaskStamp(mi) => {
                    mark(pass, BatchKind::Mask);
                    debug_marker::push(pass, "mask_stamp");
                    rebind!(
                        Bound::MaskStamp,
                        self.quad
                            .bind_mask(pass, &fmt.quad.mask_stamp, &self.gradient.bg)
                    );
                    self.quad.draw_mask(pass, mi);
                    debug_marker::pop(pass);
                }
                RenderStep::MaskClear(mi) => {
                    mark(pass, BatchKind::Mask);
                    debug_marker::push(pass, "mask_clear");
                    rebind!(
                        Bound::MaskClear,
                        self.quad
                            .bind_mask(pass, &fmt.quad.mask_clear, &self.gradient.bg)
                    );
                    self.quad.draw_mask(pass, mi);
                    debug_marker::pop(pass);
                }
                RenderStep::Quads { range } => {
                    mark(pass, BatchKind::Quads);
                    debug_marker::push(pass, "quads");
                    rebind!(
                        Bound::QuadInstance,
                        self.quad
                            .bind(pass, &fmt.quad.color, use_stencil, &self.gradient.bg)
                    );
                    self.quad.draw(pass, range);
                    debug_marker::pop(pass);
                }
                RenderStep::Text { batch } => {
                    mark(pass, BatchKind::Text);
                    debug_marker::push(pass, "text");
                    // `render_batch` pushes both halves of the
                    // immediate region (viewport at offset 0, params
                    // at offset 8) itself. Subsequent non-text steps
                    // re-push viewport via `viewport.push_into(pass)`
                    // after their bind.
                    self.text
                        .pass
                        .render_batch(batch, pass, &fmt.text, use_stencil, &viewport);
                    bound = Bound::None;
                    debug_marker::pop(pass);
                }
                RenderStep::TierBatch { tier, batch } => {
                    // Timing bucket and debug label both come off the tier,
                    // so a new one cannot land in the pass untimed or
                    // unlabelled the way a forgotten `mark` call would.
                    let kind = batch_kind(tier);
                    mark(pass, kind);
                    debug_marker::push(pass, kind.label());
                    // Lazy: the icon tier draws off the batch index alone.
                    let items = || buffer.batches(tier)[batch].items;
                    match tier {
                        PaintTier::Mesh => {
                            rebind!(Bound::Mesh, self.mesh.bind(pass, &fmt.mesh, use_stencil));
                            self.mesh.draw(
                                pass,
                                MeshBatch {
                                    draws: buffer.meshes.draw(),
                                    items: items(),
                                },
                            );
                        }
                        PaintTier::Image => {
                            rebind!(Bound::Image, self.image.bind(pass, &fmt.image, use_stencil));
                            self.image.draw(
                                pass,
                                ImageBatch {
                                    ids: buffer.images.id(),
                                    items: items(),
                                },
                                &self.image_textures,
                            );
                        }
                        PaintTier::Icon => {
                            // Like text, `render_batch` pushes both halves of
                            // the immediate region itself, so the next step
                            // must re-push the viewport after its own bind.
                            self.icon.pass.render_batch(
                                batch,
                                pass,
                                &fmt.icon,
                                use_stencil,
                                &viewport,
                            );
                            bound = Bound::None;
                        }
                        PaintTier::Curve => {
                            rebind!(
                                Bound::Curve,
                                self.curve
                                    .bind(pass, &fmt.curve, use_stencil, &self.gradient.bg)
                            );
                            self.curve.draw(pass, items());
                        }
                    }
                    debug_marker::pop(pass);
                }
            },
        );
    }

    /// Draw the damage-rect debug overlay onto the swapchain texture
    /// *after* the backbuffer→surface copy. The overlay never lands on
    /// the backbuffer, so next frame's `LoadOp::Load` reads clean
    /// pixels and there's no ghost stroke. The outline quads were
    /// uploaded in `submit`'s belt phase
    /// (`DebugOverlay::upload_damage_rects`); `count` of them draw
    /// here. Same upload-early / draw-late split as the dim pass.
    fn run_overlay_pass(
        &self,
        fmt: &FormatPipelines,
        surface_view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        viewport: ViewportPush,
        count: u32,
    ) {
        let mut pass = begin_load_pass(
            encoder,
            "palantir.renderer.overlay.damage_rect",
            surface_view,
        );
        self.debug.draw_overlays(
            &mut pass,
            fmt.quad.color.select(false),
            &self.gradient.bg,
            &viewport,
            count,
        );
    }

    /// The device every window's per-window attachment is built against
    /// — the one thing a host needs off the shared backend to size its
    /// own [`Backbuffer`] and [`Stencil`].
    pub(crate) fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Skip path: the host's damage compute returned `None`, but the
    /// swapchain target still needs valid pixels (visual tests capture
    /// it unconditionally; the showcase short-circuits earlier, but
    /// other hosts may not). A `Skip` requires the previous frame to
    /// have been submitted at this size and format (`take_frame_plan`
    /// forces `Full` otherwise), so the backbuffer must already exist
    /// and match — copying anything else would present undefined or
    /// stale-format pixels, so crash instead of degrading.
    pub(crate) fn copy_backbuffer_to_surface(
        &self,
        backbuffer: &Backbuffer,
        surface_tex: &wgpu::Texture,
    ) {
        debug_assert!(
            backbuffer.describes(surface_tex.size(), surface_tex.format()),
            "skip-copy backbuffer doesn't match the target — a Skip frame \
             implies the previous frame painted this size/format"
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.skip"),
            });
        backbuffer.copy_onto(&mut encoder, surface_tex);
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Release every `GpuView` target owned by a render stream that has been
    /// retired — the host calls this as a window closes.
    ///
    /// Necessary because per-submit eviction is owner-scoped: a submit only
    /// frees its *own* absent targets, so that another window idling for a
    /// frame does not lose its views. A closed window never submits again, so
    /// without this its textures and bind groups would be held by every
    /// surviving window until the host shuts down.
    // `allow` rather than `cfg`: the winit host is only the current caller, not
    // the only conceivable one — an embedding host needs this entry point too.
    #[cfg_attr(not(feature = "winit"), allow(dead_code))]
    pub(crate) fn retire_render_owner(&mut self, owner: RenderOwnerId) {
        self.image_textures.retire_owner(owner);
    }
}

/// The timing bucket and debug label a [`PaintTier`] replay lands in.
///
/// Here rather than on either type: `BatchKind` lives in `diagnostics`,
/// which everything reports into and which depends on nothing, and
/// `PaintTier` lives in the render buffer, which has no business knowing
/// about instrumentation. The replay below is the one place both are
/// already in scope. Exhaustive, so a new tier cannot reach the pass
/// untimed and unlabelled the way a forgotten `mark` call could.
fn batch_kind(tier: PaintTier) -> BatchKind {
    match tier {
        PaintTier::Mesh => BatchKind::Mesh,
        PaintTier::Image => BatchKind::Image,
        PaintTier::Icon => BatchKind::Icon,
        PaintTier::Curve => BatchKind::Curve,
    }
}

/// Open a color-only `LoadOp::Load` render pass — the shape shared by
/// the dim pre-pass and the damage-overlay pass (no stencil, no
/// timestamps; only the label and target view differ). Both passes run
/// the debug overlay's quad draws standalone, outside the main pass.
fn begin_load_pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &'static str,
    view: &wgpu::TextureView,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    //! Reach-in introspection for the surface-format-change tests: the
    //! count of cached per-format pipeline sets and the GPU image-cache
    //! occupancy, used to assert a new format builds its own pipelines
    //! without dropping or re-uploading cached textures.

    use crate::renderer::backend::*;

    impl WgpuBackend {
        /// Whether a pipeline set has been built for `format`.
        pub(crate) fn has_format_pipelines(&self, format: wgpu::TextureFormat) -> bool {
            self.pipelines.contains_key(&format)
        }

        /// Images resident in the GPU texture cache — see
        /// [`ImageTextures::gpu_cached_count`].
        pub(crate) fn gpu_image_cache_len(&self) -> usize {
            self.image_textures.gpu_cached_count()
        }
    }
}

#[cfg(test)]
mod tests;
