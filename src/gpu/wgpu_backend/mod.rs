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
//! a Metal blit encoder per `queue.write_buffer`. wgpu serializes in record order, so
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
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::debug_marker;
use crate::gpu::frame::debug_overlay::{DebugOverlay, OverlayDraw};
use crate::gpu::frame::gpu_timings::GpuTimings;
use crate::gpu::frame::schedule::{MaskPlan, RenderStep, Schedule};
use crate::gpu::frame::submission::{Submission, SubmissionTargets};
use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::format_pipelines::FormatPipelines;
use crate::gpu::pipeline::format_pipelines::PipelineSources;
use crate::gpu::pipeline::image_pipeline::{ImageBatch, ImagePipeline, ImageTextures};
use crate::gpu::pipeline::mesh_pipeline::{MeshBatch, MeshPipeline, MeshUpload};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutMode, ShadowEntry};
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::pipeline::quad_pipeline::{QuadBinding, QuadPipeline};
use crate::gpu::pipeline::stencil_variant::StencilMode;
use crate::gpu::raster::icon_backend::IconBackend;
use crate::gpu::raster::raster_program::RasterProgram;
use crate::gpu::raster::text_backend::{TextBackend, TextBatchInput};
use crate::gpu::resource::gpu_gradient_atlas::GpuGradientAtlas;
use crate::gpu::resource::gpu_view_targets::{GpuViewTargets, ViewSubmitter};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::backbuffer::{Backbuffer, EnsuredBackbuffer};
use crate::gpu::surface::capture::Capture;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::surface::viewport::{RepaintScissors, ViewportPush};
use crate::primitives::geometry::urect::URect;
use crate::primitives::paint::color::RgbaF32;
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
    gpu: Gpu,
    /// Routes per-frame dynamic-buffer uploads onto the main encoder, collapsing N
    /// `queue.write_buffer` calls (each a Metal blit encoder) into one.
    staging_belt: StagingBelt,
    /// Shared gradient LUT atlas lent to the quad and curve pipelines.
    gradient: GpuGradientAtlas,
    quad: QuadPipeline,
    mesh: MeshPipeline,
    image: ImagePipeline,
    /// The registered images; host recorders create, rewrite and free them through the
    /// `ImageRegistry` the host built over this `Rc`.
    image_store: Rc<WgpuImageStore>,
    gpu_view_targets: GpuViewTargets,
    /// The one shader and group-0 layout both raster tenants draw through; see [`RasterProgram`].
    raster: RasterProgram,
    blit: BlitPipeline,
    /// The blit into [`Capture::FORMAT`], built on the first screenshot.
    capture_blit: Option<wgpu::RenderPipeline>,
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

/// What every pass of one submit draws with, fixed for the frame.
#[derive(Clone, Copy, Debug)]
struct FrameDraw<'a> {
    fmt: &'a FormatPipelines,
    buffer: &'a RenderBuffer,
    viewport: ViewportPush,
    /// The frame's mask chains; `Some` exactly when the main pass has a stencil attachment.
    masks: Option<&'a MaskPlan>,
}

/// What [`WgpuBackend::run_main_pass`] draws into: color, optional stencil, the clear color and the
/// scissors it repaints, picked together per frame.
#[derive(Clone, Copy, Debug)]
struct MainPass<'a> {
    color_view: &'a wgpu::TextureView,
    stencil_view: Option<&'a wgpu::TextureView>,
    clear: wgpu::Color,
    scissors: &'a RepaintScissors,
}

impl WgpuBackend {
    /// Build the one shared GPU renderer over the host's handles. Per-format pipelines
    /// build lazily; see [`Self::ensure_format`].
    pub(crate) fn new(gpu: Gpu, resources: BackendResources<'_>, config: BackendConfig) -> Self {
        let Gpu { device, queue } = gpu;
        let image_store = Rc::clone(resources.image_store);
        let texture_binding = image_store.binding().clone();
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
            .then(|| GpuTimings::new(&device, timestamp_period, features));
        Self {
            gpu: Gpu::new(device, queue),
            staging_belt,
            gradient,
            quad,
            mesh,
            image,
            image_store,
            gpu_view_targets,
            raster,
            blit,
            capture_blit: None,
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
                &self.gpu.device,
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
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.display.physical,
            requested_plan = ?plan,
            rounded_clip = stencil_view.is_some(),
            "wgpu_backend.submit"
        );

        let surface_texture = target.texture();
        let format = target.format();
        self.ensure_format(format);

        let viewport = ViewportPush::for_buffer(buffer);
        let repaint_scissors = RepaintScissors::new(plan.damage, buffer);
        let dim_undamaged = submission.dim_undamaged();

        // One encoder: the belt's copies must land before the passes that read them.
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.main"),
            });

        self.upload_frame(&mut encoder, &submission);
        let draws_damage_rects = self.debug.has_damage_rects();

        let clear = RgbaF32::from(clear);
        let clear_color = wgpu::Color {
            r: f64::from(clear.r),
            g: f64::from(clear.g),
            b: f64::from(clear.b),
            a: f64::from(clear.a),
        };
        let draw = FrameDraw {
            fmt: &self.pipelines[&format],
            buffer,
            viewport,
            masks: stencil_view.map(|_| &self.mask_plan),
        };
        // At most one surface view, only where a pass reads it; the normal backbuffer path avoids a
        // view per frame.
        let surface_view = (via_backbuffer.is_none() || draws_damage_rects || !target.takes_copy())
            .then(|| surface_texture.create_view(&wgpu::TextureViewDescriptor::default()));
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
            self.run_dim_pass(draw, color_view, &mut encoder);
        }
        self.run_main_pass(
            draw,
            MainPass {
                color_view,
                stencil_view,
                clear: clear_color,
                scissors: &repaint_scissors,
            },
            &mut encoder,
        );

        if let Some(bb) = via_backbuffer {
            if let Some(t) = &self.gpu_timings {
                t.copy_out_begin(&mut encoder);
            }
            if target.takes_copy() {
                bb.copy_onto(&mut encoder, surface_texture);
            } else {
                let view = surface_view
                    .as_ref()
                    .expect("a target that takes no copy builds the surface view");
                bb.draw_onto(&mut encoder, view, &draw.fmt.blit);
            }
            if let Some(t) = &self.gpu_timings {
                t.copy_out_end(&mut encoder);
            }
        }

        if draws_damage_rects {
            let view = surface_view
                .as_ref()
                .expect("a non-empty overlay builds the surface view");
            self.run_overlay_pass(draw, view, &mut encoder);
        }

        if let Some(t) = self.gpu_timings.as_mut() {
            t.resolve(&mut encoder);
        }

        self.staging_belt.finish_and_recall_on_submit(&encoder);
        self.gpu.queue.submit(iter::once(encoder.finish()));

        if let Some(t) = self.gpu_timings.as_mut() {
            t.after_submit(&self.gpu.device, &self.pass_stats);
        }

        let frame = self.text.end_frame();
        self.icon.end_frame(frame);
    }

    /// The belt-routed upload phase of one [`Self::submit`].
    fn upload_frame(&mut self, encoder: &mut wgpu::CommandEncoder, sub: &Submission<'_>) {
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
        let mut ctx = GpuCtx::new(&self.gpu, &mut self.staging_belt, encoder);

        // Texture-only upload (the belt is buffer-only), first so draws see the right pixels.
        self.gradient.upload(&ctx);

        if dim_undamaged {
            self.debug
                .upload_dim(&mut ctx, buffer.display.physical.as_vec2());
        }
        // Damage-rect overlay quads (debug): uploaded now, drawn last after the backbuffer copy.
        if debug_overlay.damage_rect {
            self.debug.upload_damage_rects(&mut ctx, plan, buffer);
        } else {
            self.debug.clear_damage_rects();
        }
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
                idxs: &store.meshes.idxs,
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
            ViewSubmitter {
                owner,
                now: buffer.time,
                text: self.text.shaper(),
            },
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
            for (idx, b) in buffer.text_batches.iter().enumerate() {
                self.text.prepare_batch(TextBatchInput {
                    idx,
                    runs: &buffer.texts[b.texts.range()],
                    scale: buffer.display.scale_factor(),
                    interned_text,
                });
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
            self.icon.prewarm(buffer.display.scale_factor());
            for (i, b) in buffer.batches(PaintTier::Icon).iter().enumerate() {
                self.icon.prepare_batch(i, &buffer.icons[b.items.range()]);
            }
        }
        self.icon.flush(&mut ctx);
    }

    /// Full-viewport pass drawing a translucent black quad over the backbuffer when `dim_undamaged`
    /// is on. No stencil attachment; partial passes set their own.
    fn run_dim_pass(
        &self,
        draw: FrameDraw<'_>,
        color_view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let mut pass = begin_load_pass(encoder, "palantir.renderer.dim.pass", color_view);
        self.debug.draw_dim(&mut pass, self.overlay_draw(draw));
    }

    /// The no-stencil solid quad binding both debug-overlay passes draw through.
    const fn overlay_draw<'a>(&'a self, draw: FrameDraw<'a>) -> OverlayDraw<'a> {
        OverlayDraw {
            binding: QuadBinding {
                pipeline: draw
                    .fmt
                    .quad
                    .color(QuadForm::Solid)
                    .select(StencilMode::Off),
                gradient_bind_group: &self.gradient.bind_group,
            },
            viewport: draw.viewport,
        }
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
        draw: FrameDraw<'_>,
        target: MainPass<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        tracy::zone!();
        let MainPass {
            color_view,
            stencil_view,
            clear,
            scissors,
        } = target;
        debug_assert_eq!(
            stencil_view.is_some(),
            draw.masks.is_some(),
            "the mask plan rides exactly the passes with a stencil attachment",
        );
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
        let load_op = match scissors {
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
            match scissors {
                RepaintScissors::Full => self.render_groups(draw, &mut pass, None),
                RepaintScissors::Partial(rects) => {
                    let rect_count = rects.len();
                    for (i, r) in rects.iter().enumerate() {
                        tracing::trace!(
                            rect = i,
                            of = rect_count,
                            scissor = ?r,
                            "wgpu_backend.submit.pass.partial_rect"
                        );
                        self.render_groups(draw, &mut pass, Some(r));
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

    /// Dispatch each step of the per-frame schedule ([`Schedule::for_each_step`]) to the wgpu
    /// pass; ordering lives in the schedule module so tests assert it without a GPU.
    /// `masks` is `Some` exactly when the pass has a stencil attachment.
    fn render_groups<'a>(
        &'a self,
        draw: FrameDraw<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        damage_scissor: Option<URect>,
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

        /// `viewport.push_into(pass)` follows every (re)bind (the rule `IMMEDIATES_BYTES`
        /// states). [`Self::rebind`] bundles bind, viewport push and recording `bound`;
        /// `PreClear` stays open-coded since it uses its own vertex buffer.
        #[derive(Debug)]
        struct Binder {
            bound: Bound,
            viewport: ViewportPush,
        }

        impl Binder {
            fn rebind<'p>(
                &mut self,
                target: Bound,
                pass: &mut wgpu::RenderPass<'p>,
                bind: impl FnOnce(&mut wgpu::RenderPass<'p>),
            ) {
                if self.bound != target {
                    bind(pass);
                    self.viewport.push_into(pass);
                    self.bound = target;
                }
            }
        }

        tracy::zone!();
        let FrameDraw {
            fmt,
            buffer,
            viewport,
            masks,
        } = draw;
        let images = self.image_store.read();
        let textures = ImageTextures {
            images: &images,
            targets: &self.gpu_view_targets,
        };
        let mut binder = Binder {
            bound: Bound::None,
            viewport,
        };
        let stencil = if masks.is_some() {
            StencilMode::Test
        } else {
            StencilMode::Off
        };
        let raster_pipeline = fmt.raster.select(stencil);
        let quad = |pipeline| QuadBinding {
            pipeline,
            gradient_bind_group: &self.gradient.bind_group,
        };

        let mark = |pass: &mut wgpu::RenderPass<'a>, kind: BatchKind| {
            if let Some(t) = self.gpu_timings.as_ref() {
                t.mark(pass, kind);
            }
        };

        let schedule = Schedule {
            buffer,
            damage_scissor,
            masks,
        };
        schedule.for_each_step(&mut |step| match step {
            RenderStep::PreClear => {
                mark(pass, BatchKind::PreClear);
                debug_marker::push(pass, "preclear");
                // bind, push viewport, then draw; otherwise the clear quad reads stale immediates
                // (zero on a partial pass's first PreClear), skipping the damage-region clear.
                self.quad
                    .bind_clear(pass, quad(fmt.quad.color(QuadForm::Solid).select(stencil)));
                viewport.push_into(pass);
                pass.draw(0..4, 0..1);
                binder.bound = Bound::None;
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
                binder.rebind(Bound::MaskStamp, pass, |pass| {
                    self.quad.bind_mask(pass, quad(&fmt.quad.mask_stamp));
                });
                self.quad.draw_mask(pass, mi);
                debug_marker::pop(pass);
            }
            RenderStep::MaskClear(mi) => {
                mark(pass, BatchKind::Mask);
                debug_marker::push(pass, "mask_clear");
                binder.rebind(Bound::MaskClear, pass, |pass| {
                    self.quad.bind_mask(pass, quad(&fmt.quad.mask_clear));
                });
                self.quad.draw_mask(pass, mi);
                debug_marker::pop(pass);
            }
            RenderStep::Quads { range } => {
                mark(pass, BatchKind::Quads);
                debug_marker::push(pass, "quads");
                for run in self.quad.quad_runs(range) {
                    binder.rebind(Bound::QuadInstance(run.key), pass, |pass| {
                        self.quad
                            .bind(pass, quad(fmt.quad.color(run.key).select(stencil)));
                    });
                    self.quad.draw(pass, run.instances);
                }
                debug_marker::pop(pass);
            }
            RenderStep::Shadows { range } => {
                mark(pass, BatchKind::Shadows);
                debug_marker::push(pass, "shadows");
                for run in self.quad.shadow_runs(range) {
                    binder.rebind(Bound::ShadowInstance(run.key), pass, |pass| {
                        self.quad
                            .bind_shadows(pass, quad(fmt.quad.shadow(run.key).select(stencil)));
                    });
                    self.quad.draw_shadows(pass, run.instances);
                }
                debug_marker::pop(pass);
            }
            RenderStep::Text { batch } => {
                mark(pass, BatchKind::Text);
                debug_marker::push(pass, "text");
                binder.rebind(Bound::Raster, pass, |pass| {
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
                        binder.rebind(Bound::Mesh, pass, |pass| {
                            self.mesh.bind(pass, fmt.mesh.select(stencil));
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
                        binder.rebind(Bound::Image, pass, |pass| {
                            self.image.bind(pass, fmt.image.select(stencil));
                        });
                        self.image.draw(
                            pass,
                            ImageBatch {
                                ids: buffer.images.id(),
                                items: items(),
                            },
                            textures,
                        );
                    }
                    PaintTier::Icon => {
                        binder.rebind(Bound::Raster, pass, |pass| {
                            pass.set_pipeline(raster_pipeline);
                        });
                        self.icon.render_batch(batch, pass);
                    }
                    PaintTier::Curve => {
                        binder.rebind(Bound::Curve, pass, |pass| {
                            self.curve.bind(
                                pass,
                                fmt.curve.select(stencil),
                                &self.gradient.bind_group,
                            );
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
        draw: FrameDraw<'_>,
        surface_view: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let mut pass = begin_load_pass(
            encoder,
            "palantir.renderer.overlay.damage_rect",
            surface_view,
        );
        self.debug
            .draw_damage_rects(&mut pass, self.overlay_draw(draw));
    }

    pub(crate) const fn cutout_mode(&self) -> CutoutMode {
        self.quad.cutout_mode()
    }

    pub(crate) const fn device(&self) -> &wgpu::Device {
        &self.gpu.device
    }

    /// The device and queue every window of this backend presents through.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "only the windowed host presents through the device itself"
        )
    )]
    pub(crate) const fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    pub(crate) const fn texture_binding(&self) -> &TextureBinding {
        &self.texture_binding
    }

    /// The window's backbuffer for `target`, built if `slot` is empty or no longer
    /// [`describes`](Backbuffer::describes) it.
    pub(crate) fn ensure_backbuffer<'s>(
        &self,
        slot: &'s mut Option<Backbuffer>,
        target: RenderTarget<'_>,
    ) -> EnsuredBackbuffer<'s> {
        if slot.as_ref().is_some_and(|held| !held.describes(target)) {
            *slot = None;
        }
        let recreated = slot.is_none();
        EnsuredBackbuffer {
            backbuffer: slot.get_or_insert_with(|| {
                Backbuffer::new(self.device(), self.texture_binding(), target)
            }),
            recreated,
        }
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
        let surface_texture = target.texture();
        debug_assert!(
            backbuffer.describes(target),
            "skip-copy backbuffer doesn't match the target — a Skip frame \
             implies the previous frame painted this size/format"
        );
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.skip"),
            });
        if target.takes_copy() {
            backbuffer.copy_onto(&mut encoder, surface_texture);
        } else {
            let fmt = self
                .pipelines
                .get(&target.format())
                .expect("a skip implies a prior submit built this format's pipelines");
            let view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());
            backbuffer.draw_onto(&mut encoder, &view, &fmt.blit);
        }
        self.gpu.queue.submit(iter::once(encoder.finish()));
    }

    /// Start copying `region` of `backbuffer`, which the last submit painted, for a screenshot.
    pub(crate) fn capture(&mut self, backbuffer: &Backbuffer, region: URect) -> Capture {
        let blit = self
            .capture_blit
            .get_or_insert_with(|| self.blit.build(&self.gpu.device, Capture::FORMAT));
        Capture::start(&self.gpu.device, &self.gpu.queue, blit, backbuffer, region)
    }

    /// Run the map callbacks of the copies the device has finished, without waiting for the rest.
    pub(crate) fn poll(&self) {
        self.gpu.device.poll(wgpu::PollType::Poll).ok();
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

#[cfg(feature = "internals")]
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
