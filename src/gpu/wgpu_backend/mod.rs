//! The wgpu backend: the one GPU renderer, the render stream it opens for each window, and the
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
//! A backdrop pane that captures breaks the main pass at its barrier: the pass ends, the pane's
//! footprint is copied out of the target and blurred, and the next pass loads the target. On a
//! Partial frame the footprint joins the scissors, as the target there holds the last frame's
//! pane.
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
//! The frame's timings close just before `encoder.finish()` ([`GpuTimings::finish`]); `after_submit`
//! publishes any earlier frame that landed.

use crate::common::tracy;
use crate::diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats};
use crate::gpu::backdrop::backdrops::{BackdropFrame, Backdrops};
use crate::gpu::device::backend_resources::BackendResources;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_ctx::GpuCtx;
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::frame::debug_marker;
use crate::gpu::frame::debug_overlay::{DebugOverlay, OverlayDraw};
use crate::gpu::frame::gpu_timings::{BackdropPhase, GpuTimings, Mark};
use crate::gpu::frame::main_segment::MainSegment;
use crate::gpu::frame::schedule::{MaskPlan, RenderStep, Schedule};
use crate::gpu::frame::submission::{Route, Submission};
use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::format_pipelines::FormatPipelines;
use crate::gpu::pipeline::format_pipelines::PipelineSources;
use crate::gpu::pipeline::image_pipeline::{ImageBatch, ImagePipeline, ImageTextures};
use crate::gpu::pipeline::mesh_pipeline::{MeshBatch, MeshPipeline, MeshUpload};
use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutPlan;
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::pipeline::quad_pipeline::{QuadBinding, QuadPipeline};
use crate::gpu::pipeline::stencil_variant::StencilMode;
use crate::gpu::pipeline::viewport_push::ViewportPush;
use crate::gpu::raster::icon_backend::IconBackend;
use crate::gpu::raster::raster_program::{RasterPipelines, RasterProgram};
use crate::gpu::raster::text_backend::{TextBackend, TextBatchInput};
use crate::gpu::resource::gpu_gradient_atlas::GpuGradientAtlas;
use crate::gpu::resource::gpu_view_targets::{GpuViewTargets, ViewSubmitter};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::capture::{Capture, CaptureSource};
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::surface::stencil::Stencil;
use crate::gpu::wgpu_backend::render_stream::RenderStream;
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::repaint_scissors::RepaintScissors;
use rustc_hash::FxHashMap;
use std::iter;
use std::rc::Rc;
use std::time::Instant;
use wgpu::util::StagingBelt;

pub(crate) mod render_stream;

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
    /// The frame's backdrop panes: their captures, blurs and results.
    backdrops: Backdrops,
    mesh: MeshPipeline,
    image: ImagePipeline,
    /// The registered images; host recorders create, rewrite and free them through the
    /// `ImageRegistry` the host built over this `Rc`.
    image_store: Rc<WgpuImageStore>,
    gpu_view_targets: GpuViewTargets,
    /// The one shader and group-0 layout both raster tenants draw through; see [`RasterProgram`].
    raster: RasterProgram,
    blit: BlitPipeline,
    /// The blit into [`Capture::FORMAT`], built on the first screenshot of a backbuffer that is
    /// not 8-bit sRGB.
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
    /// Render pipelines keyed by swapchain color format, built by [`Self::prepare_format`] as a
    /// window notes a target. The surface texture handed to `submit` selects the set.
    pipelines: FxHashMap<TargetFormat, FormatPipelines>,
    /// What timing this device can do; `Some` when the host opted in, and a device without
    /// timestamps then times frames on the CPU clock ([`GpuTimings`]).
    timing: Option<TimingSupport>,
}

/// Whether the backend times the frames it submits, which a host opts into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameTiming {
    Off,
    On,
}

/// The device's timestamp tick and the timing features it offers.
#[derive(Clone, Copy, Debug)]
struct TimingSupport {
    period_ns: f32,
    features: wgpu::Features,
}

/// What every pass of one submit draws with, fixed for the frame.
#[derive(Clone, Copy, Debug)]
struct FrameDraw<'a> {
    fmt: &'a FormatPipelines,
    buffer: &'a RenderBuffer,
    viewport: ViewportPush,
    /// The frame's mask chains; `Some` exactly when the main pass has a stencil attachment.
    masks: Option<&'a MaskPlan>,
    /// The frame's shadow cutouts, which pick each shadow's pipeline.
    cutouts: &'a CutoutPlan,
    /// The stream's timestamp queries, when timing is on.
    timings: Option<&'a GpuTimings>,
    /// Where the owner's samples publish. Record time is unconditional (two `Instant::now()`
    /// calls and a `RefCell` write); making it opt-in would force the in-pass timestamp writes
    /// that perturb the number.
    stats: &'a GpuPassStats,
}

/// What [`WgpuBackend::run_main_pass`] draws into: color, optional stencil, the clear color and the
/// scissors it repaints, picked together per frame.
#[derive(Clone, Copy, Debug)]
struct MainPass<'a> {
    color_view: &'a wgpu::TextureView,
    /// The texture `color_view` views, which a pane's capture copies out of.
    color: &'a wgpu::Texture,
    stencil_view: Option<&'a wgpu::TextureView>,
    clear: wgpu::Color,
    scissors: &'a RepaintScissors,
}

/// What [`WgpuBackend::upload_frame`] uploads for: the submission, its stream, the target's
/// format, and what [`WgpuBackend::submit`] worked out of them once.
#[derive(Debug)]
struct FrameUpload<'s, 'a> {
    submission: &'s Submission<'a>,
    stream: &'s mut RenderStream,
    format: TargetFormat,
    clear: RgbaF16,
    dims_undamaged: bool,
}

impl WgpuBackend {
    /// Build the one shared GPU renderer over the host's handles. Per-format pipelines build when
    /// a window first notes a target of the format ([`Self::prepare_format`]).
    pub(crate) fn new(gpu: Gpu, resources: BackendResources<'_>, timing: FrameTiming) -> Self {
        let device = &gpu.device;
        let image_store = Rc::clone(resources.image_store);
        let texture_binding = image_store.binding().clone();
        let gradient = GpuGradientAtlas::new(
            device,
            resources.gradient_atlas.clone(),
            texture_binding.clone(),
        );
        let backdrops = Backdrops::new(device);
        let quad = QuadPipeline::new(device, &texture_binding, backdrops.layout());
        let mesh = MeshPipeline::new(device);
        let image = ImagePipeline::new(device, &texture_binding);
        let gpu_view_targets = GpuViewTargets::new(texture_binding.clone());
        let curve = CurvePipeline::new(device, &texture_binding);
        let raster = RasterProgram::new(device);
        let blit = BlitPipeline::new(device, &texture_binding);
        let text = TextBackend::new(device, &raster, resources.text.clone());
        let icon = IconBackend::new(device, &raster, resources.icons.clone());
        let debug = DebugOverlay::new(device);
        // 1 MiB chunks, above the resize-drag upload peak, so steady state uses 1-2 chunks.
        let staging_belt = StagingBelt::new(device.clone(), 1 << 20);
        let timestamp_period = gpu.queue.get_timestamp_period();
        // A device whose timestamps have no period times frames on the CPU clock.
        let timing = (timing == FrameTiming::On).then(|| TimingSupport {
            period_ns: timestamp_period,
            features: if timestamp_period > 0.0 {
                device.features()
            } else {
                wgpu::Features::empty()
            },
        });
        Self {
            gpu,
            staging_belt,
            gradient,
            quad,
            backdrops,
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
            pipelines: FxHashMap::default(),
            timing,
        }
    }

    /// A stream for `owner`'s frames: its cutouts take the device's mode, and its frames are
    /// timed where the backend times frames.
    pub(crate) fn open_stream(&self, owner: RenderOwnerId) -> RenderStream {
        let timings = self
            .timing
            .map(|support| GpuTimings::new(&self.gpu.device, support.period_ns, support.features));
        RenderStream::new(owner, self.quad.cutout_mode(), timings)
    }

    /// Build the pipeline set for `format`, where none is built, as a window notes a target of
    /// it: before the frame that paints it, so its first submit compiles nothing. Only
    /// `wgpu::RenderPipeline`s carry the color-target format, so a new format costs a few
    /// pipeline compiles and no image re-upload or glyph re-rasterization.
    pub(crate) fn prepare_format(&mut self, format: TargetFormat) {
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

    /// Render one frame of `stream` into `target`. Skip frames never reach this method.
    ///
    /// [`Submission::route`] picks the path: [`Route::ViaBackbuffer`] renders into the stream's
    /// backbuffer and copies onto `target`, whose texture needs `COPY_DST` or draws it on;
    /// [`Route::Direct`] renders straight into `target`. The scissors are the effective plan:
    /// escalations were sealed before the draw list was built.
    pub(crate) fn submit(
        &mut self,
        stream: &mut RenderStream,
        target: RenderTarget<'_>,
        submission: Submission<'_>,
    ) {
        tracy::zone!();
        let Submission {
            stats,
            buffer,
            scissors: repaint_scissors,
            route,
            ..
        } = submission;
        let via_backbuffer = route == Route::ViaBackbuffer;
        if via_backbuffer {
            let made = stream.ensure_backbuffer(&self.gpu.device, &self.texture_binding, target);
            // A partial arrives un-escalated only onto a fresh backbuffer, so a new one means that
            // broke; the list was already culled for the partial, so escalating cannot fix it.
            debug_assert!(
                !made || !repaint_scissors.is_partial(),
                "backbuffer made under a partial frame whose draw list was culled for it"
            );
        }
        stream.backbuffer_fresh = via_backbuffer;
        let stencil = !buffer.rounded_clips.is_empty();
        if stencil {
            stream.ensure_stencil(&self.gpu.device, target.size());
        }
        let clear = submission.clear();
        let dims_undamaged = submission.dims_undamaged();
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.display.physical,
            scissors = ?repaint_scissors,
            ?route,
            rounded_clip = stencil,
            "wgpu_backend.submit"
        );

        let surface_texture = target.texture();
        let format = target.format();
        let draws_damage_rects = submission.overlay.damage.is_some();
        // At most one surface view, only where a pass reads it; the normal backbuffer path avoids a
        // view per frame.
        let surface_view = (!via_backbuffer || draws_damage_rects || !target.takes_copy())
            .then(|| surface_texture.create_view(&wgpu::TextureViewDescriptor::default()));

        // One encoder: the belt's copies must land before the passes that read them.
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.main"),
            });

        self.upload_frame(
            &mut encoder,
            FrameUpload {
                submission: &submission,
                stream: &mut *stream,
                format,
                clear,
                dims_undamaged,
            },
        );

        let clear = RgbaF32::from(clear);
        let clear_color = wgpu::Color {
            r: f64::from(clear.r),
            g: f64::from(clear.g),
            b: f64::from(clear.b),
            a: f64::from(clear.a),
        };
        let backbuffer = via_backbuffer.then(|| {
            stream
                .backbuffer
                .as_ref()
                .expect("a frame through the backbuffer made it")
        });
        let stencil_view = stream
            .stencil
            .as_ref()
            .filter(|_| stencil)
            .map(Stencil::view);
        let draw = FrameDraw {
            fmt: self
                .pipelines
                .get(&format)
                .expect("a window notes its target's format before it records"),
            buffer,
            viewport: ViewportPush::for_buffer(buffer),
            masks: stencil.then_some(&self.mask_plan),
            cutouts: stream.cutouts(),
            timings: stream.timings.as_ref(),
            stats,
        };
        let color_view: &wgpu::TextureView = match backbuffer {
            Some(bb) => bb.view(),
            None => surface_view
                .as_ref()
                .expect("direct present builds the surface view"),
        };
        if dims_undamaged {
            tracing::trace!("wgpu_backend.submit.pass.dim");
            self.run_dim_pass(draw, color_view, &mut encoder);
        }
        self.run_main_pass(
            draw,
            MainPass {
                color_view,
                color: backbuffer.map_or(surface_texture, Backbuffer::texture),
                stencil_view,
                clear: clear_color,
                scissors: repaint_scissors,
            },
            &mut encoder,
        );

        if let Some(bb) = backbuffer {
            if let Some(t) = draw.timings {
                t.encoder_mark(&mut encoder, Mark::CopyOutStart);
            }
            if target.takes_copy() {
                bb.copy_onto(&mut encoder, surface_texture);
            } else {
                let view = surface_view
                    .as_ref()
                    .expect("a target that takes no copy builds the surface view");
                bb.draw_onto(&mut encoder, view, &draw.fmt.blit);
            }
            if let Some(t) = draw.timings {
                t.encoder_mark(&mut encoder, Mark::CopyOutEnd);
            }
        }

        if draws_damage_rects {
            let view = surface_view
                .as_ref()
                .expect("a non-empty overlay builds the surface view");
            self.run_overlay_pass(draw, view, &mut encoder);
        }

        if let Some(t) = &mut stream.timings {
            t.finish(&mut encoder);
        }
        self.staging_belt.finish_and_recall_on_submit(&encoder);
        self.gpu.queue.submit(iter::once(encoder.finish()));
        if let Some(t) = &mut stream.timings {
            t.after_submit(&self.gpu, stats);
        }

        let frame = self.text.end_frame();
        self.icon.end_frame(frame);
    }

    /// The belt-routed upload phase of one [`Self::submit`].
    fn upload_frame(&mut self, encoder: &mut wgpu::CommandEncoder, upload: FrameUpload<'_, '_>) {
        let FrameUpload {
            submission,
            stream,
            format,
            clear,
            dims_undamaged,
        } = upload;
        let Submission {
            store,
            buffer,
            scissors,
            route,
            overlay,
            ..
        } = *submission;
        let owner = stream.owner();
        let mut ctx = GpuCtx::new(&self.gpu, &mut self.staging_belt, encoder);

        self.gradient.upload(&mut ctx);

        if dims_undamaged {
            self.debug
                .upload_dim(&mut ctx, buffer.display.physical.as_vec2());
        }
        // Damage-rect overlay quads (debug): uploaded now, drawn last after the backbuffer copy.
        if let Some(damage) = overlay.damage {
            self.debug.upload_damage_rects(&mut ctx, damage, buffer);
        } else {
            self.debug.clear_damage_rects();
        }
        if !buffer.rounded_clips.is_empty() {
            tracy::zone!("stage_masks");
            self.mask_plan.build(buffer);
            self.quad.upload_masks(&mut ctx, self.mask_plan.quads());
        }

        self.quad.upload(&mut ctx, &buffer.quads, stream.cutouts());
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

        if scissors.is_partial() {
            self.quad
                .upload_clear(&mut ctx, buffer.display.physical.as_vec2(), clear);
        }
        // Every frame, so a pane that left frees its cached result.
        self.backdrops.prepare(
            &mut ctx,
            BackdropFrame {
                buffer,
                owner,
                format,
                clear,
                backbuffer: stream
                    .backbuffer
                    .as_mut()
                    .filter(|_| route == Route::ViaBackbuffer),
            },
        );

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

        {
            tracy::zone!(
                "icon.prepare_batches",
                value = buffer.batches(PaintTier::Icon).len() as u64
            );
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

    /// The main pass, broken at the barrier of each pane that captures: the pass ends, the pane's
    /// footprint is copied out of the target into its capture and blurred, and the next pass
    /// loads the target and paints on. Each pass walks the schedule once per repaint scissor
    /// (once, unscissored, on Full), and only the first clears; the scissors are disjoint
    /// ([`RepaintScissors::new`]), as a later pass paints over what the one before it left.
    ///
    /// Every walk leaves the stencil clean: one ending with a mask stamped emits a tail
    /// clear under the stamp's scissor, so no walk reads another's stencil writes.
    ///
    /// Host CPU time for all of it, including each pass's end-of-pass replay on drop, publishes
    /// to [`GpuPassStats::last_main_pass_cpu`]; it scales with draw-step count and is what the
    /// `record_pass` benchmark reads.
    fn run_main_pass(
        &self,
        draw: FrameDraw<'_>,
        target: MainPass<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        tracy::zone!();
        debug_assert_eq!(
            target.stencil_view.is_some(),
            draw.masks.is_some(),
            "the mask plan rides exactly the passes with a stencil attachment",
        );
        let started = Instant::now();
        let breaks = self.backdrops.breaks();
        let mut from = 0;
        for (i, at) in breaks.iter().enumerate() {
            let segment = if i == 0 {
                MainSegment::First
            } else {
                MainSegment::Inner
            };
            self.run_main_segment(draw, target, segment, (from, Some(at.barrier)), encoder);
            from = at.barrier;
            backdrop_mark(draw, encoder, i, BackdropPhase::CaptureStart);
            self.backdrops.copy_break(encoder, at, target.color);
            backdrop_mark(draw, encoder, i, BackdropPhase::BlurStart);
            self.backdrops.blur_break(encoder, at);
            backdrop_mark(draw, encoder, i, BackdropPhase::BlurEnd);
        }
        let segment = if breaks.is_empty() {
            MainSegment::Whole
        } else {
            MainSegment::Last
        };
        self.run_main_segment(draw, target, segment, (from, None), encoder);
        draw.stats
            .record_main_pass_cpu_ns(started.elapsed().as_nanos() as u64);
    }

    /// One pass of the main pass: the groups from `from` to `barrier` (to the last with `None`),
    /// once per damage rect. Timestamps open with the first pass and close with the last, the
    /// breaks between count in no category, and each pass adds its pipeline statistics.
    fn run_main_segment(
        &self,
        draw: FrameDraw<'_>,
        target: MainPass<'_>,
        segment: MainSegment,
        (from, barrier): (u32, Option<u32>),
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let MainPass {
            color_view,
            color: _,
            stencil_view,
            clear,
            scissors,
        } = target;
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
            RepaintScissors::Full if segment.opens() => wgpu::LoadOp::Clear(clear),
            _ => wgpu::LoadOp::Load,
        };
        // The descriptor covers basic mode; with `TIMESTAMP_QUERY_INSIDE_PASSES` we write inline
        // via `segment_begin` / `segment_end` for one gap-free stream.
        let timestamp_writes = draw.timings.and_then(|t| t.pass_writes(segment));
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
        if let Some(t) = draw.timings {
            t.segment_begin(&mut pass, segment);
        }
        // One for the pass: what a scissor replay leaves bound stays bound for the next.
        let mut binder = MainBinder::new(draw.viewport);
        match scissors {
            RepaintScissors::Full => {
                self.render_groups(draw, &mut binder, &mut pass, None, from, barrier);
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
                    self.render_groups(draw, &mut binder, &mut pass, Some(r), from, barrier);
                }
            }
        }
        if let Some(t) = draw.timings {
            t.segment_end(&mut pass, segment);
        }
    }

    /// Dispatch each step of the per-frame schedule ([`Schedule::for_each_step`]) to the wgpu
    /// pass; ordering lives in the schedule module so tests assert it without a GPU.
    /// `masks` is `Some` exactly when the pass has a stencil attachment.
    fn render_groups<'a>(
        &'a self,
        draw: FrameDraw<'a>,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        damage_scissor: Option<URect>,
        from: u32,
        barrier: Option<u32>,
    ) {
        tracy::zone!();
        let FrameDraw {
            fmt,
            buffer,
            viewport: _,
            masks,
            cutouts,
            timings: _,
            stats: _,
        } = draw;
        let images = self.image_store.read();
        let textures = ImageTextures {
            images: &images,
            targets: &self.gpu_view_targets,
        };
        let stencil = if masks.is_some() {
            StencilMode::Test
        } else {
            StencilMode::Off
        };
        let raster = RasterPipelines {
            flat: fmt.raster.flat.select(stencil),
            spun: fmt.raster.spun.select(stencil),
            layout: self.raster.pipeline_layout(),
        };
        let quad = |pipeline| QuadBinding {
            pipeline,
            gradient_bind_group: &self.gradient.bind_group,
        };

        let mark = |pass: &mut wgpu::RenderPass<'a>, kind: BatchKind| {
            if let Some(t) = draw.timings {
                t.mark(pass, kind);
            }
        };

        let schedule = Schedule {
            buffer,
            damage_scissor,
            masks,
            from,
            barrier,
        };
        schedule.for_each_step(&mut |step| match step {
            RenderStep::PreClear => {
                mark(pass, BatchKind::PreClear);
                debug_marker::push(pass, "preclear");
                // Through the binder, so the viewport is pushed before the draw; else the clear quad
                // reads stale immediates (zero on a partial pass's first PreClear) and clears nothing.
                self.quad.bind_clear(
                    binder,
                    pass,
                    quad(fmt.quad.color(QuadForm::Solid).select(stencil)),
                );
                pass.draw(0..4, 0..1);
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
                self.quad
                    .bind_mask(binder, pass, quad(&fmt.quad.mask_stamp));
                self.quad.draw_mask(pass, mi);
                debug_marker::pop(pass);
            }
            RenderStep::MaskClear(mi) => {
                mark(pass, BatchKind::Mask);
                debug_marker::push(pass, "mask_clear");
                self.quad
                    .bind_mask(binder, pass, quad(&fmt.quad.mask_clear));
                self.quad.draw_mask(pass, mi);
                debug_marker::pop(pass);
            }
            RenderStep::Quads { range } => {
                mark(pass, BatchKind::Quads);
                debug_marker::push(pass, "quads");
                for run in QuadPipeline::quad_runs(&buffer.quads, range) {
                    let binding = quad(fmt.quad.color(run.key).select(stencil));
                    if run.key == QuadForm::Backdrop {
                        self.quad
                            .bind_backdrop(binder, pass, binding, self.backdrops.group());
                    } else {
                        self.quad.bind(binder, pass, binding);
                    }
                    self.quad.draw(pass, run.instances);
                }
                debug_marker::pop(pass);
            }
            RenderStep::Shadows { range } => {
                mark(pass, BatchKind::Shadows);
                debug_marker::push(pass, "shadows");
                for run in self.quad.shadow_runs(cutouts, range) {
                    self.quad.bind_shadows(
                        binder,
                        pass,
                        quad(fmt.quad.shadow(run.key).select(stencil)),
                    );
                    self.quad.draw_shadows(pass, run.instances);
                }
                debug_marker::pop(pass);
            }
            RenderStep::Text { batch } => {
                mark(pass, BatchKind::Text);
                debug_marker::push(pass, "text");
                self.text.render_batch(binder, pass, batch, raster);
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
                        self.mesh.bind(binder, pass, fmt.mesh.select(stencil));
                        self.mesh.draw(
                            pass,
                            MeshBatch {
                                draws: buffer.meshes.draw(),
                                items: items(),
                            },
                        );
                    }
                    PaintTier::Image => {
                        self.image.bind(binder, pass, fmt.image.select(stencil));
                        self.image.draw(
                            binder,
                            pass,
                            ImageBatch {
                                ids: buffer.images.id(),
                                items: items(),
                            },
                            textures,
                        );
                    }
                    PaintTier::Icon => {
                        self.icon.render_batch(binder, pass, batch, raster);
                    }
                    PaintTier::Curve => {
                        self.curve.bind(
                            binder,
                            pass,
                            fmt.curve.select(stencil),
                            &self.gradient.bind_group,
                        );
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

    /// The device and queue every window of this backend presents through.
    pub(crate) const fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// Skip path: the target still needs valid pixels. A `Skip` requires the previous
    /// frame submitted at this size and format (`take_frame_plan` forces `Full`
    /// otherwise), so a missing or mismatched backbuffer crashes rather than present
    /// undefined pixels.
    pub(crate) fn copy_backbuffer_to_surface(
        &self,
        stream: &RenderStream,
        target: RenderTarget<'_>,
    ) {
        let backbuffer = stream
            .backbuffer
            .as_ref()
            .expect("a skip follows a frame painted through the backbuffer");
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
                .expect("a window notes its target's format before it records");
            let view = surface_texture.create_view(&wgpu::TextureViewDescriptor::default());
            backbuffer.draw_onto(&mut encoder, &view, &fmt.blit);
        }
        self.gpu.queue.submit(iter::once(encoder.finish()));
    }

    /// Start copying `region` of `stream`'s backbuffer, which its last submit painted, for a
    /// screenshot.
    pub(crate) fn capture(&mut self, stream: &RenderStream, region: URect) -> Capture {
        let backbuffer = stream
            .backbuffer
            .as_ref()
            .expect("a frame with a screenshot request paints through the backbuffer");
        let blit = Capture::needs_blit(backbuffer.texture().format()).then(|| {
            &*self
                .capture_blit
                .get_or_insert_with(|| self.blit.build(&self.gpu.device, Capture::FORMAT))
        });
        let source = CaptureSource {
            backbuffer,
            blit,
            binding: &self.texture_binding,
        };
        Capture::start(&self.gpu.device, &self.gpu.queue, source, region)
    }

    /// Run the map callbacks of the copies the device has finished, without waiting for the rest.
    pub(crate) fn poll(&self) {
        self.gpu.device.poll(wgpu::PollType::Poll).ok();
    }

    /// Hand `owner`'s pane ledger, through `forget`, each pane whose backdrop result the shared
    /// atlas evicted, before its next frame decides what its panes hold.
    pub(crate) fn drain_evicted_panes(
        &mut self,
        owner: RenderOwnerId,
        mut forget: impl FnMut(PaneKey),
    ) {
        self.backdrops.evicted(owner).for_each(&mut forget);
        self.backdrops.forget_evicted(owner);
    }

    /// Release what the backend keeps for a retired render stream's owner, its `GpuView` targets
    /// and pane results, as its window closes; the stream itself drops with the window.
    /// Per-submit eviction is owner-scoped, so without this survivors would hold its textures
    /// until shutdown.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "not `cfg`: the winit host is only the current caller, and an embedding host needs this entry point too"
        )
    )]
    pub(crate) fn retire_render_owner(&mut self, owner: RenderOwnerId) {
        self.gpu_view_targets.retire_owner(owner);
        self.backdrops.retire(owner);
    }
}

/// Timestamp backdrop break `at`'s `phase`, where the frame is timed.
fn backdrop_mark(
    draw: FrameDraw<'_>,
    encoder: &mut wgpu::CommandEncoder,
    at: usize,
    phase: BackdropPhase,
) {
    if let Some(t) = draw.timings {
        t.encoder_mark(encoder, Mark::Backdrop { at, phase });
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
        /// grid is compared with. Builds every format's pipelines again.
        pub(crate) fn disable_shadow_grid(&mut self) {
            self.quad.disable_shadow_grid();
            let formats: Vec<TargetFormat> = self.pipelines.keys().copied().collect();
            self.pipelines.clear();
            for format in formats {
                self.prepare_format(format);
            }
        }

        pub(crate) fn has_format_pipelines(&self, format: TargetFormat) -> bool {
            self.pipelines.contains_key(&format)
        }

        pub(crate) fn gpu_image_cache_len(&self) -> usize {
            self.image_store.resident()
        }

        /// The blurs the last frame ran, one per pane whose cached result went stale.
        pub(crate) const fn backdrop_blurs(&self) -> usize {
            self.backdrops.blurs()
        }
    }
}
