//! One render owner's side of the device: what its frames keep there between them.

use crate::gpu::backdrop::backdrop_blur::TargetSource;
use crate::gpu::backdrop::backdrops::FrameBackbuffer;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_timings::GpuTimings;
use crate::gpu::frame::submission::Route;
use crate::gpu::pipeline::quad_pipeline::cutout_placements::CutoutPlacing;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{Census, CutoutFrame, CutoutPlan};
use crate::gpu::render_owner_id::RenderOwnerId;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::backbuffer::Backbuffer;
use crate::gpu::surface::damage_canvas::DamageCanvas;
use crate::gpu::surface::render_target::RenderTarget;
use crate::renderer::render_buffer::RenderBuffer;
use crate::renderer::repaint_scissors::RepaintScissors;
use glam::UVec2;

/// What one render owner's frames keep on the device: the backbuffer that holds the last frame
/// painted through it, the shadow cutouts its frames plan,
/// and the queries its frames are timed with. The backend opens it
/// ([`WgpuBackend::open_stream`](crate::gpu::wgpu_backend::WgpuBackend::open_stream)), so its
/// cutouts take the device's mode; it drops with its window, as the backend retires the owner.
#[derive(Debug)]
pub(crate) struct RenderStream {
    owner: RenderOwnerId,
    /// Off-screen target holding the last frame painted through it, for `LoadOp::Load` partial
    /// damage; made on the first such frame, and again for a new size or format.
    backbuffer: Option<Backbuffer>,
    /// The group a backdrop's blur reads [`Self::backbuffer`] through as its level 0, made on the
    /// first frame that blurs from it and dropped with it.
    blur_source: Option<TargetSource>,
    /// Whether [`Self::backbuffer`] holds what the target shows. A direct frame leaves it stale,
    /// as does a new target; the next partial resyncs it with one full repaint.
    backbuffer_fresh: bool,
    /// The damage canvases of the last two size classes the owner's partial frames painted,
    /// the latest first: hovering from one widget to another alternates between two.
    canvases: [Option<DamageCanvas>; 2],
    cutouts: CutoutPlan,
    /// The owner's timestamp queries, where the backend times frames: a frame publishes late, so
    /// one set shared by every owner would publish a frame of another.
    timings: Option<GpuTimings>,
}

/// What a frame's passes paint through, read, beside the timings they write, borrowed at once
/// ([`RenderStream::passes`]).
#[derive(Debug)]
pub(super) struct StreamPasses<'a> {
    pub(super) backbuffer: Option<&'a Backbuffer>,
    /// The canvas [`RenderStream::ensure_canvas`] made ready last.
    pub(super) canvas: Option<&'a DamageCanvas>,
    pub(super) timings: Option<&'a mut GpuTimings>,
}

impl RenderStream {
    pub(super) const fn new(owner: RenderOwnerId, timings: Option<GpuTimings>) -> Self {
        Self {
            owner,
            backbuffer: None,
            blur_source: None,
            backbuffer_fresh: false,
            canvases: [None, None],
            cutouts: CutoutPlan::new(),
            timings,
        }
    }

    /// A stream for `owner` with no device behind it, untimed: a window's until a device
    /// attaches, and a CPU-only test's.
    pub(crate) const fn detached(owner: RenderOwnerId) -> Self {
        Self::new(owner, None)
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

    /// The backbuffer as a frame through it lends it to its backdrops, with the group they read it
    /// through; `None` where none is made.
    pub(super) fn frame_backbuffer(&mut self) -> Option<FrameBackbuffer<'_>> {
        let backbuffer = self.backbuffer.as_ref()?;
        Some(FrameBackbuffer {
            view: backbuffer.view(),
            size: backbuffer.size(),
            source: &mut self.blur_source,
        })
    }

    /// The backbuffer and the damage canvas a frame's passes paint through, made
    /// for the frame, and the timings they write.
    pub(super) const fn passes(&mut self) -> StreamPasses<'_> {
        StreamPasses {
            backbuffer: self.backbuffer.as_ref(),
            canvas: self.canvases[0].as_ref(),
            timings: self.timings.as_mut(),
        }
    }

    /// Plan the shadow cutouts of the frame `buffer` holds, repainted inside `scissors`, placed
    /// through `placing`.
    pub(crate) fn plan_cutouts(
        &mut self,
        buffer: &RenderBuffer,
        scissors: &RepaintScissors,
        placing: &mut CutoutPlacing<'_>,
    ) -> Census {
        let frame = CutoutFrame {
            quads: &buffer.quads,
            repaint: scissors,
            viewport: buffer.display.physical,
        };
        self.cutouts.build(frame, placing)
    }

    /// Give back through `placing` every cutout table this stream's plan holds, as its window
    /// closes.
    pub(super) fn release_cutouts(&mut self, placing: &mut CutoutPlacing<'_>) {
        self.cutouts.release_all(placing);
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

    /// Make the frame's canvas ([`StreamPasses::canvas`]) `size` in `format`: the one before it
    /// when it is, else a new one, which the oldest gives way to.
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
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::pipeline::quad_pipeline::cutout_plan::CutoutPlan;
    use crate::gpu::wgpu_backend::render_stream::RenderStream;

    impl RenderStream {
        /// Shade every shadow corner instead of baking tables: the visual suite's reference.
        pub(crate) fn disable_cutout_tables(&mut self) {
            self.cutouts = CutoutPlan::shading();
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
    use crate::gpu::render_owner_id::RenderOwners;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::wgpu_backend::render_stream::RenderStream;
    use glam::UVec2;

    /// A stream keeps the canvases of its last two size classes: a class it holds comes back as
    /// the same texture, and a third class takes the place of the one used longest ago. After
    /// `a`, `b`, `a`, `b` both are held; `c` drops `a`, so `b` comes back and `a` is new. A held
    /// size in another format is a new canvas.
    #[test]
    fn a_stream_keeps_the_canvases_of_its_last_two_classes() {
        let gpu = headless_test_gpu();
        let mut stream = RenderStream::detached(RenderOwners::default().reserve());
        let (a, b, c) = (UVec2::new(64, 64), UVec2::new(128, 64), UVec2::new(64, 128));
        let mut ready = |size, format| {
            stream.ensure_canvas(&gpu, format, size);
            stream.canvases[0].as_ref().unwrap().texture().clone()
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
