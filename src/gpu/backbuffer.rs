//! The off-screen colour target the backbuffer-copy path renders into.

use crate::gpu::render_target::{self, TargetFormat};
/// Persistent off-screen *color* target for the backbuffer-copy path: the
/// frontend renders into it, then [`WgpuBackend::submit`](crate::gpu::WgpuBackend::submit) copies it onto the
/// caller's surface. Keeping last frame's pixels in a texture *we* own is what
/// lets `LoadOp::Load` work for incremental damage — a fresh or rotating
/// surface texture can't be relied on. The direct-present path skips the
/// backbuffer entirely and renders straight into the surface.
///
/// Sized to match the surface texture; recreated on resize or format change.
/// Owned per-window by `WindowDriver`; the backend is otherwise
/// window-agnostic.
use glam::UVec2;

use crate::gpu::WgpuBackend;
#[derive(Debug)]
pub(crate) struct Backbuffer {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    /// Built once with the texture, because a target that cannot be copied
    /// into needs this every frame it presents — minting one per frame would
    /// put an allocation on the paint path.
    bind_group: wgpu::BindGroup,
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
    /// the same contract [`Stencil::ensure`](crate::gpu::stencil::Stencil::ensure)
    /// offers. The `format` is the per-window surface format; the
    /// matching pipeline set is fetched per submit from the backend's
    /// `pipelines` map, so no global-format assert is needed.
    pub(crate) fn ensure<'s>(
        slot: &'s mut Option<Self>,
        backend: &WgpuBackend,
        size: UVec2,
        format: TargetFormat,
    ) -> EnsuredBackbuffer<'s> {
        let size = render_target::extent(size);
        let format = format.get();
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
            backbuffer: slot.get_or_insert_with(|| Self::new(backend, size, format)),
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
    fn new(backend: &WgpuBackend, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> Self {
        let tex = backend.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.backbuffer"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            // `TEXTURE_BINDING` is for the targets that take no copy: there
            // the backbuffer is sampled and drawn rather than copied.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = backend.backbuffer_bind_group(&view);
        Self {
            tex,
            view,
            bind_group,
        }
    }

    /// Draw this backbuffer onto a target that cannot be copied into.
    ///
    /// The peer of [`Self::copy_onto`], reaching the same pixels through the
    /// one usage every surface offers. A pass of its own, after the frame's
    /// draws: it replaces the target rather than compositing onto it, so it
    /// must not share a pass with anything that blends.
    pub(super) fn draw_onto(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.renderer.blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // Clear rather than Load although the triangle writes
                    // every texel, which is what lets a rotating swapchain
                    // image hold anything at all: on the tilers this path
                    // exists for, a clear skips reading the tile memory in.
                    // `DontCare` skips even that, and asks for an unsafe
                    // contract in return for one flag.
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
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
