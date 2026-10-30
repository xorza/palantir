//! One render owner's side of the device: what its frames keep there between them.

use crate::gpu::frame::gpu_timings::GpuTimings;
use crate::gpu::pipeline::quad_pipeline::cutout_plan::{Census, CutoutMode, CutoutPlan};
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::surface::backbuffer::Backbuffer;
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
    pub(super) backbuffer: Option<Backbuffer>,
    /// Whether [`Self::backbuffer`] holds what the target shows. A direct frame leaves it stale,
    /// as does a new target; the next partial resyncs it with one full repaint.
    pub(super) backbuffer_fresh: bool,
    /// Rounded-clip stencil, made on the first frame with a rounded clip; apart from the
    /// backbuffer, so a direct frame needs none.
    pub(super) stencil: Option<Stencil>,
    cutouts: CutoutPlan,
    /// The owner's timestamp queries, where the backend times frames: a frame publishes late, so
    /// one set shared by every owner would publish a frame of another.
    pub(super) timings: Option<GpuTimings>,
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
            backbuffer_fresh: false,
            stencil: None,
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
    /// [`describes`](Backbuffer::describes) the target; `true` when it is new, which holds no
    /// pixel yet.
    pub(super) fn ensure_backbuffer(
        &mut self,
        device: &wgpu::Device,
        binding: &TextureBinding,
        target: RenderTarget<'_>,
    ) -> bool {
        if self
            .backbuffer
            .as_ref()
            .is_some_and(|held| !held.describes(target))
        {
            self.backbuffer = None;
        }
        let made = self.backbuffer.is_none();
        if made {
            self.backbuffer = Some(Backbuffer::new(device, binding, target));
        }
        made
    }

    /// The stencil at `size`, made when there is none or it has another size.
    pub(super) fn ensure_stencil(&mut self, device: &wgpu::Device, size: UVec2) {
        if !self.stencil.as_ref().is_some_and(|held| held.fits(size)) {
            self.stencil = Some(Stencil::new(device, size));
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

        /// Take the backbuffer as holding what the target shows, as a frame through it leaves it.
        #[cfg(test)]
        pub(crate) const fn assume_backbuffer_fresh(&mut self) {
            self.backbuffer_fresh = true;
        }
    }
}
