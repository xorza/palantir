//! The wgpu backend: the one GPU renderer, its per-window attachments, and the
//! pipeline sets it builds per swapchain format.
//!
//! # One frame
//!
//! [`WgpuBackend::submit`] runs two halves that cannot overlap: an upload phase
//! (`&mut self`) recording every texture and dynamic-buffer write, then the render
//! passes, which read `self.pipelines` through a shared borrow.
//!
//! Every instanced pipeline spells the same six steps (`new`, `instance_layout`,
//! `build_variants`, `upload`, `bind`, `draw`); new ones keep them. Text and icon
//! share one `RasterPass` and [`RasterProgram`].
//!
//! Each group's quads draw first, then its text, so a child quad declared after a
//! label occludes it.
//!
//! # Damage
//!
//! - [`Damage::Full`](crate::damage::Damage::Full): one `LoadOp::Clear(clear)` pass
//!   paints every group at its native scissor.
//! - [`Damage::Partial(region)`](crate::damage::Damage::Partial): one render pass per
//!   rect, `LoadOp::Load`ing the backbuffer, with each group's scissor narrowed to
//!   the rect (scaled, padded for AA bleed, clamped; zero-area rects dropped).
//!
//! The `dim_undamaged` debug mode adds a pre-pass on Partial frames in its own pass:
//! the no-stencil dim pipeline is incompatible with the main pass's stencil
//! attachment on rounded-clip frames.
//!
//! # The staging belt
//!
//! The main encoder opens before the first upload, and every dynamic-buffer upload
//! routes through `staging_belt`, which records its copies on that encoder instead of
//! a Metal blit encoder per `queue.write_buffer`. wgpu serialises in record order, so
//! copies land before the passes that read them. `finish_and_recall_on_submit` must
//! precede `encoder.finish()`.
//!
//! # The clear
//!
//! The surface clear is the bottom paint layer, so its alpha is forced to 1: there
//! are no transparent windows and the occlusion prune assumes an opaque clear. A
//! root quad folded into the clear (`RenderBuffer::clear_override`) replaces the
//! plan's clear for the Full pass and the Partial pre-clear quad.
//!
//! # GPU timestamps
//!
//! Main-pass timestamps resolve just before `encoder.finish()`; `after_submit`
//! reads back any earlier frame whose map completed.

use crate::common::tracy;
use crate::diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats};
use crate::gpu::device::backend_config::BackendConfig;
use crate::gpu::device::backend_resources::BackendResources;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::frame::debug_marker;
use crate::gpu::frame::gpu_timings::GpuTimings;
use crate::gpu::frame::overlay_pass::DebugOverlay;
use crate::gpu::frame::schedule::{MaskPlan, RenderStep, for_each_step};
use crate::gpu::frame::submission::{Submission, SubmissionTargets};
use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::format_pipelines::FormatPipelines;
use crate::gpu::pipeline::format_pipelines::PipelineSources;
use crate::gpu::pipeline::image_pipeline::{ImageBatch, ImagePipeline};
use crate::gpu::pipeline::mesh_pipeline::{MeshBatch, MeshPipeline, MeshUpload};
use crate::gpu::pipeline::quad_pipeline::QuadPipeline;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::ShadowEntry;
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::raster::icon_backend::IconBackend;
use crate::gpu::raster::raster_program::RasterProgram;
use crate::gpu::raster::text_backend::TextBackend;
use crate::gpu::resource::gpu_gradient_atlas::GpuGradientAtlas;
use crate::gpu::resource::gpu_view_targets::GpuViewTargets;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::surface::viewport::{RepaintScissors, ViewportPush, build_repaint_scissors};
use crate::primitives::geometry::urect::URect;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_owner_id::RenderOwnerId;
use rustc_hash::FxHashMap;
use std::iter;
use std::rc::Rc;
use std::time::Instant;
use wgpu::util::StagingBelt;

/// Wgpu renderer owning its device/queue handles, pipelines and text backend.
/// The text side holds the same [`TextShaper`](crate::text::shaper::TextShaper) the
/// `Ui` measures against, so measurement and rasterization share one buffer cache.
#[derive(Debug)]
pub(crate) struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Routes per-frame dynamic-buffer uploads onto the main encoder, collapsing N
    /// `queue.write_buffer` calls (each a Metal blit encoder) into one.
    staging_belt: StagingBelt,
    /// Shared gradient LUT atlas lent to the quad and curve pipelines.
    gradient: GpuGradientAtlas,
    quad: QuadPipeline,
    mesh: MeshPipeline,
    image: ImagePipeline,
    /// The registered images; host recorders create, rewrite and free them through the
    /// `ImageRegistry` this backend attached over this `Rc`.
    image_store: Rc<WgpuImageStore>,
    gpu_view_targets: GpuViewTargets,
    /// The one shader and group-0 layout both raster tenants draw through; see [`RasterProgram`].
    raster: RasterProgram,
    blit: BlitPipeline,
    icon: IconBackend,
    curve: CurvePipeline,
    text: TextBackend,
    debug: DebugOverlay,
    /// The group-0 layout and sampler every sampled texture binds through, so a group built for one
    /// binds in every pipeline that samples any.
    texture_binding: TextureBinding,
    /// Rounded-clip chains and mask quads, rebuilt on stencil frames only; retained to avoid
    /// allocation.
    mask_plan: MaskPlan,
    /// Render pipelines keyed by swapchain color format, built lazily by [`Self::ensure_format`].
    /// The surface texture handed to `submit` selects the set.
    pipelines: FxHashMap<TargetFormat, FormatPipelines>,
    /// Main-pass timestamp queries; `Some` when the host opted in and the device has
    /// `TIMESTAMP_QUERY`.
    gpu_timings: Option<GpuTimings>,
    /// The one handle this backend publishes through. Record time is unconditional
    /// (two `Instant::now()` calls and a `RefCell` write); making it opt-in would force
    /// the in-pass timestamp writes that perturb the number.
    pass_stats: GpuPassStats,
}

/// What [`WgpuBackend::run_main_pass`] draws into: color, optional stencil, and clear color, picked
/// together per frame.
#[derive(Clone, Copy, Debug)]
struct PassTarget<'a> {
    color_view: &'a wgpu::TextureView,
    stencil_view: Option<&'a wgpu::TextureView>,
    clear: wgpu::Color,
}

impl WgpuBackend {
    /// Build the one shared GPU renderer over the host's handles and attach the image
    /// store to `resources.images`. Per-format pipelines build lazily; see
    /// [`Self::ensure_format`].
    pub(crate) fn new(gpu: Gpu, resources: BackendResources<'_>, config: BackendConfig) -> Self {
        let Gpu { device, queue } = gpu;
        let texture_binding = TextureBinding::new(&device);
        let image_store = Rc::new(WgpuImageStore::new(
            device.clone(),
            queue.clone(),
            texture_binding.clone(),
        ));
        resources.images.attach(Rc::clone(&image_store));
        let gradient = GpuGradientAtlas::new(
            &device,
            resources.gradient_atlas.clone(),
            texture_binding.clone(),
        );
        let quad = QuadPipeline::new(&device, &texture_binding);
        let mesh = MeshPipeline::new(&device);
        let image = ImagePipeline::new(&device, &texture_binding);
        let gpu_view_targets = GpuViewTargets::new(texture_binding.clone());
        let curve = CurvePipeline::new(&device, &texture_binding);
        let raster = RasterProgram::new(&device);
        let blit = BlitPipeline::new(&device, &texture_binding);
        let text = TextBackend::new(&device, &raster, resources.text.clone());
        let icon = IconBackend::new(&device, &raster, resources.icons.clone());
        let debug = DebugOverlay::new(&device);
        let pipelines = FxHashMap::default();
        // 1 MiB chunks, above the resize-drag upload peak, so steady state uses 1-2 chunks.
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
                    features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
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
            image_store,
            gpu_view_targets,
            raster,
            blit,
            icon,
            curve,
            text,
            debug,
            texture_binding,
            mask_plan: MaskPlan::default(),
            pipelines,
            gpu_timings,
            pass_stats: resources.gpu_pass_stats.clone(),
        }
    }

    /// Ensure the pipeline set for `format` exists. Only `wgpu::RenderPipeline`s carry
    /// the color-target format, so a new format costs a few pipeline compiles and no
    /// image re-upload or glyph re-rasterization.
    fn ensure_format(&mut self, format: TargetFormat) {
        // Build then insert: the borrow checker can't see the builder's inputs are disjoint from
        // `self.pipelines` through `entry().or_insert_with`.
        if !self.pipelines.contains_key(&format) {
            let built = FormatPipelines::new(
                &self.device,
                format.get(),
                PipelineSources {
                    quad: &self.quad,
                    mesh: &self.mesh,
                    image: &self.image,
                    curve: &self.curve,
                    raster: &self.raster,
                    blit: &self.blit,
                },
            );
            self.pipelines.insert(format, built);
        }
    }

    /// Render one frame into the submission's target and present it. Skip frames
    /// never reach this method.
    ///
    /// [`SubmissionTargets::backbuffer`] picks the path: `Some` renders into it and
    /// copies onto [`surface`](SubmissionTargets::surface), whose texture needs
    /// `COPY_DST`; `None` renders straight into the surface. [`Submission::plan`] is the
    /// effective plan: escalations were sealed before the draw list was built.
    pub(crate) fn submit(&mut self, submission: Submission<'_>) {
        tracy::zone!();
        let SubmissionTargets {
            surface: target,
            backbuffer: via_backbuffer,
            stencil,
        } = submission.targets;
        let Submission { buffer, plan, .. } = submission;
        let clear = submission.clear();
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

        let surface_tex = target.texture();
        let format = target.format();
        self.ensure_format(format);

        let viewport = ViewportPush::for_buffer(buffer);
        let repaint_scissors = build_repaint_scissors(plan.damage, buffer);
        let dim_undamaged = submission.dim_undamaged();

        // One encoder: the belt's copies must land before the passes that read them.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.main"),
            });

        let overlay_count = self.upload_frame(&mut encoder, &submission);

        let clear = clear.unpack();
        let clear_color = wgpu::Color {
            r: f64::from(clear.r),
            g: f64::from(clear.g),
            b: f64::from(clear.b),
            a: f64::from(clear.a),
        };
        let fmt = &self.pipelines[&format];
        // At most one surface view, only where a pass reads it; the normal backbuffer path avoids a
        // view per frame.
        let surface_view = (via_backbuffer.is_none() || overlay_count > 0 || !target.takes_copy())
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
            viewport,
        );

        if let Some(bb) = via_backbuffer {
            if let Some(t) = &self.gpu_timings {
                t.copy_out_begin(&mut encoder);
            }
            if target.takes_copy() {
                bb.copy_onto(&mut encoder, surface_tex);
            } else {
                let view = surface_view
                    .as_ref()
                    .expect("a target that takes no copy builds the surface view");
                bb.draw_onto(&mut encoder, view, &fmt.blit);
            }
            if let Some(t) = &self.gpu_timings {
                t.copy_out_end(&mut encoder);
            }
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
        self.queue.submit(iter::once(encoder.finish()));

        if let Some(t) = self.gpu_timings.as_mut() {
            t.after_submit(&self.device, &self.pass_stats);
        }

        let frame = self.text.end_frame();
        self.icon.end_frame(frame);
    }

    /// The belt-routed upload phase of one [`Self::submit`]; returns the damage-overlay instance
    /// count.
    fn upload_frame(&mut self, encoder: &mut wgpu::CommandEncoder, sub: &Submission<'_>) -> u32 {
        let Submission {
            owner,
            targets,
            store,
            buffer,
            plan,
            cutouts,
            debug_overlay,
        } = *sub;
        let clear = sub.clear();
        let dim_undamaged = sub.dim_undamaged();
        let use_stencil = targets.stencil.is_some();
        let is_partial = plan.damage.is_partial();
        let mut ctx = GpuCtx::new(&self.device, &self.queue, &mut self.staging_belt, encoder);

        // Texture-only upload (the belt is buffer-only), first so draws see the right pixels.
        self.gradient.upload(&ctx);

        if dim_undamaged {
            self.debug
                .upload_dim(&mut ctx, buffer.display.physical.as_vec2());
        }
        // Damage-rect overlay quads (debug): uploaded now, drawn last after the backbuffer copy.
        let overlay_count = if debug_overlay.damage_rect {
            self.debug.upload_damage_rects(&mut ctx, plan, buffer)
        } else {
            0
        };
        if use_stencil {
            tracy::zone!("stage_masks");
            self.mask_plan.build(buffer);
            self.quad.upload_masks(&mut ctx, self.mask_plan.quads());
        }

        self.quad.upload(&mut ctx, &buffer.quads, cutouts);
        self.mesh.upload(
            &mut ctx,
            MeshUpload {
                vertices: &store.meshes.vertices,
                indices: &store.meshes.indices,
                instances: buffer.meshes.instance(),
            },
        );
        self.image.upload(&mut ctx, buffer.images.instance());
        // Paint every GpuView composited this frame into its off-screen target before
        // the main pass samples it, then free this submitter's targets absent from
        // `buffer.live_targets` (a wider set than those painted, so an unchanged view keeps
        // its texture). Eviction is owner-scoped because the backend serves every window.
        self.gpu_view_targets.paint_gpu_views(
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

        {
            tracy::zone!(
                "text.prepare_batches",
                value = buffer.text_batches.len() as u64
            );
            let interned_text = store.interned_text();
            for (i, b) in buffer.text_batches.iter().enumerate() {
                let runs = &buffer.texts[b.texts.range()];
                self.text.prepare_batch(
                    &mut ctx,
                    buffer.display.scale_factor(),
                    i,
                    runs,
                    &interned_text,
                );
            }
        }

        // One deferred vbuf write for every prepared batch, then queued glyph-atlas uploads staged
        // through the belt.
        self.text.flush(&mut ctx);

        // Icons: prewarm any filtered icon at this scale (an SVG filter is 10-20x an ordinary
        // raster, so a lazy hit drops a frame), then encode each batch.
        {
            tracy::zone!(
                "icon.prepare_batches",
                value = buffer.batches(PaintTier::Icon).len() as u64
            );
            self.icon.prewarm(&mut ctx, buffer.display.scale_factor());
            for (i, b) in buffer.batches(PaintTier::Icon).iter().enumerate() {
                let rows = &buffer.icons[b.items.range()];
                self.icon.prepare_batch(&mut ctx, i, rows);
            }
        }
        self.icon.flush(&mut ctx);

        overlay_count
    }

    /// Full-viewport pass drawing a translucent black quad over the backbuffer when `dim_undamaged`
    /// is on. No stencil attachment; partial passes set their own.
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
            fmt.quad.color(QuadForm::Solid).select(false),
            &self.gradient.bg,
            viewport,
        );
    }

    /// Open the main render pass against the backbuffer and walk the schedule once
    /// per damage rect (once, unscissored, on Full): one `begin_render_pass`, one stencil
    /// `LoadOp::Clear(0)`, one color load.
    ///
    /// Every walk leaves the stencil clean: one ending with a mask stamped emits a tail
    /// clear under the stamp's scissor. That, not rect disjointness, keeps one rect's
    /// stencil writes from a later rect's reads, since `RenderPlan::AA_PADDING` can make
    /// disjoint rects' padded scissors overlap.
    ///
    /// Host CPU time for all of it, including the end-of-pass replay on `pass`'s drop,
    /// publishes to [`GpuPassStats::last_main_pass_cpu`]; it scales with draw-step count
    /// and is what the `record_pass` benchmark reads.
    fn run_main_pass(
        &self,
        fmt: &FormatPipelines,
        target: PassTarget<'_>,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &RenderBuffer,
        repaint_scissors: &RepaintScissors,
        viewport: ViewportPush,
    ) {
        tracy::zone!();
        let PassTarget {
            color_view,
            stencil_view,
            clear,
        } = target;
        let masks = stencil_view.map(|_| &self.mask_plan);
        let depth_stencil_attachment =
            stencil_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    // One stencil clear per pass: the schedule's tail clear makes that sufficient.
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Discard,
                }),
            });
        let load_op = match repaint_scissors {
            RepaintScissors::Full => wgpu::LoadOp::Clear(clear),
            RepaintScissors::Partial(_) => wgpu::LoadOp::Load,
        };
        // The descriptor covers basic mode; with `TIMESTAMP_QUERY_INSIDE_PASSES` we write inline
        // via `pass_begin` / `pass_end` for one gap-free stream.
        let timestamp_writes = self.gpu_timings.as_ref().and_then(GpuTimings::pass_writes);
        let started = Instant::now();
        // Scoped so `pass` drops, replaying its commands, inside the measured window.
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
                    self.render_groups(fmt, &mut pass, buffer, None, masks, viewport);
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
                        self.render_groups(fmt, &mut pass, buffer, Some(r), masks, viewport);
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

    /// Dispatch each step of the per-frame schedule ([`for_each_step`]) to the wgpu
    /// pass; ordering lives in the schedule module so tests assert it without a GPU.
    /// `masks` is `Some` exactly when the pass has a stencil attachment.
    fn render_groups<'a>(
        &'a self,
        fmt: &'a FormatPipelines,
        pass: &mut wgpu::RenderPass<'a>,
        buffer: &RenderBuffer,
        damage_scissor: Option<URect>,
        masks: Option<&MaskPlan>,
        viewport: ViewportPush,
    ) {
        // Tracks the bound pipeline and vertex buffer to skip redundant calls (wgpu records every
        // `set_pipeline`; drivers don't dedupe). `PreClear` resets it to `None`.
        #[derive(Debug, PartialEq, Eq)]
        enum Bound {
            None,
            QuadInstance(QuadForm),
            ShadowInstance(ShadowEntry),
            Mesh,
            Image,
            Curve,
            MaskStamp,
            MaskClear,
            /// Text and icons share one shader and group-0 layout, hence one pipeline pair.
            /// Both arms bind `raster_pipeline`; a separate pair would draw one tenant's atlas
            /// through the other's shader (the visual suite's
            /// `an_icon_between_two_text_runs_lands_in_its_own_pixel_box` would fail).
            Raster,
        }

        // `viewport.push_into(pass)` follows every (re)bind (the rule `IMMEDIATES_BYTES`
        // states). `rebind` bundles bind, viewport push and recording `bound`; `PreClear`
        // stays open-coded since it uses its own vertex buffer.
        fn rebind<'p>(
            bound: &mut Bound,
            target: Bound,
            pass: &mut wgpu::RenderPass<'p>,
            viewport: ViewportPush,
            bind: impl FnOnce(&mut wgpu::RenderPass<'p>),
        ) {
            if *bound != target {
                bind(pass);
                viewport.push_into(pass);
                *bound = target;
            }
        }

        tracy::zone!();
        let images = self.image_store.read();
        let mut bound = Bound::None;
        let use_stencil = masks.is_some();
        let raster_pipeline = fmt.raster.select(use_stencil);

        let mark = |pass: &mut wgpu::RenderPass<'a>, kind: BatchKind| {
            if let Some(t) = self.gpu_timings.as_ref() {
                t.mark(pass, kind);
            }
        };

        for_each_step(buffer, damage_scissor, masks, &mut |step| match step {
            RenderStep::PreClear => {
                mark(pass, BatchKind::PreClear);
                debug_marker::push(pass, "preclear");
                // bind, push viewport, then draw; otherwise the clear quad reads stale immediates
                // (zero on a partial pass's first PreClear), skipping the damage-region clear.
                self.quad.bind_clear(
                    pass,
                    fmt.quad.color(QuadForm::Solid),
                    use_stencil,
                    &self.gradient.bg,
                );
                viewport.push_into(pass);
                pass.draw(0..4, 0..1);
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
                rebind(&mut bound, Bound::MaskStamp, pass, viewport, |pass| {
                    self.quad
                        .bind_mask(pass, &fmt.quad.mask_stamp, &self.gradient.bg);
                });
                self.quad.draw_mask(pass, mi);
                debug_marker::pop(pass);
            }
            RenderStep::MaskClear(mi) => {
                mark(pass, BatchKind::Mask);
                debug_marker::push(pass, "mask_clear");
                rebind(&mut bound, Bound::MaskClear, pass, viewport, |pass| {
                    self.quad
                        .bind_mask(pass, &fmt.quad.mask_clear, &self.gradient.bg);
                });
                self.quad.draw_mask(pass, mi);
                debug_marker::pop(pass);
            }
            RenderStep::Quads { range } => {
                mark(pass, BatchKind::Quads);
                debug_marker::push(pass, "quads");
                for run in self.quad.quad_runs(range) {
                    rebind(
                        &mut bound,
                        Bound::QuadInstance(run.key),
                        pass,
                        viewport,
                        |pass| {
                            self.quad.bind(
                                pass,
                                fmt.quad.color(run.key),
                                use_stencil,
                                &self.gradient.bg,
                            );
                        },
                    );
                    self.quad.draw(pass, run.instances);
                }
                debug_marker::pop(pass);
            }
            RenderStep::Shadows { range } => {
                mark(pass, BatchKind::Shadows);
                debug_marker::push(pass, "shadows");
                for run in self.quad.shadow_runs(range) {
                    rebind(
                        &mut bound,
                        Bound::ShadowInstance(run.key),
                        pass,
                        viewport,
                        |pass| {
                            self.quad.bind_shadows(
                                pass,
                                fmt.quad.shadow(run.key),
                                use_stencil,
                                &self.gradient.bg,
                            );
                        },
                    );
                    self.quad.draw_shadows(pass, run.instances);
                }
                debug_marker::pop(pass);
            }
            RenderStep::Text { batch } => {
                mark(pass, BatchKind::Text);
                debug_marker::push(pass, "text");
                rebind(&mut bound, Bound::Raster, pass, viewport, |pass| {
                    pass.set_pipeline(raster_pipeline);
                });
                self.text.render_batch(batch, pass);
                debug_marker::pop(pass);
            }
            RenderStep::TierBatch { tier, batch } => {
                // Timing bucket and label come from the tier, so a new tier cannot reach the pass
                // untimed.
                let kind = batch_kind(tier);
                mark(pass, kind);
                debug_marker::push(pass, kind.label());
                let items = || buffer.batches(tier)[batch].items;
                match tier {
                    PaintTier::Mesh => {
                        rebind(&mut bound, Bound::Mesh, pass, viewport, |pass| {
                            self.mesh.bind(pass, &fmt.mesh, use_stencil);
                        });
                        self.mesh.draw(
                            pass,
                            MeshBatch {
                                draws: buffer.meshes.draw(),
                                items: items(),
                            },
                        );
                    }
                    PaintTier::Image => {
                        rebind(&mut bound, Bound::Image, pass, viewport, |pass| {
                            self.image.bind(pass, &fmt.image, use_stencil);
                        });
                        self.image.draw(
                            pass,
                            ImageBatch {
                                ids: buffer.images.id(),
                                items: items(),
                            },
                            &images,
                            &self.gpu_view_targets,
                        );
                    }
                    PaintTier::Icon => {
                        rebind(&mut bound, Bound::Raster, pass, viewport, |pass| {
                            pass.set_pipeline(raster_pipeline);
                        });
                        self.icon.render_batch(batch, pass);
                    }
                    PaintTier::Curve => {
                        rebind(&mut bound, Bound::Curve, pass, viewport, |pass| {
                            self.curve
                                .bind(pass, &fmt.curve, use_stencil, &self.gradient.bg);
                        });
                        self.curve.draw(pass, items());
                    }
                }
                debug_marker::pop(pass);
            }
        });
    }

    /// Draw the damage-rect debug overlay onto the swapchain texture after the
    /// backbuffer-to-surface copy, so it never lands on the backbuffer and leaves no
    /// ghost stroke.
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
            fmt.quad.color(QuadForm::Solid).select(false),
            &self.gradient.bg,
            viewport,
            count,
        );
    }

    pub(crate) const fn bakes_cutouts(&self) -> bool {
        self.quad.bakes_cutouts()
    }

    pub(crate) const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub(crate) const fn texture_binding(&self) -> &TextureBinding {
        &self.texture_binding
    }

    /// Skip path: the target still needs valid pixels. A `Skip` requires the previous
    /// frame submitted at this size and format (`take_frame_plan` forces `Full`
    /// otherwise), so a missing or mismatched backbuffer crashes rather than present
    /// undefined pixels.
    pub(crate) fn copy_backbuffer_to_surface(
        &self,
        backbuffer: &Backbuffer,
        target: RenderTarget<'_>,
    ) {
        let surface_tex = target.texture();
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
        if target.takes_copy() {
            backbuffer.copy_onto(&mut encoder, surface_tex);
        } else {
            let fmt = self
                .pipelines
                .get(&target.format())
                .expect("a skip implies a prior submit built this format's pipelines");
            let view = surface_tex.create_view(&wgpu::TextureViewDescriptor::default());
            backbuffer.draw_onto(&mut encoder, &view, &fmt.blit);
        }
        self.queue.submit(iter::once(encoder.finish()));
    }

    /// Release every `GpuView` target owned by a retired render stream (called as a
    /// window closes). Per-submit eviction is owner-scoped, so without this survivors
    /// would hold its textures until shutdown.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "not `cfg`: the winit host is only the current caller, and an embedding host needs this entry point too"
        )
    )]
    pub(crate) fn retire_render_owner(&mut self, owner: RenderOwnerId) {
        self.gpu_view_targets.retire_owner(owner);
    }
}

/// The timing bucket and debug label a [`PaintTier`] replay lands in; exhaustive, so a new tier
/// cannot go untimed.
const fn batch_kind(tier: PaintTier) -> BatchKind {
    match tier {
        PaintTier::Mesh => BatchKind::Mesh,
        PaintTier::Image => BatchKind::Image,
        PaintTier::Icon => BatchKind::Icon,
        PaintTier::Curve => BatchKind::Curve,
    }
}

/// A color-only `LoadOp::Load` pass shared by the dim pre-pass and the damage-overlay pass.
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

    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::wgpu_backend::WgpuBackend;

    impl WgpuBackend {
        /// Draw every shadow as one cell of the full form instead of its grid: the reference the
        /// grid is compared with. Drops built pipelines.
        pub(crate) fn disable_shadow_grid(&mut self) {
            self.quad.disable_shadow_grid();
            self.pipelines.clear();
        }

        pub(crate) fn has_format_pipelines(&self, format: TargetFormat) -> bool {
            self.pipelines.contains_key(&format)
        }

        pub(crate) fn gpu_image_cache_len(&self) -> usize {
            self.image_store.resident()
        }
    }
}

#[cfg(test)]
mod tests;
