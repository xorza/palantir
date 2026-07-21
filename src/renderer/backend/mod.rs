mod curve_pipeline;
mod dynamic_buffer;
mod format_pipelines;
pub(crate) mod gpu_ctx;
mod gpu_timings;
mod gradient_resources;
pub(crate) mod image_pipeline;
mod mesh_pipeline;
mod overlay_pass;
pub(crate) mod pipeline_utils;
mod quad_pipeline;
pub(crate) mod queue;
mod schedule;
mod shader_template;
mod stencil;
pub(crate) mod text;
pub(crate) mod viewport;
#[cfg(feature = "internals")]
pub(crate) mod write_stats;

use self::curve_pipeline::CurvePipeline;
use self::format_pipelines::FormatPipelines;
use self::gpu_ctx::GpuCtx;
use self::gpu_timings::GpuTimings;
use self::gradient_resources::GradientResources;
use self::image_pipeline::ImagePipeline;
use self::mesh_pipeline::MeshPipeline;
use self::overlay_pass::DebugOverlay;
use self::quad_pipeline::QuadPipeline;
use self::queue::Queue;
use self::schedule::{RenderStep, for_each_step};
use self::stencil::STENCIL_FORMAT;
use self::viewport::{RepaintScissors, ViewportPush, build_repaint_scissors};
use crate::diagnostics::DebugOverlayConfig;
use crate::diagnostics::gpu_stats::{BatchKind, GpuPassStats};
use crate::primitives::urect::URect;
use crate::renderer::backend::text::TextBackend;
use crate::renderer::gradient_atlas::handle::GradientAtlas;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::plan::RenderPlan;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner::RenderOwnerId;
use crate::scene::record_store::RecordPayloads;
use crate::text::TextShaper;
use rustc_hash::FxHashMap;
use wgpu::util::StagingBelt;

/// Size of the per-pipeline immediate (push-constant) region every
/// aperture shader's `var<immediate> imm: Immediates` reads. Locked
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

/// Persistent off-screen *color* target for the backbuffer-copy path: the
/// frontend renders into it, then [`WgpuBackend::submit`] copies it onto the
/// caller's surface. Keeping last frame's pixels in a texture *we* own is what
/// lets `LoadOp::Load` work for incremental damage — a fresh or rotating
/// surface texture can't be relied on. The direct-present path skips the
/// backbuffer entirely and renders straight into the surface.
///
/// Sized to match the surface texture; recreated on resize or format change.
/// Owned per-window by
/// [`WindowDriver`](crate::host::window_driver::WindowDriver); the backend is
/// otherwise window-agnostic.
#[derive(Debug)]
pub(crate) struct Backbuffer {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    /// Cached at creation: lets `ensure_backbuffer` skip the
    /// `wgpu::Texture::size()` round-trip on every frame. The Arc
    /// traversal that call walks is ~15 µs/frame at this bench
    /// shape — small but visible in Tracy at 14% of trace time.
    size: wgpu::Extent3d,
}

/// Per-window stencil attachment for rounded-clip masking, allocated lazily on
/// the first rounded-clip frame and resized to match the render target. Kept
/// separate from [`Backbuffer`] so the direct-present path can have a stencil
/// without paying for a backbuffer color texture it never uses. Transient:
/// cleared at pass open, never read across frames. Owned per-window by
/// [`WindowDriver`](crate::host::window_driver::WindowDriver).
#[derive(Debug)]
pub(crate) struct Stencil {
    pub(crate) view: wgpu::TextureView,
    /// Current size, so `ensure_stencil` can skip recreation when unchanged.
    size: wgpu::Extent3d,
}

#[derive(Debug)]
pub(crate) struct SubmissionTargets<'a> {
    pub(crate) surface: &'a wgpu::Texture,
    pub(crate) backbuffer: Option<&'a Backbuffer>,
    pub(crate) stencil: Option<&'a wgpu::TextureView>,
}

#[derive(Debug)]
pub(crate) struct Submission<'a> {
    pub(crate) owner: RenderOwnerId,
    pub(crate) targets: SubmissionTargets<'a>,
    pub(crate) payloads: &'a RecordPayloads,
    pub(crate) buffer: &'a RenderBuffer,
    pub(crate) plan: RenderPlan,
    pub(crate) debug_overlay: DebugOverlayConfig,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BackendConfig {
    pub(crate) collect_gpu_stats: bool,
}

#[derive(Debug)]
pub(crate) struct BackendResources {
    pub(crate) text: TextShaper,
    pub(crate) images: ImageRegistry,
    pub(crate) gradient_atlas: GradientAtlas,
    pub(crate) gpu_pass_stats: GpuPassStats,
}

/// Wgpu renderer owning its device/queue handles, pipelines, and text backend.
/// The winit adapter retains cloned handles solely for surface configuration
/// and presentation.
/// The text side holds a
/// shared handle to the same `CosmicMeasure` the Ui side measures
/// against (passed in at [`Self::new`]) so layout-time measurement
/// and rasterization hit one buffer cache. No layout, no encode, no
/// compose — those happen elsewhere and arrive here as a
/// `RenderBuffer`.
#[derive(Debug)]
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
    staging_belt: StagingBelt,
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
    /// Shared CPU gradient rows flushed into the GPU atlas each frame.
    gradient_atlas: GradientAtlas,
    /// Main-pass timestamp queries. `Some` when the host opted into
    /// instrumentation and the device was created with `TIMESTAMP_QUERY`
    /// enabled. Publishes into the host's shared `GpuPassStats` handle;
    /// with one shared backend the published sample reflects the most recently
    /// submitted window.
    gpu_timings: Option<GpuTimings>,
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
        let gradient = GradientResources::new(&device);
        let quad = QuadPipeline::new(&device);
        let mesh = MeshPipeline::new(&device);
        let image = ImagePipeline::new(&device);
        let curve = CurvePipeline::new(&device);
        let text = TextBackend::new(&device, resources.text);
        let debug = DebugOverlay::new(&device);
        // Per-format pipeline sets build lazily on the first submit that
        // targets each format (`ensure_format`); none at construction.
        let pipelines = FxHashMap::default();
        // 1 MiB chunks: comfortably above the resizing-arm's ~500 KB
        // per-frame upload peak, so we land in 1-2 chunks during
        // steady state. wgpu allocates a new chunk only when the
        // active one can't fit a write.
        let staging_belt = StagingBelt::new(device.clone(), 1 << 20);
        let queue = Queue::new(queue);
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
                    resources.gpu_pass_stats,
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
            curve,
            text,
            debug,
            pipelines,
            images: resources.images,
            gradient_atlas: resources.gradient_atlas,
            gpu_timings,
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
                &self.gradient.bgl,
                &self.quad,
                &self.mesh,
                &self.image,
                &self.curve,
                &self.text,
            );
            self.pipelines.insert(format, built);
        }
    }

    /// Lazily (re)create the backbuffer to match the surface texture's
    /// size. Returns `true` if the backbuffer was just (re)created — the
    /// caller asserts its plan is already Full (a recreate implies a
    /// size / format / freshness change upstream, all of which force Full
    /// before the draw list builds; the new texture's contents are
    /// undefined until the first pass writes to it). The
    /// `format` is the per-window surface format; the matching pipeline
    /// set is fetched per submit from the `pipelines` map, so no
    /// global-format assert is needed.
    #[profiling::function]
    pub(crate) fn ensure_backbuffer(
        &self,
        bb: &mut Option<Backbuffer>,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
    ) -> bool {
        let needs_new = match &*bb {
            None => true,
            // Recreate on a size *or* format change: the per-window
            // backbuffer carries one surface's pixels, and a format flip
            // (window moved to an HDR output) needs a fresh texture at the
            // new format to match this submit's pipeline set.
            Some(b) => b.size != size || b.tex.format() != format,
        };
        if !needs_new {
            return false;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aperture.renderer.backbuffer"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        *bb = Some(Backbuffer { tex, view, size });
        true
    }

    /// Allocate (or resize) the stencil attachment to match `size`. Lazily
    /// created on the first rounded-clip frame; recreated when the render
    /// target's size changes (a mismatched-size attachment fails wgpu
    /// validation). The [`Stencil`] is owned per-window by the caller.
    #[profiling::function]
    pub(crate) fn ensure_stencil(&self, stencil: &mut Option<Stencil>, size: wgpu::Extent3d) {
        if stencil.as_ref().is_some_and(|s| s.size == size) {
            return;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aperture.renderer.stencil"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        *stencil = Some(Stencil {
            view: tex.create_view(&wgpu::TextureViewDescriptor::default()),
            size,
        });
    }

    /// Device limit `max_texture_dimension_2d`, read by the frontend `Composer`
    /// to cap each `GpuView`'s off-screen-target size (ceiled from the composed
    /// paint rect). The only backend-owned input the composer needs.
    pub(crate) fn max_texture_dim(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
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
    /// Two damage paths, branching on the `plan`'s damage region:
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
    /// Skip frames never reach this method — `WindowDriver::render_to_texture`
    /// dispatches them to the copy / no-op paths.
    ///
    /// `via_backbuffer` `Some` renders into that backbuffer and copies the
    /// result out (backbuffer-copy path); `None` renders straight into
    /// `surface_tex` (direct present). `plan` is the *effective* plan — every
    /// escalation (promote / resync) was sealed in `present_mode` *before* the
    /// draw list was built, so `plan` and `buffer` always agree; the caller
    /// (`WindowDriver`) has also ensured the stencil + backbuffer.
    #[profiling::function]
    pub(crate) fn submit(&mut self, submission: Submission<'_>) {
        let Submission {
            owner,
            targets:
                SubmissionTargets {
                    surface: surface_tex,
                    backbuffer: via_backbuffer,
                    stencil: stencil_view,
                },
            payloads,
            buffer,
            plan,
            debug_overlay,
        } = submission;
        // The composer may have folded a viewport-covering root
        // background quad into the clear (see
        // `RenderBuffer::clear_override`); it replaces the plan's clear
        // for both the Full-pass `LoadOp::Clear` and the Partial
        // pre-clear quad.
        let clear = buffer.clear_override.unwrap_or(plan.clear);
        let use_stencil = stencil_view.is_some();
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.viewport_phys,
            requested_plan = ?plan,
            rounded_clip = use_stencil,
            "wgpu_backend.submit"
        );

        // Build (once) + select the pipeline set for this surface's
        // format. Read back as `&self.pipelines[&format]` after the
        // `&mut self` upload phase so the borrows don't collide.
        let format = surface_tex.format();
        self.ensure_format(format);

        let repaint_scissors = build_repaint_scissors(plan.kind, buffer);
        let is_partial = matches!(repaint_scissors, RepaintScissors::Partial(_));
        // `dim_undamaged` debug mode: every Partial frame, before any
        // damage passes, draw one full-viewport 40%-translucent black
        // quad onto the backbuffer with `LoadOp::Load`. Each frame
        // undamaged pixels are dimmed once; damaged pixels are dimmed
        // then immediately overwritten by the fresh repaint, so they
        // stay bright. Across many frames the static background fades
        // toward black while moving content stays current — far less
        // jarring than the prior `LoadOp::Clear` flash and visually
        // pins which regions are actually repainting.
        let dim_undamaged = debug_overlay.dim_undamaged && is_partial;

        // The stencil texture (rounded-clip masking) is ensured by the
        // caller; `stencil_view` is `Some` exactly when `use_stencil`. The
        // mask quads upload further down, after the encoder is open.

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
                label: Some("aperture.renderer.main"),
            });

        // Belt-routed upload phase. Scoped so the borrows release
        // before the render-pass phase needs `&mut encoder` cleanly;
        // yields the damage-overlay instance count for the post-copy
        // overlay pass.
        let overlay_count = {
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
            self.gradient.upload(&ctx, &self.gradient_atlas);
            self.image.drain_registry(&mut ctx, &self.images);

            if dim_undamaged {
                self.debug.upload_dim(&mut ctx, buffer.viewport_phys_f);
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
                &payloads.meshes.vertices,
                &payloads.meshes.indices,
                buffer.meshes.instance(),
            );
            self.image
                .upload_instances(&mut ctx, buffer.images.instance());
            // Paint every GpuView composited this frame into its off-screen
            // target on this same encoder, before the main pass samples it.
            // The composer listed them in `buffer.frame_targets` (size + scales
            // + paint callback); this allocates each + runs its callback, then
            // evicts this submitter's targets absent from `frame_targets`
            // (eviction is owner-scoped — the shared backend serves every
            // window).
            // `submit` itself carries no render-target logic.
            self.image
                .paint_gpu_views(&mut ctx, &buffer.frame_targets, owner, buffer.time);
            self.curve.upload(&mut ctx, &buffer.curves);

            if is_partial {
                self.quad
                    .upload_clear(&mut ctx, buffer.viewport_phys_f, clear);
            }

            // Text prepare: per-batch glyph encoding. Routes its
            // vertex/atlas-staging writes through the same ctx so
            // every text-backend write lands as
            // `copy_buffer_to_buffer` on the main encoder. Viewport
            // and atlas-size params ride the shared immediate region,
            // pushed per batch by `TextBackend::render_batch` — no
            // per-frame sync from here.
            {
                profiling::scope!(
                    "text.prepare_batches",
                    &format!("count={}", buffer.text_batches.len())
                );
                for (i, b) in buffer.text_batches.iter().enumerate() {
                    let runs = &buffer.texts[b.texts.range()];
                    self.text.prepare_batch(&mut ctx, buffer.scale, i, runs);
                }
            }

            // One deferred vbuf write covering every batch prepared
            // above, then the queued glyph-atlas uploads (grow blits +
            // per-glyph copy_buffer_to_texture) on the same encoder so
            // they share the main render submit. The staging side of
            // those copies also routes through the belt — see
            // `TextBackend::flush` / `atlas::flush_pending_uploads`.
            self.text.flush(&mut ctx);

            overlay_count
        };

        // Both repaint shapes go through one `begin_render_pass`:
        // - Full: one schedule walk with no scissor,
        //   `LoadOp::Clear(color)` covers the backbuffer.
        // - Partial: an optional dim
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
        // regions. Aperture doesn't support transparent windows
        // (and the occlusion-prune assumes the clear is opaque).
        let clear_color = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: 1.0,
        };
        // Shared field borrow (the entry was built by `ensure_format`
        // above) — coexists with the `&self` pass methods.
        let fmt = &self.pipelines[&format];
        // Render target: the backbuffer's own view (copied out below) or, on
        // the direct-present path, a fresh view of the surface itself.
        let surface_view;
        let color_view: &wgpu::TextureView = match via_backbuffer {
            Some(bb) => &bb.view,
            None => {
                surface_view = surface_tex.create_view(&wgpu::TextureViewDescriptor::default());
                &surface_view
            }
        };
        if let RepaintScissors::Partial(rects) = &repaint_scissors {
            tracing::trace!(
                rects = rects.iter().count(),
                "wgpu_backend.submit.pass.partial"
            );
        } else {
            tracing::trace!("wgpu_backend.submit.pass.full");
        }
        if dim_undamaged {
            tracing::trace!("wgpu_backend.submit.pass.dim");
            let viewport = ViewportPush {
                size: buffer.viewport_phys_f,
            };
            self.run_dim_pass(fmt, color_view, &mut encoder, viewport);
        }
        self.run_main_pass(
            fmt,
            color_view,
            stencil_view,
            &mut encoder,
            buffer,
            &repaint_scissors,
            clear_color,
        );

        if let Some(bb) = via_backbuffer {
            self.copy_backbuffer_into(bb, &mut encoder, surface_tex);
        }

        if overlay_count > 0 {
            let viewport = ViewportPush {
                size: buffer.viewport_phys_f,
            };
            self.run_overlay_pass(fmt, surface_tex, &mut encoder, viewport, overlay_count);
        }

        // Last step before encoder.finish(): resolve the main-pass
        // timestamps if timing is on. The main pass closed before
        // copy_backbuffer_into; the resolve can ride in the same
        // command buffer as everything else.
        if let Some(t) = self.gpu_timings.as_mut() {
            t.resolve(&mut encoder);
        }

        // Close the belt and tie its chunk remap to this frame's
        // submission: `finish_and_recall_on_submit` records a
        // `map_buffer_on_submit` onto the encoder, so the just-used
        // chunks re-map automatically once the submission completes —
        // no explicit `recall()`. Must precede `encoder.finish()`,
        // which needs the still-live encoder. Chunks come back when the
        // map callback fires off a `device.poll`: a `PollType::Wait`
        // caller sees them next frame; a `PollType::Poll` caller may
        // allocate one more chunk during the catch-up window, which
        // wgpu's docs flag as harmless.
        self.staging_belt.finish_and_recall_on_submit(&encoder);
        self.queue.submit(std::iter::once(encoder.finish()));

        // Kick the map_async on this frame's staging slot and read
        // back any prior frame whose map has completed. Cheap (one
        // device.poll(Poll), one memcpy on the ready slot).
        if let Some(t) = self.gpu_timings.as_mut() {
            t.after_submit(&self.device);
        }

        self.text.post_record();
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
        let mut pass = begin_load_pass(encoder, "aperture.renderer.dim.pass", color_view);
        self.debug.draw_dim(
            &mut pass,
            fmt.quad.select(false),
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
    /// `DAMAGE_AA_PADDING` can make nominally-disjoint rects' padded
    /// scissors overlap, and the stencil clears once per pass. Each
    /// `render_groups` call's fresh `active_mask = None` therefore
    /// always matches the true stencil contents.
    ///
    /// `RepaintScissors::Full` runs one schedule walk with no damage
    /// scissor and clears the whole backbuffer. `Partial` loads the
    /// prior color and runs once per non-empty scissor.
    #[profiling::function]
    #[allow(clippy::too_many_arguments)]
    fn run_main_pass(
        &self,
        fmt: &FormatPipelines,
        color_view: &wgpu::TextureView,
        stencil_view: Option<&wgpu::TextureView>,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &RenderBuffer,
        repaint_scissors: &RepaintScissors,
        clear: wgpu::Color,
    ) {
        let use_stencil = stencil_view.is_some();
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
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("aperture.renderer.main.pass"),
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
            if t.inside_passes {
                t.pass_begin(&mut pass);
            }
            t.begin_pipeline_stats(&mut pass);
        }
        match repaint_scissors {
            RepaintScissors::Full => self.render_groups(fmt, &mut pass, buffer, None, use_stencil),
            RepaintScissors::Partial(rects) => {
                let rect_count = rects.iter().count();
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
            if t.inside_passes {
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
        fmt: &'a FormatPipelines,
        pass: &mut wgpu::RenderPass<'a>,
        buffer: &RenderBuffer,
        damage_scissor: Option<URect>,
        use_stencil: bool,
    ) {
        // Track what pipeline + vertex buffer is currently bound so we
        // can skip redundant `set_pipeline` / `set_vertex_buffer` calls
        // across consecutive same-kind steps. wgpu records every
        // `set_pipeline` as a real command — drivers don't dedupe.
        // `PreClear` and the text backend's render set their own state,
        // so we reset to `None` after them and re-bind on the next
        // non-text step.
        #[derive(PartialEq, Eq)]
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
        let viewport = ViewportPush {
            size: buffer.viewport_phys_f,
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
                    self.quad
                        .bind_clear(pass, &fmt.quad, use_stencil, &self.gradient.bg);
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
                RenderStep::MaskStamp(mi) => {
                    mark(pass, BatchKind::Mask);
                    pass.push_debug_group("mask_stamp");
                    rebind!(
                        Bound::MaskStamp,
                        self.quad
                            .bind_mask(pass, &fmt.quad_mask_stamp, &self.gradient.bg)
                    );
                    self.quad.draw_mask(pass, mi);
                    pass.pop_debug_group();
                }
                RenderStep::MaskClear(mi) => {
                    mark(pass, BatchKind::Mask);
                    pass.push_debug_group("mask_clear");
                    rebind!(
                        Bound::MaskClear,
                        self.quad
                            .bind_mask(pass, &fmt.quad_mask_clear, &self.gradient.bg)
                    );
                    self.quad.draw_mask(pass, mi);
                    pass.pop_debug_group();
                }
                RenderStep::Quads { range } => {
                    mark(pass, BatchKind::Quads);
                    pass.push_debug_group("quads");
                    rebind!(
                        Bound::QuadInstance,
                        self.quad
                            .bind(pass, &fmt.quad, use_stencil, &self.gradient.bg)
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
                    self.text
                        .render_batch(batch, pass, &fmt.text, use_stencil, &viewport);
                    bound = Bound::None;
                    pass.pop_debug_group();
                }
                RenderStep::MeshBatch { batch } => {
                    mark(pass, BatchKind::Mesh);
                    pass.push_debug_group("meshes");
                    rebind!(Bound::Mesh, self.mesh.bind(pass, &fmt.mesh, use_stencil));
                    let range = buffer.mesh_batches[batch].items;
                    let start = range.start as usize;
                    let end = start + range.len as usize;
                    for (offset, draw) in buffer.meshes.draw()[start..end].iter().enumerate() {
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
                    rebind!(Bound::Image, self.image.bind(pass, &fmt.image, use_stencil));
                    let range = buffer.image_batches[batch].items;
                    let start = range.start as usize;
                    let end = start + range.len as usize;
                    for (offset, id) in buffer.images.id()[start..end].iter().enumerate() {
                        self.image.draw(pass, *id, (start + offset) as u32);
                    }
                    pass.pop_debug_group();
                }
                RenderStep::CurveBatch { batch } => {
                    mark(pass, BatchKind::Curve);
                    pass.push_debug_group("curves");
                    rebind!(
                        Bound::Curve,
                        self.curve
                            .bind(pass, &fmt.curve, use_stencil, &self.gradient.bg)
                    );
                    let range = buffer.curve_batches[batch].items;
                    self.curve.draw(pass, range.start..range.start + range.len);
                    pass.pop_debug_group();
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
        surface_tex: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
        viewport: ViewportPush,
        count: u32,
    ) {
        let surface_view = surface_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = begin_load_pass(
            encoder,
            "aperture.renderer.overlay.damage_rect",
            &surface_view,
        );
        self.debug.draw_overlays(
            &mut pass,
            fmt.quad.select(false),
            &self.gradient.bg,
            &viewport,
            count,
        );
    }

    /// Skip path: the host's damage compute returned `None`, but the
    /// swapchain target still needs valid pixels (visual tests capture
    /// it unconditionally; the showcase short-circuits earlier, but
    /// other hosts may not). A `Skip` requires the previous frame to
    /// have been submitted at this size and format (`classify_frame`
    /// forces `Full` otherwise), so the backbuffer must already exist
    /// and match — copying anything else would present undefined or
    /// stale-format pixels, so crash instead of degrading.
    pub(crate) fn copy_backbuffer_to_surface(
        &self,
        backbuffer: &Backbuffer,
        surface_tex: &wgpu::Texture,
    ) {
        debug_assert!(
            backbuffer.size == surface_tex.size()
                && backbuffer.tex.format() == surface_tex.format(),
            "skip-copy backbuffer doesn't match the target — a Skip frame \
             implies the previous frame painted this size/format"
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("aperture.renderer.skip"),
            });
        self.copy_backbuffer_into(backbuffer, &mut encoder, surface_tex);
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    #[profiling::function]
    fn copy_backbuffer_into(
        &self,
        backbuffer: &Backbuffer,
        encoder: &mut wgpu::CommandEncoder,
        surface_tex: &wgpu::Texture,
    ) {
        let bb = &backbuffer.tex;
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
pub(crate) mod test_support {
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
        /// [`ImagePipeline::gpu_cached_count`].
        pub(crate) fn gpu_image_cache_len(&self) -> usize {
            self.image.gpu_cached_count()
        }
    }
}

#[cfg(test)]
mod tests;
