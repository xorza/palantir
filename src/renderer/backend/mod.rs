use super::buffer::{RenderBuffer, ScissorRect};
use super::quad::QuadPipeline;
use crate::primitives::{Color, Rect};
use crate::text::SharedCosmic;

/// Pad the damage scissor by this many physical pixels on every
/// side. Quads and glyphs may anti-alias slightly outside their
/// nominal rect (SDF rounded-rect AA, italic descenders); without
/// padding the scissor would clip the AA fringe and leave a
/// 1-px-hard edge along the damage boundary.
const DAMAGE_AA_PADDING: u32 = 2;

mod text;
use text::TextRenderer;

/// Persistent off-screen target that the render pass paints into.
/// We render to this texture (not to the swapchain view directly)
/// so we can keep last frame's pixels around between frames —
/// `LoadOp::Load` only works reliably on a texture *we* own; the
/// swapchain's preserve-contents behaviour varies by platform/
/// present-mode. After the pass, [`WgpuBackend::submit`] copies
/// the backbuffer into the swapchain texture and presents.
///
/// Sized to match the surface texture; recreated on resize or
/// format change.
struct Backbuffer {
    tex: wgpu::Texture,
    view: wgpu::TextureView,
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
}

/// wgpu backend: owns the quad pipeline + text renderer and cloned
/// device/queue handles (cheap, Arc-backed). The text side holds a shared
/// handle to the same `CosmicMeasure` the Ui side measures against (set via
/// [`Self::set_cosmic`]) — without it, text rendering is silently skipped.
/// No layout, no encode, no compose — those happen elsewhere and arrive
/// here as a `RenderBuffer`.
pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadPipeline,
    text: TextRenderer,
    /// Persistent off-screen render target; lazily created on first
    /// submit and recreated when the surface size or format changes.
    /// Stage 3 / Step 6 of the damage-rendering plan: we render here
    /// so future frames can `LoadOp::Load` last frame's pixels.
    backbuffer: Option<Backbuffer>,
}

impl WgpuBackend {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let quad = QuadPipeline::new(&device, format);
        let text = TextRenderer::new(&device, &queue, format);
        Self {
            device,
            queue,
            quad,
            text,
            backbuffer: None,
        }
    }

    /// Lazily (re)create the backbuffer to match the surface texture's
    /// size and format. Returns `true` if the backbuffer was just
    /// (re)created — caller treats that as a forced full repaint
    /// (the new texture's contents are undefined until the first pass
    /// writes to it).
    fn ensure_backbuffer(&mut self, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> bool {
        let needs_new = match &self.backbuffer {
            None => true,
            Some(b) => b.size != size || b.format != format,
        };
        if !needs_new {
            return false;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.backbuffer"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.backbuffer = Some(Backbuffer {
            tex,
            view,
            size,
            format,
        });
        true
    }

    /// Install the shared shaper handle. Pass the same `SharedCosmic` to
    /// [`crate::Ui::set_cosmic`] so layout and rendering see one cache.
    pub fn set_cosmic(&mut self, cosmic: SharedCosmic) {
        self.text.set_cosmic(cosmic);
    }

    /// Render one frame to the persistent backbuffer, then copy the
    /// backbuffer onto the swapchain texture. The caller's surface
    /// texture must have `COPY_DST` usage (set in
    /// [`wgpu::SurfaceConfiguration::usage`]).
    ///
    /// Without a shared shaper installed (mono fallback) text rendering
    /// is silently skipped; the frame still draws quads.
    ///
    /// Quads and text interleave per-group in paint order: each group's
    /// quads draw first, then its text renders on top, before the next
    /// group runs. So a child quad declared *after* a label correctly
    /// occludes that label.
    ///
    /// Step 6 of the damage-rendering plan. `damage`, when `Some`,
    /// switches the pass to `LoadOp::Load` (last frame's pixels stay) and
    /// intersects every group's scissor with the damage rect, so only the
    /// dirty region is repainted. `None` ⇒ `LoadOp::Clear` + paint normally
    /// (used on the first frame, after resize, and when the dirty area
    /// exceeds the heuristic threshold).
    ///
    /// `damage` is in *logical* pixels matching `buffer.scale`; the
    /// backend handles physical-px conversion, AA padding, and
    /// surface-bounds clamping. A degenerate damage rect that
    /// clamps to zero area is treated as "nothing to paint" — pass
    /// returns having loaded but not drawn.
    pub fn submit(
        &mut self,
        surface_tex: &wgpu::Texture,
        clear: Color,
        buffer: &RenderBuffer,
        damage: Option<Rect>,
    ) {
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.viewport_phys,
            ?damage,
            "wgpu_backend.submit"
        );

        // Match backbuffer to the swapchain texture; a freshly
        // (re)created backbuffer has undefined contents, so any
        // requested partial repaint must escalate to a full clear
        // this frame. Same for the very first frame after backend
        // construction.
        let backbuffer_recreated = self.ensure_backbuffer(surface_tex.size(), surface_tex.format());
        let damage = if backbuffer_recreated { None } else { damage };

        // Convert the logical damage rect to a physical-px scissor,
        // padded for AA bleed and clamped to the surface. `None`
        // here means "full repaint" (clear + no scissor override).
        let damage_scissor = damage.and_then(|r| {
            let phys = r.scaled_by(buffer.scale, true);
            let mins_x = (phys.min.x as i64 - DAMAGE_AA_PADDING as i64).max(0) as u32;
            let mins_y = (phys.min.y as i64 - DAMAGE_AA_PADDING as i64).max(0) as u32;
            let maxs_x = ((phys.min.x + phys.size.w) as u32 + DAMAGE_AA_PADDING)
                .min(buffer.viewport_phys[0]);
            let maxs_y = ((phys.min.y + phys.size.h) as u32 + DAMAGE_AA_PADDING)
                .min(buffer.viewport_phys[1]);
            if maxs_x > mins_x && maxs_y > mins_y {
                Some(ScissorRect {
                    x: mins_x,
                    y: mins_y,
                    w: maxs_x - mins_x,
                    h: maxs_y - mins_y,
                })
            } else {
                None
            }
        });
        let load_op = if damage_scissor.is_some() {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(wgpu::Color {
                r: clear.r as f64,
                g: clear.g as f64,
                b: clear.b as f64,
                a: clear.a as f64,
            })
        };

        let backbuffer = self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded");

        self.quad.upload(
            &self.device,
            &self.queue,
            buffer.viewport_phys_f,
            &buffer.quads,
        );

        // Prepare text per-group outside the encoder/pass borrow scope so
        // glyphon can upload to the atlas + per-renderer vertex buffer
        // freely. Viewport uniform updated once for all renderers in the
        // pool — they share the atlas-bound pipeline + viewport state.
        self.text.update_viewport(&self.queue, buffer.viewport_phys);
        for (i, g) in buffer.groups.iter().enumerate() {
            if g.texts.is_empty() {
                continue;
            }
            let runs = &buffer.texts[g.texts.start as usize..g.texts.end as usize];
            self.text
                .prepare_group(&self.device, &self.queue, buffer.scale, i, runs);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.renderer.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &backbuffer.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let full_viewport = ScissorRect {
                x: 0,
                y: 0,
                w: buffer.viewport_phys[0],
                h: buffer.viewport_phys[1],
            };
            for (i, g) in buffer.groups.iter().enumerate() {
                let group_scissor = g.scissor.unwrap_or(full_viewport);
                // Intersect with damage when partial-repainting. If
                // the result is empty, this group has nothing to paint
                // inside the dirty region — skip it entirely.
                let effective = match damage_scissor {
                    Some(d) => match group_scissor.intersect(d) {
                        Some(r) => r,
                        None => continue,
                    },
                    None => group_scissor,
                };
                if effective.w == 0 || effective.h == 0 {
                    continue;
                }
                pass.set_scissor_rect(effective.x, effective.y, effective.w, effective.h);
                if !g.quads.is_empty() {
                    self.quad.draw_range(&mut pass, g.quads.clone());
                }
                if !g.texts.is_empty() {
                    // Text uses a full-viewport scissor + per-area `bounds`
                    // for clipping (set in compose). Under partial repaint
                    // we narrow that to the damage rect so glyph fringe
                    // outside the dirty region can't bleed in.
                    let text_scissor = match damage_scissor {
                        Some(d) => d,
                        None => full_viewport,
                    };
                    pass.set_scissor_rect(
                        text_scissor.x,
                        text_scissor.y,
                        text_scissor.w,
                        text_scissor.h,
                    );
                    self.text.render_group(i, &mut pass);
                }
            }
        }

        // Copy the just-painted backbuffer onto the swapchain texture.
        // Both share format + size (`ensure_backbuffer` enforces it),
        // so this is a single direct copy — no blit pipeline required.
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &backbuffer.tex,
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
            backbuffer.size,
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        if self.text.has_prepared() {
            self.text.end_frame();
        }
    }

    /// Re-create text atlas/renderer after a surface format change.
    pub fn surface_format_changed(&mut self, format: wgpu::TextureFormat) {
        self.text
            .rebuild_for_format(&self.device, &self.queue, format);
    }
}

#[cfg(test)]
mod tests;
