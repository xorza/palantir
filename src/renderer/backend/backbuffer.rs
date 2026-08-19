//! The off-screen colour target the backbuffer-copy path renders into.

/// Persistent off-screen *color* target for the backbuffer-copy path: the
/// frontend renders into it, then [`WgpuBackend::submit`] copies it onto the
/// caller's surface. Keeping last frame's pixels in a texture *we* own is what
/// lets `LoadOp::Load` work for incremental damage — a fresh or rotating
/// surface texture can't be relied on. The direct-present path skips the
/// backbuffer entirely and renders straight into the surface.
///
/// Sized to match the surface texture; recreated on resize or format change.
/// Owned per-window by `WindowDriver`; the backend is otherwise
/// window-agnostic.
#[derive(Debug)]
pub(crate) struct Backbuffer {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Backbuffer {
    /// Whether this backbuffer is the one a target of `size` and `format`
    /// wants — the question `ensure_backbuffer` asks before recreating and
    /// the skip-copy assert asks before copying.
    ///
    /// Format is half of it: the per-window backbuffer carries one surface's
    /// pixels, and a format flip (window moved to an HDR output) needs a fresh
    /// texture at the new format to match this submit's pipeline set.
    /// Private, so [`WgpuBackend::ensure_backbuffer`] is the only way to
    /// one — it is what holds the "matches the surface" invariant that
    /// [`Self::describes`] checks.
    ///
    /// [`WgpuBackend::ensure_backbuffer`]:
    ///     crate::renderer::backend::WgpuBackend::ensure_backbuffer
    pub(super) fn new(
        device: &wgpu::Device,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
    ) -> Self {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.backbuffer"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Self {
            view: tex.create_view(&wgpu::TextureViewDescriptor::default()),
            tex,
        }
    }

    /// The colour attachment to render into.
    pub(super) fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The texture behind [`Self::view`], for the copy onto the surface.
    pub(super) fn texture(&self) -> &wgpu::Texture {
        &self.tex
    }

    pub(super) fn describes(&self, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> bool {
        self.tex.size() == size && self.tex.format() == format
    }
}
