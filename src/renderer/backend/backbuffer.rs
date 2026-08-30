//! The off-screen colour target the backbuffer-copy path renders into.

/// Persistent off-screen *color* target for the backbuffer-copy path: the
/// frontend renders into it, then [`WgpuBackend::submit`](crate::renderer::backend::WgpuBackend::submit) copies it onto the
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

/// What [`Backbuffer::ensure`] hands back: the window's backbuffer, and
/// whether that call had to build a fresh one.
#[derive(Debug)]
pub(crate) struct EnsuredBackbuffer<'a> {
    pub(crate) backbuffer: &'a Backbuffer,
    /// A fresh texture's contents are undefined until the first pass
    /// writes them, so a recreate obliges the caller to a `Full` damage
    /// plan. Every upstream cause of one — a size change, a format flip,
    /// a first frame — forces `Full` before the draw list builds, so the
    /// caller asserts this rather than acting on it.
    pub(crate) recreated: bool,
}

impl Backbuffer {
    /// The window's backbuffer at `size` and `format`, building it if the
    /// slot is empty or holds one that no longer
    /// [`describes`](Self::describes) the target.
    ///
    /// Hands the attachment back rather than only filling the slot, so
    /// the caller does not re-read its own `Option` behind an `expect` —
    /// the same contract [`Stencil::ensure`](crate::renderer::backend::stencil::Stencil::ensure)
    /// offers. The `format` is the per-window surface format; the
    /// matching pipeline set is fetched per submit from the backend's
    /// `pipelines` map, so no global-format assert is needed.
    pub(crate) fn ensure<'s>(
        slot: &'s mut Option<Self>,
        device: &wgpu::Device,
        size: wgpu::Extent3d,
        format: wgpu::TextureFormat,
    ) -> EnsuredBackbuffer<'s> {
        // Drop a stale one first, then a plain get-or-insert: the two
        // steps are what let this hand back a `&Backbuffer` without an
        // `expect` re-reading the slot it just filled.
        if slot
            .as_ref()
            .is_some_and(|held| !held.describes(size, format))
        {
            *slot = None;
        }
        let recreated = slot.is_none();
        EnsuredBackbuffer {
            backbuffer: slot.get_or_insert_with(|| Self::new(device, size, format)),
            recreated,
        }
    }

    /// Copy this backbuffer's pixels onto `surface_tex`. The caller's
    /// surface must have `COPY_DST` usage (set in
    /// [`wgpu::SurfaceConfiguration::usage`]), and must
    /// [`describe`](Self::describes) this backbuffer — copying a
    /// mismatched target would present undefined or stale-format pixels.
    pub(super) fn copy_onto(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_tex: &wgpu::Texture,
    ) {
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: surface_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.tex.size(),
        );
    }

    /// Private, so [`Self::ensure`] is the only way to one — it is what
    /// holds the "matches the surface" invariant [`Self::describes`]
    /// checks.
    fn new(device: &wgpu::Device, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> Self {
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

    /// Whether this backbuffer is the one a target of `size` and
    /// `format` wants — the question [`Self::ensure`] asks before
    /// recreating and the skip-copy assert asks before copying.
    ///
    /// Format is half of it: the per-window backbuffer carries one
    /// surface's pixels, and a format flip (window moved to an HDR
    /// output) needs a fresh texture at the new format to match this
    /// submit's pipeline set.
    pub(super) fn describes(&self, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> bool {
        self.tex.size() == size && self.tex.format() == format
    }
}
