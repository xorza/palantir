//! The texture a frame renders into.

use crate::gpu::texel::{self, TargetFormat};
use glam::UVec2;
use std::borrow::Cow;

/// A texture Palantir renders a frame into; borrowed.
#[derive(Clone, Copy, Debug)]
pub struct RenderTarget<'a> {
    texture: &'a wgpu::Texture,
    /// The texture's whole view, where its owner keeps one, as an [`OwnedTarget`] does; else a
    /// frame that writes the texture makes one, as it must for a swapchain image, which lives one
    /// frame.
    ///
    /// [`OwnedTarget`]: crate::gpu::surface::owned_target::OwnedTarget
    view: Option<&'a wgpu::TextureView>,
}

impl<'a> RenderTarget<'a> {
    /// Render into `texture`; the format must be sRGB or float, usage must include `RENDER_ATTACHMENT`.
    ///
    /// # Panics
    ///
    /// Panics on a non-sRGB/float format or usage lacking `RENDER_ATTACHMENT`.
    #[track_caller]
    pub fn new(texture: &'a wgpu::Texture) -> Self {
        assert!(
            texture
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT),
            "a render target must allow RENDER_ATTACHMENT",
        );
        TargetFormat::new(texture.format());
        Self {
            texture,
            view: None,
        }
    }

    /// [`Self::new`] with `view`, the default view of `texture` its owner keeps, so a frame makes
    /// none.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[track_caller]
    pub(crate) fn with_view(texture: &'a wgpu::Texture, view: &'a wgpu::TextureView) -> Self {
        Self {
            view: Some(view),
            ..Self::new(texture)
        }
    }

    /// The texture's whole view: the one its owner keeps, else a new one.
    pub(crate) fn view(self) -> Cow<'a, wgpu::TextureView> {
        match self.view {
            Some(view) => Cow::Borrowed(view),
            None => Cow::Owned(
                self.texture
                    .create_view(&wgpu::TextureViewDescriptor::default()),
            ),
        }
    }

    pub(crate) fn size(self) -> UVec2 {
        texel::size(self.texture)
    }

    pub(crate) fn format(self) -> TargetFormat {
        TargetFormat::new(self.texture.format())
    }

    /// Whether a frame can be copied onto this target (false for a GLES swapchain image).
    pub(crate) fn takes_copy(self) -> bool {
        self.texture.usage().contains(wgpu::TextureUsages::COPY_DST)
    }

    /// Whether pixels can be copied out of this target, as a backdrop's capture is.
    pub(crate) fn gives_copy(self) -> bool {
        self.texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
    }

    pub(crate) const fn texture(self) -> &'a wgpu::Texture {
        self.texture
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::RenderTarget;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::texel::TargetFormat;
    use crate::gpu::texel::TargetSpec;
    use crate::internals::panic_probe;
    use glam::UVec2;
    use std::borrow::Cow;

    /// Panics when the format is not sRGB/float or usage lacks a render pass.
    #[test]
    fn targets_check_their_format_and_usage() {
        use wgpu::{TextureFormat as F, TextureUsages as U};

        for format in [F::Rgba8UnormSrgb, F::Bgra8UnormSrgb, F::Rgba16Float] {
            let _ = TargetFormat::new(format);
        }
        for format in [F::Rgba8Unorm, F::Bgra8Unorm, F::R8Unorm] {
            panic_probe::assert_panics_with("format must be sRGB or float", || {
                TargetFormat::new(format)
            });
        }

        let gpu = headless_test_gpu();
        let made = |format, usage| {
            let spec = TargetSpec {
                label: "target-check",
                size: UVec2::splat(4),
                format,
                usage,
            };
            OwnedTarget::new(&gpu, spec)
        };
        let good = made(F::Rgba8UnormSrgb, U::RENDER_ATTACHMENT);
        assert_eq!(good.as_target().size(), UVec2::splat(4));
        let linear = made(F::Rgba8Unorm, U::RENDER_ATTACHMENT);
        panic_probe::assert_panics_with("format must be sRGB or float", || {
            RenderTarget::new(linear.texture())
        });
        let sampled = made(F::Rgba8UnormSrgb, U::TEXTURE_BINDING);
        panic_probe::assert_panics_with("must allow RENDER_ATTACHMENT", || {
            RenderTarget::new(sampled.texture())
        });
    }

    /// An owned target lends the view it keeps, so a frame into it makes none; a bare texture's
    /// target makes one, as a swapchain image's must.
    #[test]
    fn an_owned_target_lends_its_kept_view() {
        let gpu = headless_test_gpu();
        let owned = OwnedTarget::new(
            &gpu,
            TargetSpec {
                label: "target-view",
                size: UVec2::splat(4),
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            },
        );
        match owned.as_target().view() {
            Cow::Borrowed(view) => assert_eq!(view, owned.view(), "the kept view itself"),
            Cow::Owned(_) => panic!("an owned target made a view"),
        }
        assert!(matches!(
            RenderTarget::new(owned.texture()).view(),
            Cow::Owned(_)
        ));
    }
}
