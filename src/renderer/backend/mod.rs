mod curve_pipeline;
mod debug_overlay;
mod dynamic_buffer;
pub(crate) mod gpu_ctx;
pub(crate) mod gpu_pass_stats;
mod gpu_timings;
mod gradient_resources;
pub(crate) mod image_pipeline;
mod mesh_pipeline;
mod pipeline_utils;
mod quad_pipeline;
pub(crate) mod queue;
mod schedule;
mod stencil;
pub mod text;
pub(crate) mod viewport;
#[cfg(feature = "internals")]
pub(crate) mod write_stats;

use self::curve_pipeline::CurvePipeline;
use self::debug_overlay::{
    DAMAGE_OVERLAY_COLOR, DAMAGE_OVERLAY_GAP, DAMAGE_OVERLAY_STROKE_WIDTH, DebugOverlay,
};
use self::gpu_ctx::GpuCtx;
use self::gpu_pass_stats::{BatchKind, GpuPassStats};
use self::gpu_timings::GpuTimings;
use self::gradient_resources::GradientResources;
use self::image_pipeline::ImagePipeline;
use self::mesh_pipeline::MeshPipeline;
use self::quad_pipeline::QuadPipeline;
use self::queue::Queue;
use self::schedule::{RenderStep, for_each_step};
use self::stencil::STENCIL_FORMAT;
use self::viewport::{ViewportPush, build_damage_scissors};
use crate::debug_overlay::DebugOverlayConfig;
use crate::forest::frame_arena::FrameArena;
use crate::primitives::{rect::Rect, size::Size, spacing::Spacing, urect::URect};
use crate::renderer::backend::text::{StencilMode as TextStencilMode, TextBackend};
use crate::renderer::caches::RenderCaches;
use crate::renderer::render_buffer::RenderBuffer;
use crate::text::TextShaper;
use crate::ui::damage::region::DAMAGE_RECT_CAP;
use crate::ui::frame_report::RenderPlan;

/// Size of the per-pipeline immediate (push-constant) region every
/// palantir shader's `var<immediate> imm: Immediates` reads. Locked
/// at the maximum used by any pipeline so a `set_immediates` for one
/// shader stays valid across pipeline switches:
///
/// - offset 0 (8 bytes): [`ViewportPush`] — viewport size, written
///   once per pass by `WgpuBackend`.
/// - offset 8 (8 bytes): `text::Params` — atlas dimensions,
///   written per text batch by `TextBackend::render_batch`.
///
/// Pipelines that don't use the tail (quad/mesh/image/curve) still
/// declare `immediate_size = IMMEDIATES_BYTES` so the immediate-state
/// layout matches and bytes written by other pipelines stay valid
/// after a pipeline switch.
pub(crate) const IMMEDIATES_BYTES: u32 = 16;

/// Construction-time knobs for [`WgpuBackend::new`]. Grouped so the
/// `WindowRenderer` / `WinitHost` call sites don't grow a long positional
/// signature each time a new GPU-side setting is exposed.
pub(crate) struct WgpuBackendConfig {
    /// `Some(stats)` opts the backend into GPU instrumentation: the
    /// backend writes resolved samples through the shared handle and
    /// pays the per-frame `resolve_query_set` + `map_async` +
    /// `device.poll(Poll)` + readback cost. `None` skips the whole
    /// path — `GpuTimings` is never constructed. Adapter features
    /// (`TIMESTAMP_QUERY`, `+TIMESTAMP_QUERY_INSIDE_PASSES`,
    /// `+PIPELINE_STATISTICS_QUERY`) still gate what actually gets
    /// collected; missing features degrade individually.
    pub(crate) pass_stats: Option<GpuPassStats>,
}

/// Persistent off-screen target that the render pass paints into.
/// We render to this texture (not to the swapchain view directly)
/// so we can keep last frame's pixels around between frames —
/// `LoadOp::Load` only works reliably on a texture *we* own; the
/// swapchain's preserve-contents behaviour varies by platform/
/// present-mode. After the pass, [`WgpuBackend::submit`] copies
/// the backbuffer into the swapchain texture and presents.
///
/// Sized to match the surface texture; recreated on resize or
/// format change.
struct Backbuffer {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    /// Cached at creation: lets `ensure_backbuffer` skip the
    /// `wgpu::Texture::size()` round-trip on every frame. The Arc
    /// traversal that call walks is ~15 µs/frame at this bench
    /// shape — small but visible in Tracy at 14% of trace time.
    size: wgpu::Extent3d,
    /// Lazy stencil attachment, allocated on first frame with rounded
    /// clipping (`FrameOutput::has_rounded_clip == true`). Apps that
    /// never use rounded clip never allocate this. Recreated alongside
    /// the color texture on resize / format change.
    stencil: Option<StencilAttachment>,
}

struct StencilAttachment {
    #[allow(dead_code)] // owns the GPU resource that `view` points into
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

/// wgpu backend: owns the quad pipeline + text renderer and cloned
/// device/queue handles (cheap, Arc-backed). The text side holds a
/// shared handle to the same `CosmicMeasure` the Ui side measures
/// against (passed in at [`Self::new`]) so layout-time measurement
/// and rasterization hit one buffer cache. No layout, no encode, no
/// compose — those happen elsewhere and arrive here as a
/// `RenderBuffer`.
pub(crate) struct WgpuBackend {
    device: wgpu::Device,
    queue: Queue,
    /// All per-frame dynamic-buffer uploads route through this belt so
    /// the resulting `copy_buffer_to_buffer` commands ride the main
    /// encoder. On Metal that collapses N `queue.write_buffer` calls
    /// (each spinning up a fresh `MTLBlitCommandEncoder`) down to one
    /// blit encoder per submit. Chunk size sized to comfortably hold a
    /// resizing-frame's worth of buffer uploads (~512 KB observed in
    /// the frame bench).
    staging_belt: wgpu::util::StagingBelt,
    /// Latest viewport size (physical px) consumed by every pipeline
    /// via the shared immediate region. Refreshed at the top of
    /// `submit` from the current `RenderBuffer`; pushed by each pass
    /// open via `pass.set_immediates(0, ..)`. There's no GPU buffer
    /// or bind group anymore — the value rides command-buffer record
    /// state directly.
    viewport_size: glam::Vec2,
    /// Shared gradient LUT atlas resources (texture + sampler + group-0
    /// bind group), lent to the quad and curve pipelines — both render
    /// gradient brushes off this one allocation.
    gradient: GradientResources,
    quad: QuadPipeline,
    mesh: MeshPipeline,
    image: ImagePipeline,
    curve: CurvePipeline,
    text: TextBackend,
    debug: DebugOverlay,
    /// Color format every pipeline (quad / mesh / image / curve / text
    /// atlas) was built for. Set at [`Self::new`] and updated by
    /// [`Self::recreate_for_format`], which rebuilds all of them
    /// together. [`Self::ensure_backbuffer`] hard-asserts the swapchain
    /// texture handed to `submit` matches this — a mismatch means the
    /// host changed format without going through
    /// [`WindowRenderer::set_surface_format`](crate::WindowRenderer::set_surface_format),
    /// which would leave the pipelines stale and silently mis-render.
    color_format: wgpu::TextureFormat,
    /// Persistent off-screen render target; lazily created on first
    /// submit and recreated when the surface size or format changes.
    /// Stage 3 / Step 6 of the damage-rendering plan: we render here
    /// so future frames can `LoadOp::Load` last frame's pixels.
    backbuffer: Option<Backbuffer>,
    /// Shared frame arena (clone of `WindowRenderer`'s canonical handle). The
    /// backend reads mesh vertices/indices from it during upload.
    frame_arena: FrameArena,
    /// Shared cross-frame GPU resource caches (image registry +
    /// gradient atlas). Drained / flushed each frame to push newly
    /// registered images and dirty gradient rows to GPU.
    caches: RenderCaches,
    /// Main-pass timestamp queries. `Some` when the host opted into
    /// instrumentation (see `WinitHostConfig::collect_gpu_stats`) AND
    /// the adapter advertises `TIMESTAMP_QUERY`. Resolved values
    /// publish through the `GpuPassStats` handle the backend got at
    /// construction; `WindowRenderer` keeps the canonical clone.
    gpu_timings: Option<GpuTimings>,
}

impl WgpuBackend {
    /// Reconfigure a `wgpu::Surface` against this backend's device.
    /// Encapsulates the device handle so callers don't need to read
    /// it directly — used by the host on suboptimal/outdated/lost
    /// surface acquisitions.
    pub(crate) fn configure_surface(
        &self,
        surface: &wgpu::Surface,
        config: &wgpu::SurfaceConfiguration,
    ) {
        surface.configure(&self.device, config);
    }

    pub(crate) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
        frame_arena: FrameArena,
        caches: RenderCaches,
        config: WgpuBackendConfig,
    ) -> Self {
        let WgpuBackendConfig { pass_stats } = config;
        // GPU pass timing collection is opt-in via `pass_stats`:
        // `Some(handle)` → wire up `GpuTimings` and write samples
        // through the handle; `None` → skip the whole readback path.
        // Adapter features then degrade what gets collected:
        // `TIMESTAMP_QUERY` is required at all; `+
        // TIMESTAMP_QUERY_INSIDE_PASSES` enables per-batch
        // attribution; `+PIPELINE_STATISTICS_QUERY` adds VS/FS
        // invocation counts. The non-zero `period` check guards
        // against headless / software queues that advertise the
        // feature but can't actually time submissions.
        let features = device.features();
        let gpu_timings = pass_stats.and_then(|sink| {
            (features.contains(wgpu::Features::TIMESTAMP_QUERY)
                && queue.get_timestamp_period() > 0.0)
                .then(|| {
                    GpuTimings::new(
                        &device,
                        queue.get_timestamp_period(),
                        features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
                        features.contains(wgpu::Features::PIPELINE_STATISTICS_QUERY),
                        sink,
                    )
                })
        });
        // Viewport now rides immediates (push constants) — no bind
        // group, no buffer. Pipelines all declare the same
        // `IMMEDIATES_BYTES` region so the immediate state stays
        // valid across pipeline switches; the backend pushes it once
        // per pass open.
        // Gradient LUT atlas resources, shared by the quad and curve
        // pipelines (both sample gradient brushes). Owned here so
        // neither pipeline owns the other's input — each composes its
        // layout against `gradient.bgl` and binds `gradient.bg`.
        let gradient = GradientResources::new(&device);
        let quad = QuadPipeline::new(&device, &gradient.bgl, format);
        let mesh = MeshPipeline::new(&device, format);
        let image = ImagePipeline::new(&device, format);
        let curve = CurvePipeline::new(&device, format, &gradient.bgl);
        let text = TextBackend::new(&device, format, &Self::text_stencil_states(), shaper);
        let debug = DebugOverlay::new(&device);
        // 1 MiB chunks: comfortably above the resizing-arm's ~500 KB
        // per-frame upload peak, so we land in 1-2 chunks during
        // steady state. wgpu allocates a new chunk only when the
        // active one can't fit a write.
        let staging_belt = wgpu::util::StagingBelt::new(device.clone(), 1 << 20);
        Self {
            device,
            queue: Queue::new(queue),
            staging_belt,
            viewport_size: glam::Vec2::ZERO,
            gradient,
            quad,
            mesh,
            image,
            curve,
            text,
            debug,
            color_format: format,
            backbuffer: None,
            frame_arena,
            caches,
            gpu_timings,
        }
    }

    /// Per-pipeline stencil configs the production text pipelines are
    /// built with. Index 0 = Plain, index 1 = Stencil — matches
    /// `text::StencilMode::pipeline_idx`. Single source of truth shared
    /// by [`Self::new`] and [`Self::recreate_for_format`] so the rebuilt
    /// text pipelines can't drift from the originals.
    fn text_stencil_states() -> [Option<wgpu::DepthStencilState>; 2] {
        [None, Some(stencil::stencil_test_state())]
    }

    /// Rebuild every format-dependent pipeline (quad / mesh / image /
    /// curve / text) against `format`. No-op when `format` already
    /// matches. Surgical: only the `wgpu::RenderPipeline` objects carry
    /// the color-target format, so each pipeline swaps just those and
    /// keeps its format-independent resources — uploaded image textures
    /// with their bind groups, the gradient LUT atlas, the glyph atlas
    /// (every rasterized glyph), samplers, and instance/index buffers
    /// all survive. **No image re-upload or glyph re-rasterization.** Lazy
    /// stencil variants are dropped and rebuild on the next rounded-clip
    /// frame. Drops the backbuffer so the next submit full-clears at the
    /// new format (the old texture carries the old format). Counterpart
    /// to the hard-assert in [`Self::ensure_backbuffer`] — the host
    /// calls this via
    /// [`WindowRenderer::set_surface_format`](crate::WindowRenderer::set_surface_format)
    /// when it observes a surface format change.
    pub(crate) fn recreate_for_format(&mut self, format: wgpu::TextureFormat) {
        if self.color_format == format {
            return;
        }
        let device = &self.device;
        // Gradient resources are format-independent — only the pipelines
        // carry the color target. Re-thread the shared `bgl` so quad and
        // curve rebuild against the same group-0 layout.
        self.quad
            .rebuild_for_format(device, &self.gradient.bgl, format);
        self.mesh.rebuild_for_format(device, format);
        self.image.rebuild_for_format(device, format);
        self.curve
            .rebuild_for_format(device, &self.gradient.bgl, format);
        self.text
            .rebuild_for_format(device, format, &Self::text_stencil_states());
        self.color_format = format;
        // Old backbuffer carries the previous format; force a fresh
        // allocation + full clear on the next submit.
        self.backbuffer = None;
    }

    /// Lazily (re)create the backbuffer to match the surface texture's
    /// size. Returns `true` if the backbuffer was just (re)created —
    /// caller treats that as a forced full repaint (the new texture's
    /// contents are undefined until the first pass writes to it).
    /// Hard-asserts that the swapchain format hasn't changed since
    /// construction; see [`Self::color_format`].
    #[profiling::function]
    fn ensure_backbuffer(&mut self, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> bool {
        assert_eq!(
            self.color_format, format,
            "WgpuBackend was built for surface format {:?}; got {:?} this submit. \
             Every format-dependent pipeline (quad / mesh / image / curve / text \
             atlas) was built against the original format. Call \
             `WindowRenderer::set_surface_format` when the surface format changes \
             mid-session — it rebuilds them all. Reaching here means the \
             swapchain format changed without that call.",
            self.color_format, format,
        );
        let needs_new = match &self.backbuffer {
            None => true,
            Some(b) => b.size != size,
        };
        if !needs_new {
            return false;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.backbuffer"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.backbuffer = Some(Backbuffer {
            tex,
            view,
            size,
            // Drop any stale stencil — it was sized to the old
            // backbuffer; `ensure_stencil` lazily allocates a fresh
            // one matching the new size on the next rounded-clip
            // frame. Without this, wgpu validation rejects the pass
            // (mismatched attachment sizes).
            stencil: None,
        });
        true
    }

    /// Allocate the stencil attachment if it isn't already present.
    /// `ensure_backbuffer` resets `stencil` to `None` whenever it
    /// rebuilds the color texture, so a `Some` here is always
    /// size-matched to the current backbuffer.
    #[profiling::function]
    fn ensure_stencil(&mut self) {
        let bb = self
            .backbuffer
            .as_mut()
            .expect("ensure_backbuffer must run first");
        if bb.stencil.is_some() {
            return;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.stencil"),
            size: bb.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        bb.stencil = Some(StencilAttachment { tex, view });
    }

    /// Render one frame to the persistent backbuffer, then copy the
    /// backbuffer onto the swapchain texture. The caller's surface
    /// texture must have `COPY_DST` usage (set in
    /// [`wgpu::SurfaceConfiguration::usage`]).
    ///
    /// Without a shared shaper installed (mono fallback) text rendering
    /// is silently skipped; the frame still draws quads.
    ///
    /// Quads and text interleave per-group in paint order: each group's
    /// quads draw first, then its text renders on top, before the next
    /// group runs. So a child quad declared *after* a label correctly
    /// occludes that label.
    ///
    /// Two damage paths, branching on `damage`:
    ///
    /// - [`Damage::Full`]: a single `LoadOp::Clear(clear)` pass
    ///   paints every group at its native scissor. First frame,
    ///   post-resize, post-format-change, and coverage-above-threshold
    ///   all land here.
    /// - [`Damage::Partial(region)`][Damage::Partial]: one
    ///   render pass per rect in the region. Each pass `LoadOp::Load`s
    ///   the backbuffer (preserving last frame outside the rect) and
    ///   the schedule narrows every group's scissor to that pass's
    ///   damage rect. Logical-px in; the backend scales, pads for AA
    ///   bleed, and clamps to surface; rects that clamp to zero area
    ///   are filtered out.
    ///
    /// Skip frames are handled in `WindowRenderer::render`'s early-return branch
    /// (`pending_damage` is `None`); this method is only entered with
    /// `Some(Full | Partial)`.
    ///
    /// A region whose every rect clamps to zero physical-px area
    /// degrades to a single `Full` pass — correct, just wasteful.
    #[profiling::function]
    pub(crate) fn submit(
        &mut self,
        surface_tex: &wgpu::Texture,
        buffer: &RenderBuffer,
        plan: RenderPlan,
        debug_overlay: DebugOverlayConfig,
    ) {
        let clear = match plan {
            RenderPlan::Full { clear } | RenderPlan::Partial { clear, .. } => clear,
        };
        let arena = self.frame_arena.clone();
        let arena = arena.inner();

        let use_stencil = buffer.has_rounded_clip;
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.viewport_phys,
            requested_plan = ?plan,
            rounded_clip = use_stencil,
            "wgpu_backend.submit"
        );

        // Match backbuffer to the swapchain texture. A freshly
        // (re)created backbuffer has undefined contents, so any
        // requested Partial must escalate to a full clear+paint this
        // frame. `effective_damage` is what we'll actually render;
        // `damage` is what the host asked for. The two diverge only on
        // backbuffer recreate, but the debug overlay's damage-rect
        // outline shows what we *rendered*, not what was requested, so
        // threading the renamed value through is the right semantic.
        let backbuffer_recreated = self.ensure_backbuffer(surface_tex.size(), surface_tex.format());
        // `effective_plan` is what we'll actually render; `plan` is
        // what the host asked for. The two diverge only on backbuffer
        // recreate, but the debug overlay's damage-rect outline shows
        // what we *rendered*, not what was requested, so threading
        // the renamed value through is the right semantic.
        let effective_plan = if backbuffer_recreated {
            RenderPlan::Full { clear }
        } else {
            plan
        };

        // Build the per-frame scissor list. `Full` → empty list →
        // single Clear+full-viewport pass. `Partial` → one entry per
        // rect in the region (see `build_damage_scissors`).
        let mut damage_scissors: tinyvec::ArrayVec<[URect; DAMAGE_RECT_CAP]> = Default::default();
        build_damage_scissors(&mut damage_scissors, effective_plan, buffer);
        // `dim_undamaged` debug mode: every Partial frame, before any
        // damage passes, draw one full-viewport 40%-translucent black
        // quad onto the backbuffer with `LoadOp::Load`. Each frame
        // undamaged pixels are dimmed once; damaged pixels are dimmed
        // then immediately overwritten by the fresh repaint, so they
        // stay bright. Across many frames the static background fades
        // toward black while moving content stays current — far less
        // jarring than the prior `LoadOp::Clear` flash and visually
        // pins which regions are actually repainting.
        let dim_undamaged = debug_overlay.dim_undamaged && !damage_scissors.is_empty();

        // Stencil path activates whenever the encoded frame contains a
        // `PushClip` with a non-zero radius. Lazy-init the stencil
        // texture + pipeline variants the first time we land here;
        // thereafter both stay warm. Apps that never round-clip never
        // enter this branch. The mask upload happens further down,
        // after the encoder is open, alongside every other dynamic
        // buffer upload.
        let text_mode = if use_stencil {
            TextStencilMode::Stencil
        } else {
            TextStencilMode::Plain
        };
        if use_stencil {
            self.ensure_stencil();
            self.quad.ensure_stencil(&self.device, &self.gradient.bgl);
            self.mesh.ensure_stencil(&self.device);
            self.image.ensure_stencil(&self.device);
            self.curve.ensure_stencil(&self.device, &self.gradient.bgl);
        }

        // Open the main encoder up front: every dynamic-buffer upload
        // below routes through `staging_belt`, which schedules its
        // `copy_buffer_to_buffer` commands on this encoder rather
        // than allocating its own MTLBlitCommandEncoder per
        // `queue.write_buffer`. Render passes are recorded on this
        // same encoder later in the function; wgpu serialises
        // commands in record order so the copies land before the
        // passes that read from the destination buffers.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.main"),
            });

        // Image-registry texture uploads: rare, hit `queue.write_texture`
        // directly (StagingBelt's buffer-only path doesn't help here).
        // Belt-routed upload phase. Scoped so the borrows release
        // before the render-pass phase needs `&mut encoder` cleanly.
        {
            let mut ctx = GpuCtx::new(
                &self.device,
                &self.queue,
                &mut self.staging_belt,
                &mut encoder,
            );

            // Texture-only uploads (the belt is buffer-only). Run
            // first so any draws below see the right pixels:
            // - gradient LUT atlas: idle frames drain an empty dirty
            //   flag and do nothing; first frame uploads row 0's
            //   magenta fallback plus any baked rows composer queued.
            // - image registry: first-frame images need a bind group
            //   ready when the schedule's draw call lands.
            self.gradient.upload(&ctx, &self.caches.gradients);
            self.image.drain_registry(&mut ctx, &self.caches.images);

            if dim_undamaged {
                self.debug.upload_dim(&mut ctx, buffer.viewport_phys_f, 0.4);
            }
            if use_stencil {
                // After staging, `self.quad.mask_indices` parallels
                // `buffer.groups` and `render_groups` reads it directly.
                self.quad.stage_masks(&mut ctx, &buffer.groups);
            }

            // Cache the size for `set_immediates` at each pass open
            // — no buffer write, no bind group, no dirty tracking.
            self.viewport_size = buffer.viewport_phys_f;
            self.quad.upload(&mut ctx, &buffer.quads);
            self.mesh.upload(
                &mut ctx,
                &arena.meshes.vertices,
                &arena.meshes.indices,
                buffer.meshes.rows.instance(),
            );
            self.image
                .upload_instances(&mut ctx, buffer.images.rows.instance());
            self.curve.upload(&mut ctx, &buffer.curves);

            if !damage_scissors.is_empty() {
                self.quad
                    .upload_clear(&mut ctx, buffer.viewport_phys_f, clear);
            }

            // Text prepare: per-batch glyph encoding. Routes its
            // vertex/params/atlas-staging writes through the same
            // ctx so every text-backend write lands as
            // `copy_buffer_to_buffer` on the main encoder. Viewport
            // size is read by the text shader from the shared
            // `@group(0)` uniform — no per-frame push from here.
            {
                profiling::scope!(
                    "text.prepare_batches",
                    &format!("count={}", buffer.text_batches.len())
                );
                for (i, b) in buffer.text_batches.iter().enumerate() {
                    let runs = &buffer.texts[b.texts.range()];
                    self.text.prepare_batch(&mut ctx, buffer.scale, i, runs);
                }
                // (TextBackend flushes its params buffer inside
                // prepare_batch whenever resolution or atlas sizes
                // change — no second sync needed.)
            }

            // Drain glyph atlas uploads (atlas-grow blits + per-glyph
            // copy_buffer_to_texture) into the same encoder so they
            // share the main render submit. The staging side of those
            // copies also routes through the belt — see
            // `atlas::flush_pending_uploads`.
            self.text.flush_atlas_uploads(&mut ctx);
        }
        // (Belt stays open across the scope boundary —
        // `draw_debug_overlay` reconstructs a short-lived ctx for its
        // damage-rect outline upload. `belt.finish()` lives right
        // before `queue.submit` below.)

        // Two paths, branching on whether the frame is a Full or
        // Partial repaint. Both go through one `begin_render_pass`:
        // - `damage_scissors` empty ⇒ Full: one schedule walk with no
        //   scissor, `LoadOp::Clear(color)` covers the backbuffer.
        // - `damage_scissors` non-empty ⇒ Partial: an optional dim
        //   pre-pass (`dim_undamaged`) that paints a 40% black quad
        //   over the full backbuffer with `LoadOp::Load` in its own
        //   render pass (no-stencil pipeline incompatible with the
        //   main pass's stencil attachment on rounded-clip frames),
        //   followed by one main pass with `LoadOp::Load` and one
        //   schedule walk per damage rect inside it. Rects are
        //   pairwise disjoint, so the per-pass stencil clear is
        //   sufficient — no per-rect stencil reset needed.
        // Force alpha to 1: the surface clear is the bottom-most
        // paint layer of the frame, so any sub-1 alpha would let the
        // host's desktop show through the framebuffer's transparent
        // regions. Palantir doesn't support transparent windows
        // (and the occlusion-prune assumes the clear is opaque).
        let clear_color = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: 1.0,
        };
        if damage_scissors.is_empty() {
            tracing::trace!("wgpu_backend.submit.pass.full");
            self.run_main_pass(
                &mut encoder,
                buffer,
                None,
                clear_color,
                use_stencil,
                text_mode,
            );
        } else {
            if dim_undamaged {
                tracing::trace!("wgpu_backend.submit.pass.dim");
                self.run_dim_pass(&mut encoder);
            }
            tracing::trace!(
                rects = damage_scissors.len(),
                "wgpu_backend.submit.pass.partial"
            );
            self.run_main_pass(
                &mut encoder,
                buffer,
                Some(damage_scissors.as_slice()),
                clear_color,
                use_stencil,
                text_mode,
            );
        }

        self.copy_backbuffer_into(&mut encoder, surface_tex);

        self.draw_debug_overlay(
            surface_tex,
            &mut encoder,
            buffer,
            effective_plan,
            debug_overlay,
        );

        // Last step before encoder.finish(): resolve the main-pass
        // timestamps if timing is on. The main pass closed before
        // copy_backbuffer_into; the resolve can ride in the same
        // command buffer as everything else.
        if let Some(t) = self.gpu_timings.as_mut() {
            t.resolve(&mut encoder);
        }

        // Close the belt: no more `belt.write_buffer` calls allowed
        // until after `submit + recall` below.
        self.staging_belt.finish();
        self.queue.submit(std::iter::once(encoder.finish()));

        // Return the just-used staging-belt chunks for remap. Closed
        // chunks come back when their `map_async` callback fires off
        // the next `device.poll` — `PollType::Wait` callers see them
        // ready next frame; `PollType::Poll` callers may need to
        // allocate one more chunk during the catch-up window. wgpu's
        // own docs flag this as harmless.
        self.staging_belt.recall();

        // Kick the map_async on this frame's staging slot and read
        // back any prior frame whose map has completed. Cheap (one
        // device.poll(Poll), one memcpy on the ready slot).
        if let Some(t) = self.gpu_timings.as_mut() {
            t.after_submit(&self.device);
        }

        if self.text.prepared_anything {
            self.text.post_record();
        }
    }

    /// Full-viewport pass that draws one 40%-translucent black quad
    /// over the backbuffer with `LoadOp::Load`. Runs before partial
    /// damage passes when the debug `dim_undamaged` flag is on (see
    /// `dim_undamaged` in [`Self::submit`]). No stencil attachment
    /// even when the frame uses rounded clipping — the dim quad
    /// paints uniformly and subsequent partial passes set their own.
    fn run_dim_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let backbuffer = self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.renderer.dim.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backbuffer.view,
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
        });
        // Dim pass uses the quad pipeline outside the main pass.
        // `draw_dim` binds the pipeline first; immediately after, push
        // the shared viewport immediate.
        let viewport = ViewportPush {
            size: self.viewport_size,
        };
        self.debug
            .draw_dim(&mut pass, &self.quad, &self.gradient.bg, &viewport);
    }

    /// Open the main render pass against the backbuffer and walk the
    /// schedule once per damage rect (or once with no scissor on Full).
    /// All rects share one pass: one `begin_render_pass`, one stencil
    /// `LoadOp::Clear(0)`, one color load. Per-rect work is just a
    /// `SetScissor` + the schedule's group walk (plus the schedule's
    /// own per-rect `PreClear` quad on Partial).
    ///
    /// Rects are pairwise disjoint (the damage merger always merges
    /// intersecting pairs — see `ui/damage/region/mod.rs`), so per-rect
    /// stencil writes from one rect's groups can't bleed into another
    /// rect's reads. Each `render_groups` call starts with a fresh
    /// `active_mask = None`; that matches the stencil contents inside
    /// the rect's scissor (always 0 there at pass open, never written
    /// outside another rect's scissor).
    ///
    /// `partial_scissors == None` ⇒ Full frame: one schedule walk with
    /// no damage scissor, `LoadOp::Clear(color)` covers the whole
    /// backbuffer. `Some(rects)` ⇒ Partial: `LoadOp::Load`, one walk
    /// per rect inside the same pass.
    #[profiling::function]
    fn run_main_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &RenderBuffer,
        partial_scissors: Option<&[URect]>,
        clear: wgpu::Color,
        use_stencil: bool,
        text_mode: TextStencilMode,
    ) {
        let backbuffer = self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded");
        let stencil_view = if use_stencil {
            Some(
                &backbuffer
                    .stencil
                    .as_ref()
                    .expect("ensure_stencil populated this")
                    .view,
            )
        } else {
            None
        };
        let depth_stencil_attachment =
            stencil_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    // One stencil clear per *pass*, not per rect — the
                    // rect-disjointness invariant means rect B's
                    // scissor reads a region that rect A's masks never
                    // touched, so the cleared-once-per-pass stencil is
                    // sufficient.
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            });
        let load_op = match partial_scissors {
            None => wgpu::LoadOp::Clear(clear),
            Some(_) => wgpu::LoadOp::Load,
        };
        // Timestamp writes via the descriptor cover the basic mode
        // (TIMESTAMP_QUERY only — pass begin / end). In per-batch
        // mode (TIMESTAMP_QUERY_INSIDE_PASSES additionally on) we
        // skip the descriptor and write begin/end inline via
        // `pass_begin` / `pass_end` so a single sequential timestamp
        // stream covers begin → midpoints → end without index gaps.
        let timestamp_writes = self.gpu_timings.as_ref().and_then(|t| t.pass_writes());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.renderer.main.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backbuffer.view,
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
            if t.inside_passes() {
                t.pass_begin(&mut pass);
            }
            t.begin_pipeline_stats(&mut pass);
        }
        match partial_scissors {
            None => self.render_groups(&mut pass, buffer, None, use_stencil, text_mode),
            Some(rects) => {
                for (i, &r) in rects.iter().enumerate() {
                    tracing::trace!(
                        rect = i,
                        of = rects.len(),
                        scissor = ?r,
                        "wgpu_backend.submit.pass.partial_rect"
                    );
                    self.render_groups(&mut pass, buffer, Some(r), use_stencil, text_mode);
                }
            }
        }
        if let Some(t) = &self.gpu_timings {
            t.end_pipeline_stats(&mut pass);
            if t.inside_passes() {
                t.pass_end(&mut pass);
            }
        }
    }

    /// Dispatch every step in the per-frame schedule
    /// ([`schedule::for_each_step`]) to the wgpu render pass. Logic
    /// for *what* runs in *what order* lives in the schedule module;
    /// this method is purely the wgpu translation layer for each
    /// `RenderStep`. Tests reuse the same schedule emitter to assert
    /// on the sequence without GPU.
    #[profiling::function]
    fn render_groups<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        buffer: &RenderBuffer,
        damage_scissor: Option<URect>,
        use_stencil: bool,
        text_mode: TextStencilMode,
    ) {
        // Track what pipeline + vertex buffer is currently bound so we
        // can skip redundant `set_pipeline` / `set_vertex_buffer` calls
        // across consecutive same-kind steps. wgpu records every
        // `set_pipeline` as a real command — drivers don't dedupe.
        // `PreClear` and glyphon's `render_group` set their own state,
        // so we reset to `None` after them and re-bind on the next
        // non-text step.
        #[derive(PartialEq, Eq)]
        enum Bound {
            None,
            QuadInstance,
            Mesh,
            Image,
            Curve,
            MaskWrite,
        }
        let mut bound = Bound::None;
        let viewport = ViewportPush {
            size: self.viewport_size,
        };

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
            |step| match step {
                RenderStep::PreClear => {
                    mark(pass, BatchKind::PreClear);
                    pass.push_debug_group("preclear");
                    // bind → push viewport → draw. Pushing after the
                    // draw (or skipping it) leaves the clear quad
                    // reading whatever's in the immediate region —
                    // zero on the first PreClear of a partial pass,
                    // which lands the quad at garbage NDC and skips
                    // the damage-region clear.
                    self.quad.bind_clear(pass, use_stencil, &self.gradient.bg);
                    viewport.push_into(pass);
                    pass.draw(0..4, 0..1);
                    // Distinct vertex buffer (clear_buffer); next
                    // non-clear step re-binds.
                    bound = Bound::None;
                    pass.pop_debug_group();
                }
                RenderStep::SetScissor(r) => {
                    pass.set_scissor_rect(r.x, r.y, r.w, r.h);
                }
                RenderStep::SetStencilRef(v) => {
                    pass.set_stencil_reference(v);
                }
                RenderStep::MaskQuad(mi) => {
                    mark(pass, BatchKind::Mask);
                    pass.push_debug_group("mask");
                    rebind!(
                        Bound::MaskWrite,
                        self.quad.bind_mask_write(pass, &self.gradient.bg)
                    );
                    self.quad.draw_mask(pass, mi);
                    pass.pop_debug_group();
                }
                RenderStep::Quads { range, .. } => {
                    mark(pass, BatchKind::Quads);
                    pass.push_debug_group("quads");
                    rebind!(
                        Bound::QuadInstance,
                        self.quad.bind(pass, use_stencil, &self.gradient.bg)
                    );
                    self.quad.draw_range(pass, range);
                    pass.pop_debug_group();
                }
                RenderStep::Text { batch } => {
                    mark(pass, BatchKind::Text);
                    pass.push_debug_group("text");
                    // `render_batch` pushes both halves of the
                    // immediate region (viewport at offset 0, params
                    // at offset 8) itself. Subsequent non-text steps
                    // re-push viewport via `viewport.push_into(pass)`
                    // after their bind.
                    self.text.render_batch(batch, pass, text_mode, &viewport);
                    bound = Bound::None;
                    pass.pop_debug_group();
                }
                RenderStep::MeshBatch { batch } => {
                    mark(pass, BatchKind::Mesh);
                    pass.push_debug_group("meshes");
                    rebind!(Bound::Mesh, self.mesh.bind(pass, use_stencil));
                    let range = buffer.mesh_batches[batch].meshes;
                    let start = range.start as usize;
                    let end = start + range.len as usize;
                    for (offset, draw) in buffer.meshes.rows.draw()[start..end].iter().enumerate() {
                        // `draw_indexed` takes a per-call vertex
                        // offset; pass the mesh's vertex start as
                        // `base_vertex` so indices stay buffer-local.
                        // Instance index is the draw's absolute slot in
                        // `meshes.instances`.
                        self.mesh.draw(
                            pass,
                            draw.indices.into(),
                            draw.vertices.start as i32,
                            (start + offset) as u32,
                        );
                    }
                    pass.pop_debug_group();
                }
                RenderStep::ImageBatch { batch } => {
                    mark(pass, BatchKind::Image);
                    pass.push_debug_group("images");
                    rebind!(Bound::Image, self.image.bind(pass, use_stencil));
                    let range = buffer.image_batches[batch].images;
                    let start = range.start as usize;
                    let end = start + range.len as usize;
                    for (offset, id) in buffer.images.rows.id()[start..end].iter().enumerate() {
                        self.image.draw(pass, *id, (start + offset) as u32);
                    }
                    pass.pop_debug_group();
                }
                RenderStep::CurveBatch { batch } => {
                    mark(pass, BatchKind::Curve);
                    pass.push_debug_group("curves");
                    rebind!(
                        Bound::Curve,
                        self.curve.bind(pass, use_stencil, &self.gradient.bg)
                    );
                    let range = buffer.curve_batches[batch].instances;
                    self.curve.draw(pass, range.start..range.start + range.len);
                    pass.pop_debug_group();
                }
            },
        );
    }

    /// Draw the debug overlay onto the swapchain texture *after* the
    /// backbuffer→surface copy. The overlay never lands on the
    /// backbuffer, so next frame's `LoadOp::Load` reads clean pixels
    /// and there's no ghost stroke. Each `bool` on `config` toggles a
    /// distinct visualization; the function is a no-op when all flags
    /// are off. Caller already filtered `Damage::Skip`.
    fn draw_debug_overlay(
        &mut self,
        surface_tex: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &RenderBuffer,
        plan: RenderPlan,
        config: DebugOverlayConfig,
    ) {
        if config.damage_rect {
            // One stroked outline per damage rect — `Partial`
            // contributes the whole region; `Full` contributes a
            // single full-viewport outline. All quads ride one
            // instanced draw inside one pass so a single
            // `queue.write_buffer` covers them (per-iteration writes
            // to the same buffer would all collapse to the last
            // value at submit time).
            let gap_px = (DAMAGE_OVERLAY_GAP * buffer.scale).max(1.0);
            let stroke_width = DAMAGE_OVERLAY_STROKE_WIDTH * buffer.scale;
            let mut overlay_rects: tinyvec::ArrayVec<[Rect; DAMAGE_RECT_CAP]> = Default::default();
            match plan {
                RenderPlan::Partial { region, .. } => {
                    // Outset, not inset: damage rects can be thinner than
                    // `2 * gap_px` (a 1px text caret), and insetting would
                    // collapse them to zero area — no outline drawn. An
                    // outset box always survives and brackets the damage
                    // from just outside. The overlay pass is unscissored
                    // and the surface clips, so spilling a few px past the
                    // damage edge is fine.
                    for r in region.iter_rects() {
                        overlay_rects.push(r.scaled_by(buffer.scale, true).inflated(gap_px));
                    }
                }
                // The full-viewport outline insets instead: outsetting it
                // would push the whole box off-screen, leaving only a
                // half-clipped edge line.
                RenderPlan::Full { .. } => overlay_rects.push(
                    Rect {
                        min: glam::Vec2::ZERO,
                        size: Size::new(buffer.viewport_phys_f.x, buffer.viewport_phys_f.y),
                    }
                    .deflated_by(Spacing::all(gap_px)),
                ),
            }
            if overlay_rects.is_empty() {
                return;
            }
            // Short-lived ctx just for the overlay upload — the main
            // upload phase in `submit` has already closed its ctx but
            // the belt is still open until `belt.finish()`. Scoped so
            // the encoder borrow releases before `begin_render_pass`
            // below.
            {
                let mut ctx =
                    GpuCtx::new(&self.device, &self.queue, &mut self.staging_belt, encoder);
                self.debug.upload_overlays(
                    &mut ctx,
                    &overlay_rects,
                    DAMAGE_OVERLAY_COLOR,
                    stroke_width,
                );
            }
            let surface_view = surface_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.renderer.overlay.damage_rect"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
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
            });
            // Damage-overlay pass uses the quad pipeline against the
            // swapchain — separate pass, no inherited state. `draw_overlays`
            // binds the pipeline and pushes viewport in the right order.
            let viewport = ViewportPush {
                size: self.viewport_size,
            };
            self.debug.draw_overlays(
                &mut pass,
                &self.quad,
                &self.gradient.bg,
                &viewport,
                overlay_rects.len() as u32,
            );
        }
    }

    /// Skip path: the host's damage compute returned `None`, but the
    /// swapchain target still needs valid pixels (visual tests capture
    /// it unconditionally; the showcase short-circuits earlier, but
    /// other hosts may not). Ensure the backbuffer matches the
    /// swapchain size, then copy it through. A freshly (re)created
    /// backbuffer has undefined contents — `ensure_backbuffer` forces
    /// the next painting frame to `Full` via the same signal, so the
    /// one-frame glitch self-heals.
    pub(crate) fn copy_backbuffer_to_surface(&mut self, surface_tex: &wgpu::Texture) {
        self.ensure_backbuffer(surface_tex.size(), surface_tex.format());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.skip"),
            });
        self.copy_backbuffer_into(&mut encoder, surface_tex);
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    #[profiling::function]
    fn copy_backbuffer_into(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_tex: &wgpu::Texture,
    ) {
        let bb = &self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded")
            .tex;
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: bb,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: surface_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bb.size(),
        );
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    //! Reach-in introspection for the surface-format-change tests:
    //! the current color format and the GPU image-cache occupancy,
    //! used to assert a format flip rebuilds pipelines without dropping
    //! or re-uploading cached textures.

    use crate::renderer::backend::*;

    impl WgpuBackend {
        /// Current swapchain color format the pipelines were built for.
        pub(crate) fn color_format(&self) -> wgpu::TextureFormat {
            self.color_format
        }

        /// Images resident in the GPU texture cache — see
        /// [`ImagePipeline::gpu_cached_count`].
        pub(crate) fn gpu_image_cache_len(&self) -> usize {
            self.image.gpu_cached_count()
        }
    }
}

#[cfg(test)]
mod tests;
