//! [`StagingTarget`]: the small render target a device whose passes move their whole target draws
//! a frame's writes to its pages into, which copies then take onto the pages.

use crate::common::expiry_window::ExpiryWindow;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::decaying_texture::DecayingTexture;
use crate::gpu::texel::{self, TargetSpec};
use crate::primitives::geometry::urect::URect;
use glam::UVec2;

/// A render target a frame's small writes to large textures land in first, each copied onto its
/// texture after, where a device's passes load and store their whole target
/// ([`PassTraffic::WholeTarget`](crate::gpu::device::pass_traffic::PassTraffic::WholeTarget)): a
/// pass over a page for a few of its texels loads and stores all of them, 2.2 to 5.1 ms on V3D at
/// 2048², where a copy takes only its rectangle, as the drains' moves copy there
/// (`src/gpu/AGENTS.md`, Atlases). Every pass over it clears it, as each write covers the texels it
/// copies. It grows to a frame's need and steps down to the peak of an expiry window, going where
/// none drew into it (`src/gpu/AGENTS.md`, GPU memory). Refused: a render area limited to the
/// written texels, which wgpu does not expose; a compute pass storing the texels, a second form of
/// each draw and storage on every page.
#[derive(Debug)]
pub(crate) struct StagingTarget {
    label: &'static str,
    format: wgpu::TextureFormat,
    target: DecayingTexture<Target>,
    window: ExpiryWindow,
}

/// The texture and the view a pass draws into.
#[derive(Debug)]
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl StagingTarget {
    /// No texture yet, of `format`, named `label`.
    pub(crate) const fn new(label: &'static str, format: wgpu::TextureFormat) -> Self {
        Self {
            label,
            format,
            target: DecayingTexture::new(),
            window: ExpiryWindow::NEW,
        }
    }

    /// Hold at least `size` texels, made again larger, each edge rounded up to a granule and held
    /// to the device's edge, where it holds less.
    pub(crate) fn reserve(&mut self, gpu: &Gpu, size: UVec2) {
        let (label, format) = (self.label, self.format);
        let ceiling = gpu.max_texture_dimension().get();
        self.target
            .reserve(size, ceiling, |size| Target::new(gpu, label, format, size));
    }

    /// The texels it holds, none before its first [`Self::reserve`].
    pub(crate) fn size(&self) -> UVec2 {
        self.target.extent()
    }

    /// Begin a pass over it on `encoder`, which clears it.
    pub(crate) fn begin_pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
    ) -> wgpu::RenderPass<'e> {
        let target = self
            .target
            .get()
            .expect("a staging is reserved before a pass");
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(self.label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        })
    }

    /// Copy its texels `from` onto `to` at `at`, on `encoder`, after the pass that drew them.
    pub(crate) fn copy_to(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        from: URect,
        to: &wgpu::Texture,
        at: UVec2,
    ) {
        let target = self
            .target
            .get()
            .expect("a staging is reserved before a copy");
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &target.texture,
                mip_level: 0,
                origin: texel::origin(from.min),
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: to,
                mip_level: 0,
                origin: texel::origin(at),
                aspect: wgpu::TextureAspect::All,
            },
            texel::extent(from.size),
        );
    }

    /// At the end of the expiry window display frame `frame` closes, make the texture again at the
    /// window's peak, which each [`Self::reserve`] notes, where it holds more, or drop it where no
    /// frame drew into it.
    pub(crate) fn decay(&mut self, gpu: &Gpu, frame: u64) {
        if !self.window.ends(frame) {
            return;
        }
        let (label, format) = (self.label, self.format);
        let ceiling = gpu.max_texture_dimension().get();
        self.target
            .step_down(ceiling, |size| Target::new(gpu, label, format, size));
    }
}

impl Target {
    /// A texture of `size` in `format` a pass draws into and a copy reads, named `label`.
    fn new(gpu: &Gpu, label: &'static str, format: wgpu::TextureFormat, size: UVec2) -> Self {
        let texture = TargetSpec {
            label,
            size,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        }
        .texture(gpu);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self { texture, view }
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::resource::staging_target::StagingTarget;

    impl StagingTarget {
        /// Its texture, `None` before its first reserve or after a window that drew into it not.
        pub(crate) fn texture(&self) -> Option<&wgpu::Texture> {
            self.target.get().map(|target| &target.texture)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::expiry_window::EXPIRY_FRAMES;
    use crate::gpu::resource::staging_target::StagingTarget;
    use crate::gpu::test_gpu::headless_test_gpu;
    use glam::UVec2;

    /// A staging grows in granules to what a frame draws into, keeps its texture through a window
    /// whose peak it holds already, steps down at the end of one whose peak it holds more than,
    /// and goes after one that drew nothing: 300 × 40 takes 512 × 256; a window of 600 × 40 grows
    /// it to 768 × 256 at once; a window whose peak is 300 × 40 again makes it 512 × 256 at its
    /// end, and the next, of the same peak, keeps that texture; a window of none drops it.
    #[test]
    fn a_staging_grows_to_need_and_steps_down_to_a_window_s_peak() {
        let gpu = headless_test_gpu();
        let mut staging = StagingTarget::new("test.staging", wgpu::TextureFormat::R8Unorm);
        assert_eq!(staging.size(), UVec2::ZERO);
        let small = UVec2::new(300, 40);
        let wide = UVec2::new(600, 40);
        let mut frame = 0;
        let mut window = |staging: &mut StagingTarget, need: UVec2| {
            for _ in 0..EXPIRY_FRAMES {
                staging.reserve(&gpu, need);
                staging.decay(&gpu, frame);
                frame += 1;
            }
        };
        staging.reserve(&gpu, small);
        assert_eq!(staging.size(), UVec2::new(512, 256));
        window(&mut staging, wide);
        assert_eq!(staging.size(), UVec2::new(768, 256));
        window(&mut staging, small);
        assert_eq!(staging.size(), UVec2::new(512, 256), "stepped down");
        let kept = staging.texture().cloned();
        window(&mut staging, small);
        assert_eq!(
            staging.texture().cloned(),
            kept,
            "a window at its size keeps it"
        );
        window(&mut staging, UVec2::ZERO);
        assert!(staging.texture().is_none(), "a window of none drops it");
    }
}
