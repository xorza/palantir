//! One render owner's side of the device: what its frames keep there between them.

use crate::gpu::backdrop::backdrop_blur::TargetSource;
use crate::gpu::backdrop::backdrops::FrameBackbuffer;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_timings::GpuTimings;
use crate::gpu::frame::submission::Route;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{Census, CutoutMode, CutoutPlan};
use crate::gpu::resource::expiry_window::ExpiryWindow;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::damage_canvas::DamageCanvas;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::surface::stencil::Stencil;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::render_owner_id::RenderOwnerId;
use crate::renderer::repaint_scissors::RepaintScissors;
use glam::UVec2;

/// What one render owner's frames keep on the device: the backbuffer that holds the last frame
/// painted through it, the stencil its rounded clips stamp, the shadow cutouts its frames plan,
/// and the queries its frames are timed with. The backend opens it
/// ([`WgpuBackend::open_stream`](crate::gpu::wgpu_backend::WgpuBackend::open_stream)), so its
/// cutouts take the device's mode; it drops with its window, as the backend retires the owner.
#[derive(Debug)]
pub(crate) struct RenderStream {
    owner: RenderOwnerId,
    /// Off-screen target holding the last frame painted through it, for `LoadOp::Load` partial
    /// damage; made on the first such frame, and again for a new size or format.
    backbuffer: Option<Backbuffer>,
    /// The group a pane's blur reads [`Self::backbuffer`] through as its level 0, made on the
    /// first frame that blurs from it and dropped with it.
    blur_source: Option<TargetSource>,
    /// Whether [`Self::backbuffer`] holds what the target shows. A direct frame leaves it stale,
    /// as does a new target; the next partial resyncs it with one full repaint.
    backbuffer_fresh: bool,
    /// Rounded-clip stencil, made on the first frame with a rounded clip; apart from the
    /// backbuffer, so a direct frame needs none. Pure per-frame scratch, as each main pass clears
    /// it and discards it, so it goes at the end of an expiry window no frame stamped it in: at
    /// 7680 × 2160 it is 16 MiB.
    stencil: Option<Stencil>,
    /// The expiry window the stencil ages in, and whether a frame of it stamped the stencil.
    stencil_window: ExpiryWindow,
    stencil_used: bool,
    /// The damage canvases of the last two size classes the owner's partial frames painted,
    /// the latest first: hovering from one widget to another alternates between two.
    canvases: [Option<DamageCanvas>; 2],
    cutouts: CutoutPlan,
    /// The owner's timestamp queries, where the backend times frames: a frame publishes late, so
    /// one set shared by every owner would publish a frame of another.
    timings: Option<GpuTimings>,
}

impl RenderStream {
    pub(super) const fn new(
        owner: RenderOwnerId,
        cutout_mode: CutoutMode,
        timings: Option<GpuTimings>,
    ) -> Self {
        Self {
            owner,
            backbuffer: None,
            blur_source: None,
            backbuffer_fresh: false,
            stencil: None,
            stencil_window: ExpiryWindow::NEW,
            stencil_used: false,
            canvases: [None, None],
            cutouts: CutoutPlan::new(cutout_mode),
            timings,
        }
    }

    /// A stream for `owner` with no device behind it, cutting shadow corners by `cutout_mode`,
    /// untimed: a window's until a device attaches, and a CPU-only test's.
    pub(crate) const fn detached(owner: RenderOwnerId, cutout_mode: CutoutMode) -> Self {
        Self::new(owner, cutout_mode, None)
    }

    pub(crate) const fn owner(&self) -> RenderOwnerId {
        self.owner
    }

    /// Whether the backbuffer holds what the target shows, so a partial frame can paint through
    /// it.
    pub(crate) const fn is_backbuffer_fresh(&self) -> bool {
        self.backbuffer_fresh
    }

    /// Declare the target's contents gone: the backbuffer no longer mirrors it.
    pub(crate) const fn invalidate(&mut self) {
        self.backbuffer_fresh = false;
    }

    /// Note a frame painted by `route`: through the backbuffer it holds what the target shows; a
    /// direct frame leaves it stale. The one writer of [`Self::is_backbuffer_fresh`] beside
    /// [`Self::invalidate`].
    pub(super) fn mark_painted(&mut self, route: Route) {
        self.backbuffer_fresh = route == Route::ViaBackbuffer;
    }

    /// The backbuffer, made by a frame through it.
    pub(super) const fn backbuffer(&self) -> Option<&Backbuffer> {
        self.backbuffer.as_ref()
    }

    /// The backbuffer as a frame through it lends it to its panes, with the group they read it
    /// through; `None` where none is made.
    pub(super) fn frame_backbuffer(&mut self) -> Option<FrameBackbuffer<'_>> {
        let backbuffer = self.backbuffer.as_ref()?;
        Some(FrameBackbuffer {
            backbuffer,
            source: &mut self.blur_source,
        })
    }

    /// The window's stencil view, which [`Self::age_stencil`] made for a frame that stamps it.
    pub(super) fn stencil_view(&self) -> Option<&wgpu::TextureView> {
        self.stencil.as_ref().map(Stencil::view)
    }

    pub(super) const fn timings(&self) -> Option<&GpuTimings> {
        self.timings.as_ref()
    }

    pub(super) const fn timings_mut(&mut self) -> Option<&mut GpuTimings> {
        self.timings.as_mut()
    }

    /// Plan the shadow cutouts of the frame `buffer` holds, repainted inside `scissors`.
    pub(crate) fn plan_cutouts(
        &mut self,
        buffer: &RenderBuffer,
        scissors: &RepaintScissors,
    ) -> Census {
        self.cutouts
            .build(&buffer.quads, scissors, buffer.display.physical)
    }

    pub(super) const fn cutouts(&self) -> &CutoutPlan {
        &self.cutouts
    }

    /// Make the backbuffer for `target` when there is none or it no longer
    /// [`fits`](Backbuffer::fits) the target; `true` when it is new, which holds no
    /// pixel yet.
    pub(super) fn ensure_backbuffer(
        &mut self,
        gpu: &Gpu,
        binding: &TextureBinding,
        target: RenderTarget<'_>,
    ) -> bool {
        if self
            .backbuffer
            .as_ref()
            .is_some_and(|held| !held.fits(target))
        {
            self.backbuffer = None;
        }
        let made = self.backbuffer.is_none();
        if made {
            self.backbuffer = Some(Backbuffer::new(gpu, binding, target));
            self.blur_source = None;
        }
        made
    }

    /// Make [`Self::canvas`] `size` in `format`: the one before it when it is, else a new one,
    /// which the oldest gives way to.
    pub(super) fn ensure_canvas(&mut self, gpu: &Gpu, format: wgpu::TextureFormat, size: UVec2) {
        let fits = |canvas: &Option<DamageCanvas>| {
            canvas
                .as_ref()
                .is_some_and(|canvas| canvas.fits(size, format))
        };
        if fits(&self.canvases[0]) {
            return;
        }
        self.canvases.swap(0, 1);
        if !fits(&self.canvases[0]) {
            self.canvases[0] = Some(DamageCanvas::new(gpu, format, size));
        }
    }

    /// Give [`Self::canvas`] its stencil.
    pub(super) fn ensure_canvas_stencil(&mut self, gpu: &Gpu) {
        self.canvases[0]
            .as_mut()
            .expect("a canvas made ready")
            .ensure_stencil(gpu);
    }

    /// The canvas [`Self::ensure_canvas`] made ready last.
    pub(super) const fn canvas(&self) -> Option<&DamageCanvas> {
        self.canvases[0].as_ref()
    }

    /// Age the stencil by display frame `frame`, whose main pass stamps one at `size` or none:
    /// make it when there is none or it has another size, and drop it at the end of an expiry
    /// window no frame stamped it in.
    pub(super) fn age_stencil(&mut self, gpu: &Gpu, frame: u64, size: Option<UVec2>) {
        if let Some(size) = size {
            self.stencil_used = true;
            if !self.stencil.as_ref().is_some_and(|held| held.fits(size)) {
                self.stencil = Some(Stencil::new(gpu, size));
            }
        }
        if self.stencil_window.ends(frame) {
            if !self.stencil_used {
                self.stencil = None;
            }
            self.stencil_used = false;
        }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::{CutoutMode, CutoutPlan};
    use crate::gpu::wgpu_backend::render_stream::RenderStream;

    impl RenderStream {
        /// Shade every shadow corner instead of baking tables: the visual suite's reference.
        pub(crate) fn disable_cutout_tables(&mut self) {
            self.cutouts = CutoutPlan::new(CutoutMode::Shade);
        }

        /// Whether a partial frame painted a damage canvas.
        #[cfg(test)]
        pub(crate) const fn holds_canvas(&self) -> bool {
            self.canvases[0].is_some()
        }

        /// Take the backbuffer as holding what the target shows, as a frame through it leaves it.
        #[cfg(test)]
        pub(crate) const fn assume_backbuffer_fresh(&mut self) {
            self.backbuffer_fresh = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutMode;
    use crate::gpu::resource::expiry_window::EXPIRY_FRAMES;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::wgpu_backend::render_stream::RenderStream;
    use crate::renderer::render_owner_id::RenderOwners;
    use glam::UVec2;

    /// The stencil a frame stamps stays through the expiry window it fell in, and goes at the end
    /// of the next, which no frame stamped it in; a frame that stamps it again makes it again.
    #[test]
    fn a_stencil_no_frame_of_a_window_stamps_goes() {
        let gpu = headless_test_gpu();
        let mut stream =
            RenderStream::detached(RenderOwners::default().reserve(), CutoutMode::Shade);
        let size = UVec2::new(64, 32);
        stream.age_stencil(&gpu, 0, Some(size));
        for frame in 1..EXPIRY_FRAMES {
            stream.age_stencil(&gpu, frame, None);
        }
        assert!(stream.stencil.is_some(), "its window stamped it");
        for frame in EXPIRY_FRAMES..2 * EXPIRY_FRAMES {
            stream.age_stencil(&gpu, frame, None);
        }
        assert!(
            stream.stencil.is_none(),
            "a window with no rounded clip drops it"
        );
        stream.age_stencil(&gpu, 2 * EXPIRY_FRAMES, Some(size));
        assert!(stream.stencil.is_some());
    }

    /// A stream keeps the canvases of its last two size classes: a class it holds comes back as
    /// the same texture, and a third class takes the place of the one used longest ago. After
    /// `a`, `b`, `a`, `b` both are held; `c` drops `a`, so `b` comes back and `a` is new. A held
    /// size in another format is a new canvas.
    #[test]
    fn a_stream_keeps_the_canvases_of_its_last_two_classes() {
        let gpu = headless_test_gpu();
        let mut stream =
            RenderStream::detached(RenderOwners::default().reserve(), CutoutMode::Shade);
        let (a, b, c) = (UVec2::new(64, 64), UVec2::new(128, 64), UVec2::new(64, 128));
        let mut ready = |size, format| {
            stream.ensure_canvas(&gpu, format, size);
            stream.canvas().unwrap().texture().clone()
        };
        let srgb = wgpu::TextureFormat::Rgba8UnormSrgb;
        let first_a = ready(a, srgb);
        assert_eq!(ready(a, srgb), first_a);
        let first_b = ready(b, srgb);
        assert_ne!(first_b, first_a);
        assert_eq!(ready(a, srgb), first_a);
        assert_eq!(ready(b, srgb), first_b);
        assert_ne!(ready(c, srgb), first_a);
        assert_eq!(ready(b, srgb), first_b);
        assert_ne!(ready(a, srgb), first_a, "c dropped a");
        assert_ne!(ready(b, wgpu::TextureFormat::Bgra8UnormSrgb), first_b);
    }
}
