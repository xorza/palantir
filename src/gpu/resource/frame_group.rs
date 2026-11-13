//! The frame group: what every color pipeline of the main pass reads at one slot.

use crate::gpu::atlas::row_texture::RowTexture;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::frame::gpu_ctx::GpuCtx;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::renderer::render_buffer::rounded_clip::RoundedClip;

/// The group every color pipeline reads at
/// [`FRAME_GROUP`](crate::primitives::wgsl::FRAME_GROUP), which the main pass binds once: the
/// gradient atlas and the frame's rounded clips, the pool each draw's
/// [`ClipChain`](crate::renderer::render_buffer::clip_chain::ClipChain) names a run of. Owned by
/// the backend beside the instance buffers, as every window's submit uploads its own before its
/// passes.
#[derive(Debug)]
pub(crate) struct FrameGroup {
    binding: TextureBinding,
    clips: DynamicBuffer<RoundedClip>,
    group: wgpu::BindGroup,
    /// What `group` binds, which a grown atlas or pool changes.
    bound: Bound,
}

/// The atlas generation and the pool's capacity a frame group was made over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Bound {
    atlas: u64,
    clips: usize,
}

impl FrameGroup {
    /// The group over `atlas` and an empty pool.
    pub(crate) fn new(gpu: &Gpu, binding: TextureBinding, atlas: &RowTexture) -> Self {
        let clips = DynamicBuffer::storage(gpu, "palantir.frame.clips", 16);
        let group = binding.frame_group(gpu, atlas.view(), &clips.buffer);
        let bound = Bound {
            atlas: atlas.generation(),
            clips: clips.capacity(),
        };
        Self {
            binding,
            clips,
            group,
            bound,
        }
    }

    /// Upload the frame's `clips` and bind `atlas`, making the group again where the atlas grew or
    /// the pool moved to another buffer.
    pub(crate) fn upload(
        &mut self,
        ctx: &mut GpuCtx<'_>,
        atlas: &RowTexture,
        clips: &[RoundedClip],
    ) {
        self.clips.upload_instances(ctx, clips);
        let bound = Bound {
            atlas: atlas.generation(),
            clips: self.clips.capacity(),
        };
        if bound != self.bound {
            self.group = self
                .binding
                .frame_group(ctx.gpu, atlas.view(), &self.clips.buffer);
            self.bound = bound;
        }
    }

    /// The group the main pass binds.
    pub(crate) const fn group(&self) -> &wgpu::BindGroup {
        &self.group
    }
}
