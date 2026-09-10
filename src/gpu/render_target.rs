//! The texture a frame is rendered into, and the one fact about it that
//! travels outside this module.

use glam::UVec2;

/// A texture Palantir renders a frame into.
///
/// The handle every target arrives as: a window's acquired swapchain frame,
/// and the texture an [`OffscreenHost`](crate::OffscreenHost) caller supplies.
/// Borrowed rather than owned, because a swapchain frame lives only until it
/// is presented.
///
/// This is the seam an application meets. Palantir's host and driver code
/// passes it around without naming a graphics-API type, and the [`From`] impl
/// below is where a caller that owns its own device hands one in — every
/// entry point that takes a target takes an `impl Into<RenderTarget>`, so a
/// `&wgpu::Texture` goes straight in.
#[derive(Clone, Copy, Debug)]
pub struct RenderTarget<'a> {
    texture: &'a wgpu::Texture,
}

impl<'a> RenderTarget<'a> {
    pub(crate) fn size(self) -> UVec2 {
        let size = self.texture.size();
        UVec2::new(size.width, size.height)
    }

    pub(crate) fn format(self) -> TargetFormat {
        TargetFormat::from(self.texture.format())
    }

    pub(crate) fn texture(self) -> &'a wgpu::Texture {
        self.texture
    }
}

/// Render into `texture`.
///
/// It must carry `RENDER_ATTACHMENT`, and `COPY_DST` as well when the host
/// presents through its backbuffer.
impl<'a> From<&'a wgpu::Texture> for RenderTarget<'a> {
    fn from(texture: &'a wgpu::Texture) -> Self {
        Self { texture }
    }
}

/// The texel format of a [`RenderTarget`].
///
/// Opaque on purpose. Outside this module a format is compared and keyed on —
/// a change of it rebuilds the pipelines and invalidates what the driver
/// retained — and never read for what it means.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetFormat(wgpu::TextureFormat);

impl TargetFormat {
    pub(crate) fn get(self) -> wgpu::TextureFormat {
        self.0
    }
}

/// Name a format directly, for a caller that has one in hand before it has a
/// texture to read it off.
impl From<wgpu::TextureFormat> for TargetFormat {
    fn from(format: wgpu::TextureFormat) -> Self {
        Self(format)
    }
}

/// A render-target extent in the driver's own type. Depth is always one:
/// every target Palantir draws into is a 2D texture.
pub(crate) fn extent(size: UVec2) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: size.x,
        height: size.y,
        depth_or_array_layers: 1,
    }
}
