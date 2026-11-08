//! The wgpu backend: the one GPU renderer, the render stream it opens for each window, and the
//! pipeline sets it builds per swapchain format.
//!
//! # One frame
//!
//! [`WgpuBackend::submit`] runs two halves that cannot overlap: an upload phase
//! (`&mut self`) recording every texture and dynamic-buffer write, then the render
//! passes, which read `self.pipelines` through a shared borrow. The passes run as
//! [`FramePasses`] plans them, and every step of a main pass records through one `DrawCtx`.
//!
//! Each group's quads draw first, then its text, so a child quad declared after a
//! label occludes it.
//!
//! # Damage
//!
//! - [`Damage::Full`](crate::damage::Damage::Full): one `LoadOp::Clear(clear)` pass
//!   paints every group at its native scissor.
//! - [`Damage::Partial(region)`](crate::damage::Damage::Partial): one render pass that
//!   `LoadOp::Load`s the backbuffer and replays the groups once per rect, each group's
//!   scissor narrowed to the rect (scaled, padded for AA bleed, clamped; zero-area rects
//!   dropped). On a device whose pass loads and stores its whole target
//!   ([`PassTraffic::WholeTarget`]), the pass instead clears a [`DamageCanvas`] of the rects'
//!   bounds under the surface's viewport moved onto it, and each rect is copied into the
//!   backbuffer.
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
//! routes through `staging`, which records its copies on that encoder instead of
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
use crate::diagnostics::gpu_pass_stats::BatchKind;
use crate::gpu::backdrop::backdrops::{BackdropFrame, Backdrops};
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::pass_traffic::PassTraffic;
use crate::gpu::device::upload_staging::UploadStaging;
use crate::gpu::frame::debug_overlay::{self, DebugOverlay, OverlayDraw};
use crate::gpu::frame::frame_passes::{ColorLoad, FramePasses, FrameShape, PassStep, TargetKind};
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::frame::gpu_timings::{BackdropPhase, GpuTimings, Mark};
use crate::gpu::frame::main_segment::MainSegment;
use crate::gpu::frame::schedule::{GroupSpan, MaskPlan, RenderStep, Schedule};
use crate::gpu::frame::submission::{Route, Submission};
use crate::gpu::pipeline::blit_pipeline::BlitPipeline;
use crate::gpu::pipeline::curve_pipeline::CurvePipeline;
use crate::gpu::pipeline::effect_pipeline::{EffectBatch, EffectPipeline, EffectUpload};
use crate::gpu::pipeline::format_pipelines::{
    FormatPipelines, PassPipelines, PipelineCensus, Programs,
};
use crate::gpu::pipeline::image_pipeline::{ImageBatch, ImagePipeline, ImageTextures};
use crate::gpu::pipeline::mesh_pipeline::{MeshBatch, MeshPipeline, MeshUpload};
use crate::gpu::pipeline::pass_recorder::PassRecorder;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacements;
use crate::gpu::pipeline::quad_pipeline::quad_draw::QuadDraw;
use crate::gpu::pipeline::quad_pipeline::quad_form::QuadForm;
use crate::gpu::pipeline::quad_pipeline::{
    self, QuadBinding, QuadExtras, QuadPass, QuadPipeline, QuadRun,
};
use crate::gpu::pipeline::raster_pipeline::RasterProgram;
use crate::gpu::pipeline::stencil_variant::StencilMode;
use crate::gpu::pipeline::viewport_push::ViewportPush;
use crate::gpu::raster::icon_backend::IconBackend;
use crate::gpu::raster::text_backend::{TextBackend, TextBatchInput};
use crate::gpu::render_owner_id::RenderOwnerId;
use crate::gpu::resource::gpu_gradient_atlas::GpuGradientAtlas;
use crate::gpu::resource::gpu_view_targets::{GpuViewTargets, ViewSubmitter};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_effect_store::WgpuEffectStore;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::capture::{Capture, CaptureBlit, CaptureSource, TexelOrder};
use crate::gpu::surface::damage_canvas::DamageCanvas;
use crate::gpu::surface::render_target::{RenderTarget, TargetFormat};
use crate::gpu::wgpu_backend::backend_resources::BackendResources;
use crate::gpu::wgpu_backend::display_frames::DisplayFrames;
use crate::gpu::wgpu_backend::render_stream::{RenderStream, StreamPasses};
use crate::primitives::geometry::urect::URect;
use crate::primitives::identity::pane_key::PaneKey;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_buffer::paint_tier::PaintTier;
use crate::renderer::repaint_scissors::{PartialScissors, RepaintScissors};
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::iter;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(crate) mod backend_resources;
pub(crate) mod display_frames;
pub(crate) mod render_stream;

/// Wgpu renderer owning its device/queue handles, pipelines and text backend.
/// The text side holds the same [`TextShaper`](crate::text::shaper::TextShaper) the
/// `Ui` measures against, so measurement and rasterization share one buffer cache.
#[derive(Debug)]
pub(crate) struct WgpuBackend {
    gpu: Gpu,
    /// Routes per-frame dynamic-buffer uploads onto the main encoder, collapsing N
    /// `queue.write_buffer` calls (each a Metal blit encoder) into one.
    staging: UploadStaging,
    /// The display frame every submit's per-frame scratch ages against.
    frames: DisplayFrames,
    /// Shared gradient LUT atlas lent to the quad and curve pipelines.
    gradient: GpuGradientAtlas,
    programs: Programs,
    /// The frame's backdrop panes: their captures, blurs and results.
    backdrops: Backdrops,
    /// The registered images; host recorders create, rewrite and free them through the
    /// `ImageRegistry` the host built over this `Rc`.
    image_store: Rc<WgpuImageStore>,
    /// The loaded effects' pipelines; the host's `EffectRegistry` loads through this `Rc`.
    effect_store: Rc<WgpuEffectStore>,
    gpu_view_targets: GpuViewTargets,
    blit: BlitPipeline,
    icon: IconBackend,
    text: TextBackend,
    debug: DebugOverlay,
    /// The group-0 layout and sampler every sampled texture binds through, so a group built for one
    /// binds in every pipeline that samples any.
    texture_binding: TextureBinding,
    /// Rounded-clip chains and mask quads, rebuilt on stencil frames only; retained to avoid
    /// allocation.
    mask_plan: MaskPlan,
    /// The passes of the frame being submitted, planned before any is recorded.
    passes: FramePasses,
    /// Render pipelines keyed by swapchain color format, built by [`Self::prepare_format`] as a
    /// window notes a target. The surface texture handed to `submit` selects the set.
    pipelines: FxHashMap<TargetFormat, FormatPipelines>,
    /// What timing this device can do; `Some` when the host opted in, and a device without
    /// timestamps then times frames on the CPU clock ([`GpuTimings`]).
    timing: Option<TimingSupport>,
}

/// Whether the backend times the frames it submits, which a host opts into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum FrameTiming {
    #[default]
    Off,
    On,
}

impl FrameTiming {
    /// [`Self::On`] when a host's builder asks it to collect GPU stats.
    pub(crate) const fn new(collect_gpu_stats: bool) -> Self {
        if collect_gpu_stats {
            Self::On
        } else {
            Self::Off
        }
    }
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
    /// The blit into the target's format, which presents the backbuffer.
    blit: &'a wgpu::RenderPipeline,
    buffer: &'a RenderBuffer,
    /// The surface's, which every pass but a main pass on a damage canvas pushes as it is.
    viewport: ViewportPush,
    /// The surface pixel the main pass's target pixel 0 is: zero, but on a damage canvas, where
    /// the viewport and every scissor move by it.
    origin: UVec2,
    /// The frame's mask chains; `Some` exactly when the main pass has a stencil attachment.
    masks: Option<&'a MaskPlan>,
    /// The frame's backdrop results, which its panes read; `None` before the first pane.
    backdrops: Option<&'a wgpu::BindGroup>,
}

/// Where a frame's main pass paints, with what it paints through, built once per frame: the
/// plan derives from its kind ([`FrameShape::target`]), and each step reads its own resources.
/// Refused: the shape as bools beside an `Option` per resource, which each step took back out
/// with an `expect`.
#[derive(Clone, Copy, Debug)]
enum FrameTarget<'a> {
    /// Into the target, through the frame's view of it.
    Direct {
        view: &'a wgpu::TextureView,
        texture: &'a wgpu::Texture,
    },
    /// Into the backbuffer, then onto the target.
    Backbuffer { bb: &'a Backbuffer },
    /// A partial frame's damage into a canvas at surface pixel `origin`, then each of `rects` into
    /// the backbuffer, then onto the target.
    Canvas {
        canvas: &'a DamageCanvas,
        bb: &'a Backbuffer,
        origin: UVec2,
        rects: &'a PartialScissors,
    },
}

impl<'a> FrameTarget<'a> {
    const fn kind(self) -> TargetKind {
        match self {
            Self::Direct { .. } => TargetKind::Direct,
            Self::Backbuffer { .. } => TargetKind::Backbuffer,
            Self::Canvas { .. } => TargetKind::Canvas,
        }
    }

    /// The view the main pass paints into.
    const fn color_view(self) -> &'a wgpu::TextureView {
        match self {
            Self::Direct { view, .. } => view,
            Self::Backbuffer { bb } => bb.view(),
            Self::Canvas { canvas, .. } => canvas.view(),
        }
    }

    /// The texture [`Self::color_view`] views, which a pane's capture copies out of.
    const fn color(self) -> &'a wgpu::Texture {
        match self {
            Self::Direct { texture, .. } => texture,
            Self::Backbuffer { bb } => bb.texture(),
            Self::Canvas { canvas, .. } => canvas.texture(),
        }
    }

    /// The surface pixel the main pass's target pixel 0 is.
    const fn origin(self) -> UVec2 {
        match self {
            Self::Direct { .. } | Self::Backbuffer { .. } => UVec2::ZERO,
            Self::Canvas { origin, .. } => origin,
        }
    }

    /// The backbuffer a frame through it puts on the target.
    const fn backbuffer(self) -> Option<&'a Backbuffer> {
        match self {
            Self::Direct { .. } => None,
            Self::Backbuffer { bb } | Self::Canvas { bb, .. } => Some(bb),
        }
    }
}

/// What every pass of the main pass draws into: color, optional stencil, the clear color and the
/// scissors it repaints, picked together per frame.
#[derive(Clone, Copy, Debug)]
struct MainPass<'a> {
    color_view: &'a wgpu::TextureView,
    /// The texture `color_view` views, which a pane's capture copies out of.
    color: &'a wgpu::Texture,
    stencil_view: Option<&'a wgpu::TextureView>,
    clear: wgpu::Color,
    scissors: &'a RepaintScissors,
    /// Whether `color` is a damage canvas, which the viewport moves the surface onto.
    canvas: bool,
}

/// One pass of the main pass, as the frame's plan sets it ([`PassStep::Main`]).
#[derive(Debug)]
struct SegmentPass<'a, 't> {
    main: MainPass<'a>,
    segment: MainSegment,
    groups: GroupSpan,
    load: ColorLoad,
    /// The stream's timestamp queries, when timing is on.
    timings: Option<&'t mut GpuTimings>,
}

/// One pass's draws: the frame's, the pass's pipelines, and the recorder they record through.
#[derive(Debug)]
struct DrawCtx<'a, 'p> {
    draw: FrameDraw<'a>,
    pipelines: PassPipelines<'a>,
    rec: &'p mut PassRecorder<'a>,
    /// The stream's timestamp queries, when timing is on.
    timings: Option<&'p mut GpuTimings>,
}

impl DrawCtx<'_, '_> {
    /// Record `body`, a batch of `kind`, under its timing mark and its debug group.
    fn batch(&mut self, kind: BatchKind, body: impl FnOnce(&mut Self)) {
        if let Some(t) = self.timings.as_deref_mut() {
            t.mark(self.rec, kind);
        }
        self.rec.push_marker(kind.label());
        body(self);
        self.rec.pop_marker();
    }
}

/// The canvas a partial frame paints: its rect, and the frame's scissors, each copied out of it.
#[derive(Clone, Copy, Debug)]
struct CanvasCut<'a> {
    rect: URect,
    rects: &'a PartialScissors,
}

/// What [`WgpuBackend::run_steps`] records the frame's passes with.
#[derive(Debug)]
struct FrameSteps<'a, 't> {
    draw: FrameDraw<'a>,
    main: MainPass<'a>,
    frame_target: FrameTarget<'a>,
    target: RenderTarget<'a>,
    /// The target's view, where a pass writes it.
    surface_view: Option<&'a wgpu::TextureView>,
    /// The stream's timestamp queries, when timing is on.
    timings: Option<&'t mut GpuTimings>,
}

/// What [`WgpuBackend::upload_frame`] uploads for: the submission, its stream, the target's
/// format, and the display frame.
#[derive(Debug)]
struct FrameUpload<'s, 'a> {
    submission: &'s Submission<'a>,
    stream: &'s mut RenderStream,
    format: TargetFormat,
    /// The display frame the submit belongs to ([`DisplayFrames`]).
    frame: u64,
}

impl WgpuBackend {
    /// Build the one shared GPU renderer over the host's handles. Per-format pipelines build when
    /// a window first notes a target of the format ([`Self::prepare_format`]).
    pub(crate) fn new(gpu: Gpu, resources: BackendResources<'_>, timing: FrameTiming) -> Self {
        let image_store = Rc::clone(resources.image_store);
        let effect_store = Rc::clone(resources.effect_store);
        let texture_binding = resources.texture_binding;
        let gradient =
            GpuGradientAtlas::new(&gpu, resources.gradient_rows, texture_binding.clone());
        let backdrops = Backdrops::new(&gpu);
        let quad = QuadPipeline::new(&gpu, &texture_binding, backdrops.layout());
        let mesh = MeshPipeline::new(&gpu);
        let image = ImagePipeline::new(&gpu, &texture_binding);
        let gpu_view_targets = GpuViewTargets::new(texture_binding.clone());
        let curve = CurvePipeline::new(&gpu, &texture_binding);
        let raster = RasterProgram::new(&gpu);
        let effect = EffectPipeline::new(&gpu);
        let blit = BlitPipeline::new(&gpu, &texture_binding);
        let text = TextBackend::new(&gpu, &raster, resources.text.clone());
        let icon = IconBackend::new(&gpu, &raster, resources.icons.clone());
        let debug = DebugOverlay::default();
        // 1 MiB chunks, above the resize-drag upload peak, so steady state uses 1-2 chunks.
        let staging = UploadStaging::new(&gpu.device, 1 << 20);
        let timestamp_period = gpu.queue.get_timestamp_period();
        // A device whose timestamps have no period times frames on the CPU clock.
        let timing = (timing == FrameTiming::On).then(|| TimingSupport {
            period_ns: timestamp_period,
            features: if timestamp_period > 0.0 {
                gpu.device.features()
            } else {
                wgpu::Features::empty()
            },
        });
        Self {
            gpu,
            staging,
            frames: DisplayFrames::default(),
            gradient,
            programs: Programs {
                quad,
                mesh,
                image,
                curve,
                raster,
                effect,
            },
            backdrops,
            image_store,
            effect_store,
            gpu_view_targets,
            blit,
            icon,
            text,
            debug,
            texture_binding,
            mask_plan: MaskPlan::default(),
            passes: FramePasses::default(),
            pipelines: FxHashMap::default(),
            timing,
        }
    }

    /// The registered images, which the next device's backend holds too.
    pub(crate) const fn image_store(&self) -> &Rc<WgpuImageStore> {
        &self.image_store
    }

    /// The loaded effects, which the next device's backend builds again.
    pub(crate) const fn effect_store(&self) -> &Rc<WgpuEffectStore> {
        &self.effect_store
    }

    /// Whether this backend times frames, which the next device's does too.
    pub(crate) const fn frame_timing(&self) -> FrameTiming {
        if self.timing.is_some() {
            FrameTiming::On
        } else {
            FrameTiming::Off
        }
    }

    /// A stream for `owner`'s frames: its cutouts take the device's mode, and its frames are
    /// timed where the backend times frames.
    pub(crate) fn open_stream(&self, owner: RenderOwnerId) -> RenderStream {
        let timings = self
            .timing
            .map(|support| GpuTimings::new(&self.gpu, support.period_ns, support.features));
        RenderStream::new(owner, self.programs.quad.cutout_mode(), timings)
    }

    /// Build the pipeline set for `format`, where none is built, as a window notes a target of
    /// it: before the frame that paints it, so its first submit compiles nothing. Only
    /// `wgpu::RenderPipeline`s carry the color-target format, so a new format costs a few
    /// pipeline compiles and no image re-upload or glyph re-rasterization.
    pub(crate) fn prepare_format(&mut self, format: TargetFormat) {
        if !self.pipelines.contains_key(&format) {
            let built = FormatPipelines::new(&self.gpu, format.get(), &self.programs);
            self.pipelines.insert(format, built);
        }
        self.blit.ensure(&self.gpu, format.get());
        self.effect_store.ensure_format(format.get());
    }

    /// Render one frame of `stream` into `target`. Skip frames never reach this method.
    ///
    /// [`Submission::route`] picks the path: [`Route::ViaBackbuffer`] renders into the stream's
    /// backbuffer and copies onto `target`, whose texture needs `COPY_DST` or draws it on;
    /// [`Route::Direct`] renders straight into `target`, a partial frame loading what it holds,
    /// which only a target that keeps its pixels may take. The scissors are the effective plan:
    /// escalations were sealed before the draw list was built.
    pub(crate) fn submit(
        &mut self,
        stream: &mut RenderStream,
        target: RenderTarget<'_>,
        submission: Submission<'_>,
    ) {
        tracy::zone!();
        let Submission {
            buffer,
            scissors: repaint_scissors,
            route,
            ..
        } = submission;
        let via_backbuffer = route == Route::ViaBackbuffer;
        if via_backbuffer {
            let made = stream.ensure_backbuffer(&self.gpu, &self.texture_binding, target);
            // A partial arrives un-escalated only onto a fresh backbuffer, so a new one means that
            // broke; the list was already culled for the partial, so escalating cannot fix it.
            debug_assert!(
                !made || !repaint_scissors.is_partial(),
                "backbuffer made under a partial frame whose draw list was culled for it"
            );
        }
        stream.mark_painted(route);
        let stencil = !buffer.rounded_clips.is_empty();
        let frame = self.frames.advance(stream.owner());
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

        let format = target.format();
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
                frame,
            },
        );

        let canvas_cut = self.canvas_cut(&submission, target);
        if let Some(CanvasCut { rect, .. }) = canvas_cut {
            stream.ensure_canvas(
                &self.gpu,
                target.texture().format(),
                DamageCanvas::class(rect, target.size()),
            );
            if stencil {
                stream.ensure_canvas_stencil(&self.gpu);
            }
        }
        // The window's stencil only where the main pass draws onto the backbuffer or the target:
        // a canvas frame stamps the canvas's own.
        let window_stencil = stencil && canvas_cut.is_none();
        stream.age_stencil(&self.gpu, frame, window_stencil.then(|| target.size()));

        let outlines_damage = submission.overlay.damage.is_some();
        // At most one surface view, only where a pass writes it: a direct frame's main pass and
        // the outline. The normal backbuffer path takes none, and a target that keeps its view
        // makes none.
        let surface_view = (!via_backbuffer || outlines_damage).then(|| target.view());
        let StreamPasses {
            backbuffer,
            canvas,
            stencil_view,
            mut timings,
        } = stream.passes();
        let frame_target = if via_backbuffer {
            let bb = backbuffer.expect("a frame through the backbuffer made it");
            match canvas_cut {
                Some(CanvasCut { rect, rects }) => FrameTarget::Canvas {
                    canvas: canvas.expect("made for the frame"),
                    bb,
                    origin: rect.min,
                    rects,
                },
                None => FrameTarget::Backbuffer { bb },
            }
        } else {
            FrameTarget::Direct {
                view: surface_view
                    .as_ref()
                    .expect("a direct frame makes the target's view"),
                texture: target.texture(),
            }
        };
        self.passes.plan(
            FrameShape {
                target: frame_target.kind(),
                partial: repaint_scissors.is_partial(),
                dims_undamaged: submission.dims_undamaged(),
                outlines_damage,
            },
            self.backdrops.breaks().iter().map(|at| at.barrier),
        );
        debug_assert_eq!(
            self.passes.writes_target(frame_target.kind()),
            surface_view.is_some(),
            "a view of the target exactly where a pass writes it"
        );
        self.ensure_pipelines(format, stencil, buffer);
        let main_pass_cpu = self.run_steps(
            FrameSteps {
                draw: FrameDraw {
                    fmt: &self.pipelines[&format],
                    blit: self.blit.get(format.get()),
                    buffer,
                    viewport: ViewportPush::for_buffer(buffer),
                    origin: frame_target.origin(),
                    masks: stencil.then_some(&self.mask_plan),
                    backdrops: self.backdrops.group(),
                },
                main: MainPass {
                    color_view: frame_target.color_view(),
                    color: frame_target.color(),
                    stencil_view: match frame_target {
                        FrameTarget::Canvas { canvas, .. } => {
                            stencil.then(|| canvas.stencil().expect("made for the frame").view())
                        }
                        FrameTarget::Direct { .. } | FrameTarget::Backbuffer { .. } => {
                            stencil_view.filter(|_| stencil)
                        }
                    },
                    clear: wgpu_color(submission.clear),
                    scissors: repaint_scissors,
                    canvas: frame_target.kind() == TargetKind::Canvas,
                },
                frame_target,
                target,
                surface_view: surface_view.as_deref(),
                timings: timings.as_deref_mut(),
            },
            &mut encoder,
        );

        if let Some(t) = timings.as_deref_mut() {
            t.finish(&mut encoder);
        }
        self.staging.finish_and_recall_on_submit(&encoder);
        self.gpu.queue.submit(iter::once(encoder.finish()));
        // Every frame, timing on or off, as it costs two `Instant::now()` calls.
        let stats = submission.stats;
        stats.record_main_pass_cpu(main_pass_cpu);
        if let Some(t) = timings {
            t.after_submit(&self.gpu, stats);
        }

        let frame = self.text.end_frame();
        self.icon.end_frame(frame);
    }

    /// Where a pass loads and stores its whole target, the canvas a partial frame paints the size
    /// of its damage, as a pass on the backbuffer would move the whole surface to keep what lies
    /// around it. A pane that captures reads the target where the canvas does not reach, so this
    /// runs after the upload, which plans the breaks.
    fn canvas_cut<'a>(
        &self,
        submission: &Submission<'a>,
        target: RenderTarget<'_>,
    ) -> Option<CanvasCut<'a>> {
        match submission.scissors {
            RepaintScissors::Partial(rects)
                if submission.route == Route::ViaBackbuffer
                    && !submission.dims_undamaged()
                    && self.backdrops.breaks().is_empty()
                    && self.gpu.pass_traffic() == PassTraffic::WholeTarget =>
            {
                DamageCanvas::rect(rects.bounds(), target.size())
                    .map(|rect| CanvasCut { rect, rects })
            }
            _ => None,
        }
    }

    /// The pipelines this frame draws through and the set of `format` lacks, built before its
    /// passes borrow it: the pre-clear and the overlays draw solid quads.
    fn ensure_pipelines(&mut self, format: TargetFormat, stencil: bool, buffer: &RenderBuffer) {
        self.pipelines
            .get_mut(&format)
            .expect("a window notes its target's format before it records")
            .ensure(
                &self.gpu,
                &self.programs,
                PipelineCensus {
                    stencil: if stencil {
                        StencilMode::Test
                    } else {
                        StencilMode::Off
                    },
                    quads: self
                        .programs
                        .quad
                        .taken()
                        .with(QuadDraw::Form(QuadForm::Solid)),
                    meshes: !buffer.meshes.is_empty(),
                    images: !buffer.images.is_empty(),
                    curves: !buffer.curves.is_empty(),
                    rasters: !buffer.texts.is_empty() || !buffer.icons.is_empty(),
                },
            );
    }

    /// Record the passes [`FramePasses`] planned, and return the CPU time of the main pass.
    fn run_steps(&self, steps: FrameSteps<'_, '_>, encoder: &mut wgpu::CommandEncoder) -> Duration {
        let FrameSteps {
            draw,
            main,
            frame_target,
            target,
            surface_view,
            mut timings,
        } = steps;
        let mut started = None;
        let mut main_pass_cpu = None;
        for &step in self.passes.steps() {
            match step {
                PassStep::Dim => self.run_dim_pass(draw, main.color_view, encoder),
                PassStep::Main {
                    segment,
                    groups,
                    load,
                } => {
                    if segment.opens() {
                        started = Some(Instant::now());
                    }
                    self.run_main_segment(
                        draw,
                        SegmentPass {
                            main,
                            segment,
                            groups,
                            load,
                            timings: timings.as_deref_mut(),
                        },
                        encoder,
                    );
                    if segment.closes() {
                        let started = started.expect("a main pass opens before it closes");
                        main_pass_cpu = Some(started.elapsed());
                    }
                }
                PassStep::Break { at } => {
                    let breaks = &self.backdrops.breaks()[at];
                    let mark = |phase| Mark::Backdrop { at, phase };
                    encoder_mark(
                        timings.as_deref_mut(),
                        encoder,
                        mark(BackdropPhase::CaptureStart),
                    );
                    self.backdrops.copy_break(encoder, breaks, main.color);
                    encoder_mark(
                        timings.as_deref_mut(),
                        encoder,
                        mark(BackdropPhase::BlurStart),
                    );
                    self.backdrops.blur_break(encoder, breaks);
                    encoder_mark(
                        timings.as_deref_mut(),
                        encoder,
                        mark(BackdropPhase::BlurEnd),
                    );
                }
                PassStep::CanvasOut => {
                    let FrameTarget::Canvas {
                        canvas,
                        bb,
                        origin,
                        rects,
                    } = frame_target
                    else {
                        unreachable!("the plan copies a canvas out only from a canvas");
                    };
                    for rect in rects {
                        canvas.copy_into(encoder, bb.texture(), rect, origin);
                    }
                }
                PassStep::BackbufferOut => {
                    let bb = frame_target
                        .backbuffer()
                        .expect("the plan puts a backbuffer out only through one");
                    encoder_mark(timings.as_deref_mut(), encoder, Mark::CopyOutStart);
                    bb.present_onto(encoder, target, draw.blit, self.gpu.backbuffer_out());
                    encoder_mark(timings.as_deref_mut(), encoder, Mark::CopyOutEnd);
                }
                PassStep::Overlay => {
                    let view = surface_view.expect("the outline writes the target");
                    self.run_overlay_pass(draw, view, encoder);
                }
            }
        }
        main_pass_cpu.expect("every frame closes its main pass")
    }

    /// The belt-routed upload phase of one [`Self::submit`].
    fn upload_frame(&mut self, encoder: &mut wgpu::CommandEncoder, upload: FrameUpload<'_, '_>) {
        let FrameUpload {
            submission,
            stream,
            format,
            frame,
        } = upload;
        let Submission {
            store,
            buffer,
            gradients,
            gpu_views,
            clear,
            scissors,
            route,
            overlay,
            ..
        } = *submission;
        let owner = stream.owner();
        let mut ctx = GpuCtx::new(&self.gpu, &mut self.staging, encoder, frame);

        if let Some(rows) = gradients {
            self.gradient.upload(&mut ctx, rows);
        }

        // Damage-rect overlay quads (debug): uploaded now, drawn last after the backbuffer copy.
        if let Some(damage) = overlay.damage {
            self.debug.build_outlines(damage, buffer);
        } else {
            self.debug.clear_outlines();
        }
        let masks: &[Quad] = if buffer.rounded_clips.is_empty() {
            &[]
        } else {
            tracy::zone!("stage_masks");
            self.mask_plan.build(buffer);
            self.mask_plan.quads()
        };
        let viewport = buffer.display.physical.as_vec2();
        self.programs.quad.upload(
            &mut ctx,
            &buffer.quads,
            stream.cutouts(),
            QuadExtras {
                masks,
                clear: scissors
                    .is_partial()
                    .then(|| quad_pipeline::pre_clear(viewport, clear)),
                dim: submission
                    .dims_undamaged()
                    .then(|| debug_overlay::dim_quad(viewport)),
                outlines: self.debug.outlines(),
            },
        );
        self.programs
            .quad
            .prepare_cutouts(&mut ctx, stream.cutouts());
        self.programs.mesh.upload(
            &mut ctx,
            MeshUpload {
                vertices: &store.mesh_vertices,
                idxs: &store.mesh_idxs,
                instances: buffer.meshes.instance(),
            },
        );
        self.programs
            .image
            .upload(&mut ctx, buffer.images.instance());
        // Paint every GpuView composited this frame into its off-screen target before
        // the main pass samples it, then free this submitter's targets absent from
        // `buffer.live_targets` (a wider set than those painted, so an unchanged view keeps
        // its texture). Eviction is owner-scoped because the backend serves every window.
        self.gpu_view_targets.paint_gpu_views(
            &mut ctx,
            buffer.frame_views(),
            gpu_views,
            ViewSubmitter {
                owner,
                now: buffer.time,
                text: self.text.shaper(),
            },
        );
        self.programs.curve.upload(&mut ctx, &buffer.curves);
        self.programs.effect.upload(
            &mut ctx,
            &self.effect_store,
            EffectUpload {
                instances: buffer.effects.instance(),
                words: &store.effect_params,
                format: format.get(),
            },
        );

        // Every frame, so a pane that left frees its cached result.
        self.backdrops.prepare(
            &mut ctx,
            BackdropFrame {
                buffer,
                owner,
                format,
                clear,
                backbuffer: if route == Route::ViaBackbuffer {
                    stream.frame_backbuffer()
                } else {
                    None
                },
            },
        );

        {
            tracy::zone!(
                "text.prepare_batches",
                value = buffer.text_batches.len() as u64
            );
            let interned_text = store.text.interned();
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
        self.overlay_draw(draw).quads(
            &mut pass,
            self.programs.quad.instances(),
            self.programs.quad.dim(),
        );
    }

    /// The no-stencil solid quad binding both debug-overlay passes draw through.
    const fn overlay_draw<'a>(&'a self, draw: FrameDraw<'a>) -> OverlayDraw<'a> {
        OverlayDraw {
            binding: QuadBinding {
                pipeline: draw
                    .fmt
                    .for_pass(StencilMode::Off)
                    .quad(QuadDraw::Form(QuadForm::Solid)),
                gradient_bind_group: &self.gradient.bind_group,
            },
            viewport: draw.viewport,
        }
    }

    /// One pass of the main pass: its groups, walked once per repaint scissor (once, unscissored,
    /// on Full). The scissors are disjoint ([`RepaintScissors::new`]), as a later pass paints over
    /// what the one before it left. Timestamps open with the first pass and close with the last,
    /// the breaks between count in no category, and each pass adds its pipeline statistics.
    ///
    /// Every walk leaves the stencil clean: one ending with a mask stamped emits a tail
    /// clear under the stamp's scissor, so no walk reads another's stencil writes.
    fn run_main_segment(
        &self,
        draw: FrameDraw<'_>,
        segment_pass: SegmentPass<'_, '_>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        tracy::zone!();
        let SegmentPass {
            main,
            segment,
            groups,
            load,
            mut timings,
        } = segment_pass;
        let MainPass {
            color_view,
            color: _,
            stencil_view,
            clear,
            scissors,
            canvas,
        } = main;
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
        let load_op = match load {
            ColorLoad::Clear => wgpu::LoadOp::Clear(clear),
            ColorLoad::Load => wgpu::LoadOp::Load,
        };
        // The descriptor covers basic mode; with `TIMESTAMP_QUERY_INSIDE_PASSES` we write inline
        // via `segment_begin` / `segment_end` for one gap-free stream.
        let timestamp_writes = timings.as_deref().and_then(|t| t.pass_writes(segment));
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
        if let Some(t) = timings.as_deref_mut() {
            t.segment_begin(&mut pass, segment);
        }
        if canvas {
            // The surface's viewport, moved so its `origin` lands on the canvas's pixel 0: the
            // same clip space as on the backbuffer, a whole number of pixels away.
            let size = draw.viewport.size;
            pass.set_viewport(
                -(draw.origin.x as f32),
                -(draw.origin.y as f32),
                size.x,
                size.y,
                0.0,
                1.0,
            );
        }
        // One for the pass: what a scissor replay leaves bound stays bound for the next.
        let mut rec = PassRecorder::new(pass, draw.viewport.with_origin(draw.origin));
        let stencil = if draw.masks.is_some() {
            StencilMode::Test
        } else {
            StencilMode::Off
        };
        let mut ctx = DrawCtx {
            draw,
            pipelines: draw.fmt.for_pass(stencil),
            rec: &mut rec,
            timings: timings.as_deref_mut(),
        };
        let schedule = |damage_scissor| Schedule {
            buffer: draw.buffer,
            damage_scissor,
            masks: draw.masks,
            groups,
            load,
        };
        match scissors {
            RepaintScissors::Full => self.render_groups(&mut ctx, schedule(None)),
            RepaintScissors::Partial(rects) => {
                for r in rects {
                    tracing::trace!(scissor = ?r, "wgpu_backend.submit.pass.partial_rect");
                    self.render_groups(&mut ctx, schedule(Some(r)));
                }
            }
        }
        if let Some(t) = timings {
            t.segment_end(&mut rec, segment);
        }
    }

    /// Dispatch each step of `schedule` ([`Schedule::for_each_step`]) to the pass; ordering lives
    /// in the schedule module so tests assert it without a GPU.
    fn render_groups<'a>(&'a self, ctx: &mut DrawCtx<'a, '_>, schedule: Schedule<'a>) {
        tracy::zone!();
        let draw = ctx.draw;
        let buffer = draw.buffer;
        let images = self.image_store.read();
        let textures = ImageTextures {
            images: &images,
            targets: &self.gpu_view_targets,
        };
        // Read at a raster draw, as a frame that draws none builds no raster pipeline.
        let pipelines = ctx.pipelines;
        let raster = move || pipelines.raster();
        let quad = |pipeline| QuadBinding {
            pipeline,
            gradient_bind_group: &self.gradient.bind_group,
        };
        schedule.for_each_step(&mut |step| match step {
            RenderStep::PreClear => ctx.batch(BatchKind::PreClear, |ctx| {
                // Through the recorder, so the viewport is pushed before the draw; else the clear
                // quad reads stale immediates (zero on a partial pass's first PreClear) and
                // clears nothing.
                let solid = ctx.pipelines.quad(QuadDraw::Form(QuadForm::Solid));
                self.programs.quad.draw_clear(ctx.rec, quad(solid));
            }),
            RenderStep::SetScissor(r) => {
                ctx.rec.scissor(URect {
                    min: r.min - draw.origin,
                    size: r.size,
                });
            }
            RenderStep::SetStencilRef(v) => ctx.rec.stencil_reference(v),
            RenderStep::MaskStamp(mi) => ctx.batch(BatchKind::Mask, |ctx| {
                let stamp = ctx.pipelines.mask_stamp();
                self.programs.quad.bind_mask(ctx.rec, quad(stamp));
                self.programs.quad.draw_mask(ctx.rec, mi);
            }),
            RenderStep::MaskClear(mi) => ctx.batch(BatchKind::Mask, |ctx| {
                let clear = ctx.pipelines.mask_clear();
                self.programs.quad.bind_mask(ctx.rec, quad(clear));
                self.programs.quad.draw_mask(ctx.rec, mi);
            }),
            RenderStep::Quads { range } => {
                for run in self.programs.quad.runs(range) {
                    ctx.batch(run.key.batch_kind(), |ctx| {
                        self.programs.quad.draw_run(
                            ctx.rec,
                            QuadPass {
                                pipelines: ctx.pipelines,
                                gradient_bind_group: &self.gradient.bind_group,
                                backdrops: draw.backdrops,
                            },
                            QuadRun {
                                draw: run.key,
                                instances: run.instances,
                            },
                        );
                    });
                }
            }
            RenderStep::Text { batch } => ctx.batch(BatchKind::Text, |ctx| {
                self.text.render_batch(ctx.rec, batch, raster());
            }),
            RenderStep::TierBatch { tier, batch } => {
                // The timing bucket and label come from the tier, so a new tier cannot reach the
                // pass untimed.
                ctx.batch(batch_kind(tier), |ctx| {
                    let items = || buffer.batches(tier)[batch].items;
                    match tier {
                        PaintTier::Mesh => self.programs.mesh.draw_batch(
                            ctx.rec,
                            ctx.pipelines.mesh(),
                            MeshBatch {
                                draws: buffer.meshes.draw(),
                                items: items(),
                            },
                        ),
                        PaintTier::Image => self.programs.image.draw_batch(
                            ctx.rec,
                            ctx.pipelines.image(),
                            ImageBatch {
                                ids: buffer.images.id(),
                                items: items(),
                            },
                            textures,
                        ),
                        PaintTier::Icon => self.icon.render_batch(ctx.rec, batch, raster()),
                        PaintTier::Curve => self.programs.curve.draw_batch(
                            ctx.rec,
                            ctx.pipelines.curve(),
                            &self.gradient.bind_group,
                            items(),
                        ),
                        PaintTier::Effect => self.programs.effect.draw_batch(
                            ctx.rec,
                            ctx.pipelines.stencil(),
                            EffectBatch {
                                ids: buffer.effects.id(),
                                items: items(),
                            },
                        ),
                    }
                });
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
        self.overlay_draw(draw).quads(
            &mut pass,
            self.programs.quad.instances(),
            self.programs.quad.outlines(),
        );
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
            .backbuffer()
            .expect("a skip follows a frame painted through the backbuffer");
        let blit = self.blit.get(target.format().get());
        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.skip"),
            });
        backbuffer.present_onto(&mut encoder, target, blit, self.gpu.backbuffer_out());
        self.gpu.queue.submit(iter::once(encoder.finish()));
    }

    /// Start copying `region` of `stream`'s backbuffer, which its last submit painted, for a
    /// screenshot.
    pub(crate) fn capture(&mut self, stream: &RenderStream, region: URect) -> Capture {
        let backbuffer = stream
            .backbuffer()
            .expect("a frame with a screenshot request paints through the backbuffer");
        let needs_blit = TexelOrder::of(backbuffer.texture().format()) == TexelOrder::Blit;
        if needs_blit {
            self.blit.ensure(&self.gpu, Capture::FORMAT);
        }
        let blit = needs_blit.then(|| CaptureBlit {
            pipeline: self.blit.get(Capture::FORMAT),
            binding: &self.texture_binding,
        });
        let source = CaptureSource {
            texture: backbuffer.texture(),
            blit,
        };
        Capture::start(&self.gpu, source, region)
    }

    /// Run the map callbacks of the copies the device has finished, without waiting for the rest.
    pub(crate) fn poll(&self) {
        self.gpu.poll();
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

    /// Release what the backend keeps for `stream`'s owner, its `GpuView` targets and pane
    /// results, and the cutout cells its plan holds, as its window closes; the stream itself drops
    /// with the window. Per-submit eviction is owner-scoped, so without this survivors would hold
    /// its textures until shutdown, and its tables' cells would stay out of the atlas's reach.
    #[cfg_attr(
        all(not(feature = "winit"), not(test)),
        expect(
            dead_code,
            reason = "not `cfg`: the winit host is only the current caller, and an embedding host needs this entry point too"
        )
    )]
    pub(crate) fn retire_stream(&mut self, stream: &mut RenderStream) {
        let owner = stream.owner();
        self.frames.retire(owner);
        self.gpu_view_targets.retire_owner(owner);
        self.backdrops.retire(owner);
        stream.release_cutouts(self.programs.quad.cutout_placements_mut());
    }

    /// Where the shadow atlas places each key's table, which a frame's plan builds in.
    pub(crate) const fn cutout_placements_mut(&mut self) -> &mut CutoutPlacements {
        self.programs.quad.cutout_placements_mut()
    }
}

/// Timestamp `mark` between passes, where the frame is timed.
fn encoder_mark(timings: Option<&mut GpuTimings>, encoder: &mut wgpu::CommandEncoder, mark: Mark) {
    if let Some(t) = timings {
        t.encoder_mark(encoder, mark);
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
        PaintTier::Effect => BatchKind::Effect,
    }
}

/// `clear` as a pass's load op takes it.
fn wgpu_color(clear: RgbaF16) -> wgpu::Color {
    let clear = RgbaF32::from(clear);
    wgpu::Color {
        r: f64::from(clear.r),
        g: f64::from(clear.g),
        b: f64::from(clear.b),
        a: f64::from(clear.a),
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
    #[cfg(test)]
    use crate::gpu::device::backbuffer_out::BackbufferOut;
    #[cfg(test)]
    use crate::gpu::device::pass_traffic::PassTraffic;
    #[cfg(test)]
    use crate::gpu::pipeline::format_pipelines::FormatPipelines;
    #[cfg(test)]
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutMode;
    #[cfg(test)]
    use crate::gpu::raster::raster_atlas::counters::AtlasCounts;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::wgpu_backend::WgpuBackend;

    impl WgpuBackend {
        /// Draw every shadow as one cell of the full form instead of its grid: the reference the
        /// grid is compared with. Builds every format's pipelines again.
        pub(crate) fn disable_shadow_grid(&mut self) {
            self.programs.quad.disable_shadow_grid();
            let formats: Vec<TargetFormat> = self.pipelines.keys().copied().collect();
            self.pipelines.clear();
            for format in formats {
                self.prepare_format(format);
            }
        }

        /// How this device cuts shadow corners.
        #[cfg(test)]
        pub(crate) const fn cutout_mode(&self) -> CutoutMode {
            self.programs.quad.cutout_mode()
        }

        /// The glyph atlas's eviction and growth counts so far.
        #[cfg(test)]
        pub(crate) fn text_atlas_counts(&self) -> AtlasCounts {
            self.text.atlas_counts()
        }

        /// The pipelines built for `format`.
        #[cfg(test)]
        pub(crate) fn format_pipelines(&self, format: TargetFormat) -> &FormatPipelines {
            &self.pipelines[&format]
        }

        /// The shadow cutout tables the atlas holds from the frames before.
        #[cfg(test)]
        pub(crate) fn held_cutout_tables(&self) -> usize {
            self.programs.quad.held_cutout_tables()
        }

        /// Paint as a device whose passes move `traffic` paints, whatever the device is.
        #[cfg(test)]
        pub(crate) const fn paint_as(&mut self, traffic: PassTraffic) {
            self.gpu.paint_as(traffic);
        }

        /// Put the backbuffer out by `out`, whatever the device is.
        #[cfg(test)]
        pub(crate) const fn put_backbuffer_by(&mut self, out: BackbufferOut) {
            self.gpu.put_backbuffer_by(out);
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
