mod quad_pipeline;
mod schedule;

use self::quad_pipeline::QuadPipeline;
use self::schedule::{RenderStep, for_each_step};
use super::frontend::FrameOutput;
use crate::primitives::{
    color::Color, rect::Rect, size::Size, spacing::Spacing, stroke::Stroke, urect::URect,
};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::RenderBuffer;
use crate::text::TextShaper;
use crate::ui::damage::DamagePaint;
use crate::ui::debug_overlay::DebugOverlayConfig;

/// Pad the damage scissor by this many physical pixels on every
/// side. Quads and glyphs may anti-alias slightly outside their
/// nominal rect (SDF rounded-rect AA, italic descenders); without
/// padding the scissor would clip the AA fringe and leave a
/// 1-px-hard edge along the damage boundary.
const DAMAGE_AA_PADDING: u32 = 2;

/// Stroke color for the debug damage overlay (see
/// [`crate::DebugOverlayConfig::damage_rect`]). Bright opaque red —
/// picked for contrast against any UI palette, not theme-driven.
const DAMAGE_OVERLAY_COLOR: Color = Color::rgb(1.0, 0.0, 0.0);

/// Stroke width for the debug damage overlay, in logical pixels.
/// Multiplied by `scale_factor` at submit time.
const DAMAGE_OVERLAY_STROKE_WIDTH: f32 = 2.0;

/// How far the overlay rect is inset from the damage rect, in logical
/// pixels. Centers the stroke fully inside the highlighted region.
const DAMAGE_OVERLAY_INSET: f32 = 1.0;

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
    /// Lazy stencil attachment, allocated on first frame with rounded
    /// clipping (`FrameOutput::has_rounded_clip == true`). Apps that
    /// never use rounded clip never allocate this. Recreated alongside
    /// the color texture on resize / format change.
    stencil: Option<StencilAttachment>,
}

struct StencilAttachment {
    #[allow(dead_code)] // owns the GPU resource that `view` points into
    tex: wgpu::Texture,
    view: wgpu::TextureView,
}

/// Format used for the lazy stencil attachment. `Stencil8` is the
/// minimum that satisfies the rounded-clip mask path; no depth
/// component is needed (UI is 2D, no z-test). Read by the
/// stencil-aware quad pipeline variants in `quad_pipeline.rs`.
pub(crate) const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Stencil8;

/// Stencil-test pipeline state shared by the quad's `stencil_test`
/// pipeline and glyphon's stencil-aware text renderer. Sole source of
/// truth so the two can't drift (mismatched `read_mask` etc. would
/// silently break rounded text under mask).
pub(crate) fn stencil_test_state() -> wgpu::DepthStencilState {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Keep,
    };
    wgpu::DepthStencilState {
        format: STENCIL_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::Always),
        stencil: wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: 0x00,
        },
        bias: wgpu::DepthBiasState::default(),
    }
}

/// wgpu backend: owns the quad pipeline + text renderer and cloned
/// device/queue handles (cheap, Arc-backed). The text side holds a shared
/// handle to the same `CosmicMeasure` the Ui side measures against (set via
/// [`Self::set_text_shaper`]) — without it, text rendering is silently skipped.
/// No layout, no encode, no compose — those happen elsewhere and arrive
/// here as a `RenderBuffer`.
pub struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadPipeline,
    text: TextRenderer,
    /// Color format the quad pipeline + text atlas were built for.
    /// Fixed at [`Self::new`]; [`Self::ensure_backbuffer`] hard-asserts
    /// that the swapchain texture handed to `submit` keeps this format
    /// across the backend's lifetime. Format change requires
    /// recreating the backend — partial in-place rebuild (atlas only,
    /// quad pipeline left stale) was previously possible and would
    /// silently mis-render quads. We'd rather fail loudly until a
    /// real format-flip use case shows up and we wire the full
    /// rebuild path.
    color_format: wgpu::TextureFormat,
    /// Persistent off-screen render target; lazily created on first
    /// submit and recreated when the surface size or format changes.
    /// Stage 3 / Step 6 of the damage-rendering plan: we render here
    /// so future frames can `LoadOp::Load` last frame's pixels.
    backbuffer: Option<Backbuffer>,
    /// Retained scratch for the per-frame stencil-mask sweep. `Some(j)`
    /// at index `i` says "group `i`'s mask is mask quad `j` in the
    /// upload buffer". Sized to `buffer.groups.len()` each frame; capacity
    /// retained across frames so steady-state runs alloc-free.
    mask_indices: Vec<Option<u32>>,
    /// Retained scratch for stencil-mask quads. One entry per rounded-clip
    /// group, uploaded via [`QuadPipeline::upload_masks`]. Cleared at the
    /// start of each stencil frame; capacity retained.
    masks: Vec<Quad>,
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
            color_format: format,
            backbuffer: None,
            mask_indices: Vec::new(),
            masks: Vec::new(),
        }
    }

    /// Install the shared shaper handle. Pass the same [`TextShaper`]
    /// to [`crate::Ui::set_text_shaper`] so layout-time measurement
    /// and rasterization see one buffer cache. Without it, text
    /// rendering is silently skipped at submit time.
    pub fn set_text_shaper(&mut self, shaper: TextShaper) {
        self.text.set_shaper(shaper);
    }

    /// Lazily (re)create the backbuffer to match the surface texture's
    /// size. Returns `true` if the backbuffer was just (re)created —
    /// caller treats that as a forced full repaint (the new texture's
    /// contents are undefined until the first pass writes to it).
    /// Hard-asserts that the swapchain format hasn't changed since
    /// construction; see [`Self::color_format`].
    fn ensure_backbuffer(&mut self, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> bool {
        assert_eq!(
            self.color_format, format,
            "WgpuBackend was built for surface format {:?}; got {:?} this submit. \
             Mid-session format change isn't yet supported (quad pipeline + text \
             atlas were built against the original format). Recreate the \
             WgpuBackend with the new format.",
            self.color_format, format,
        );
        let needs_new = match &self.backbuffer {
            None => true,
            Some(b) => b.tex.size() != size,
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
            // Drop any stale stencil — it was sized to the old
            // backbuffer; `ensure_stencil` lazily allocates a fresh
            // one matching the new size on the next rounded-clip
            // frame. Without this, wgpu validation rejects the pass
            // (mismatched attachment sizes).
            stencil: None,
        });
        true
    }

    /// Allocate the stencil attachment if it isn't already present.
    /// `ensure_backbuffer` resets `stencil` to `None` whenever it
    /// rebuilds the color texture, so a `Some` here is always
    /// size-matched to the current backbuffer.
    fn ensure_stencil(&mut self) {
        let bb = self
            .backbuffer
            .as_mut()
            .expect("ensure_backbuffer must run first");
        if bb.stencil.is_some() {
            return;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palantir.renderer.stencil"),
            size: bb.tex.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: STENCIL_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        bb.stencil = Some(StencilAttachment { tex, view });
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
    /// Three damage paths, branching on `frame.damage`:
    ///
    /// - [`DamagePaint::Full`]: `LoadOp::Clear(clear)` + paint every
    ///   group at its native scissor. First frame, post-resize, post-
    ///   format-change, and area-over-threshold all land here.
    /// - [`DamagePaint::Partial(rect)`][DamagePaint::Partial]:
    ///   `LoadOp::Load` (preserves last frame) + intersects every
    ///   group's scissor with the damage rect. Logical-px in;
    ///   the backend pads for AA bleed and clamps to surface.
    /// - [`DamagePaint::Skip`]: render pass is skipped entirely.
    ///   The persistent backbuffer already holds last frame's pixels,
    ///   so submit just copies it to the swapchain texture and returns.
    ///
    /// A `Partial` rect that clamps to zero physical-px area
    /// degrades to "loaded but not drawn" inside the pass.
    pub fn submit(&mut self, surface_tex: &wgpu::Texture, clear: Color, frame: FrameOutput<'_>) {
        let buffer = frame.buffer;
        let damage = frame.damage;
        let use_stencil = buffer.has_rounded_clip;
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.viewport_phys,
            ?damage,
            rounded_clip = use_stencil,
            "wgpu_backend.submit"
        );

        // Match backbuffer to the swapchain texture. A freshly
        // (re)created backbuffer has undefined contents, so any
        // requested Partial / Skip must escalate to a full clear+paint
        // this frame.
        let backbuffer_recreated = self.ensure_backbuffer(surface_tex.size(), surface_tex.format());
        let damage = if backbuffer_recreated {
            DamagePaint::Full
        } else {
            damage
        };

        // Skip: nothing changed and the backbuffer already holds the
        // right pixels. Bypass the render pass entirely and just copy
        // backbuffer → swapchain so something gets presented.
        if let DamagePaint::Skip = damage {
            self.copy_backbuffer_to_surface(surface_tex);
            return;
        }

        // Convert the logical damage rect (Partial only) to a
        // physical-px scissor, padded for AA bleed and clamped to the
        // surface. `Full` skips this and paints the whole viewport.
        let damage_scissor = match damage {
            DamagePaint::Partial(r) => {
                let phys = r.scaled_by(buffer.scale, true);
                let pad = DAMAGE_AA_PADDING as f32;
                let mins_x = (phys.min.x - pad).max(0.0) as u32;
                let mins_y = (phys.min.y - pad).max(0.0) as u32;
                let maxs_x =
                    ((phys.min.x + phys.size.w + pad).max(0.0) as u32).min(buffer.viewport_phys.x);
                let maxs_y =
                    ((phys.min.y + phys.size.h + pad).max(0.0) as u32).min(buffer.viewport_phys.y);
                if maxs_x > mins_x && maxs_y > mins_y {
                    Some(URect::new(mins_x, mins_y, maxs_x - mins_x, maxs_y - mins_y))
                } else {
                    None
                }
            }
            DamagePaint::Full => None,
            DamagePaint::Skip => unreachable!("handled above"),
        };
        let clear_op = wgpu::LoadOp::Clear(wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        });
        let force_clear = frame.debug_overlay.is_some_and(|c| c.clear_damage);
        let load_op = if damage_scissor.is_some() && !force_clear {
            wgpu::LoadOp::Load
        } else {
            clear_op
        };

        // Stencil path activates whenever the encoded frame contains a
        // `PushClipRounded`. Lazy-init the stencil texture + pipeline
        // variants the first time we land here; thereafter both stay
        // warm. Apps that never round-clip never enter this branch.
        let text_mode = if use_stencil {
            text::StencilMode::Stencil
        } else {
            text::StencilMode::Plain
        };
        // One mask quad per group whose `rounded_clip.is_some()`. The
        // map `mask_indices[i] = Some(j)` says "group i's mask is mask
        // quad j in the upload buffer". `None` = no mask (plain
        // scissor or non-stencil path). Both vecs are retained scratch
        // — cleared+filled each frame, capacity reused.
        self.mask_indices.clear();
        self.masks.clear();
        if use_stencil {
            self.ensure_stencil();
            self.quad.ensure_stencil(&self.device);
            self.mask_indices.resize(buffer.groups.len(), None);
            for (i, g) in buffer.groups.iter().enumerate() {
                if let (Some(scissor), Some(radius)) = (g.scissor, g.rounded_clip) {
                    self.mask_indices[i] = Some(self.masks.len() as u32);
                    self.masks
                        .push(QuadPipeline::mask_instance(scissor, radius));
                }
            }
            self.quad
                .upload_masks(&self.device, &self.queue, &self.masks);
        }

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

        if damage_scissor.is_some() {
            self.quad
                .upload_clear(&self.queue, buffer.viewport_phys_f, clear);
        }

        // Prepare text per-group outside the encoder/pass borrow scope so
        // glyphon can upload to the atlas + per-renderer vertex buffer
        // freely. Viewport uniform updated once for all renderers in the
        // pool — they share the atlas-bound pipeline + viewport state.
        // `prepare_group` returns `false` (no-op) when no shaper has
        // been installed via [`Self::set_text_shaper`], so the loop is
        // safe to run unconditionally.
        self.text.update_viewport(&self.queue, buffer.viewport_phys);
        for (i, g) in buffer.groups.iter().enumerate() {
            if g.texts.len == 0 {
                continue;
            }
            let runs = &buffer.texts[g.texts.range()];
            self.text
                .prepare_group(&self.device, &self.queue, buffer.scale, i, runs, text_mode);
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.main"),
            });
        {
            // Stencil attachment is built around `&backbuffer`, so its
            // lifetime needs to outlive the pass — extract before the
            // descriptor block.
            let stencil_view = if use_stencil {
                Some(
                    &backbuffer
                        .stencil
                        .as_ref()
                        .expect("ensure_stencil populated this")
                        .view,
                )
            } else {
                None
            };
            let depth_stencil_attachment =
                stencil_view.map(|view| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        // Cleared every frame — stencil contents never
                        // need to survive across passes (the cmd-buffer
                        // replays mask writes on every frame regardless
                        // of cache hits).
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Discard,
                    }),
                });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.renderer.main.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &backbuffer.view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.render_groups(&mut pass, buffer, damage_scissor, use_stencil, text_mode);
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
            backbuffer.tex.size(),
        );

        if let Some(config) = frame.debug_overlay {
            self.draw_debug_overlay(surface_tex, &mut encoder, buffer, damage, config);
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        if self.text.has_prepared() {
            self.text.end_frame();
        }
    }

    /// Dispatch every step in the per-frame schedule
    /// ([`schedule::for_each_step`]) to the wgpu render pass. Logic
    /// for *what* runs in *what order* lives in the schedule module;
    /// this method is purely the wgpu translation layer for each
    /// `RenderStep`. Tests reuse the same schedule emitter to assert
    /// on the sequence without GPU.
    fn render_groups<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        buffer: &RenderBuffer,
        damage_scissor: Option<URect>,
        use_stencil: bool,
        text_mode: text::StencilMode,
    ) {
        for_each_step(
            buffer,
            damage_scissor,
            &self.mask_indices,
            use_stencil,
            |step| match step {
                RenderStep::PreClear => {
                    self.quad.draw_clear(pass, use_stencil);
                }
                RenderStep::SetScissor(r) => {
                    pass.set_scissor_rect(r.x, r.y, r.w, r.h);
                }
                RenderStep::SetStencilRef(v) => {
                    pass.set_stencil_reference(v);
                }
                RenderStep::MaskQuad(mi) => {
                    self.quad.bind_mask_write(pass);
                    self.quad.draw_mask(pass, mi);
                }
                RenderStep::Quads { range, .. } => {
                    if use_stencil {
                        self.quad.bind_stencil_test(pass);
                    } else {
                        self.quad.bind(pass);
                    }
                    self.quad.draw_range(pass, range);
                }
                RenderStep::Text { group } => {
                    self.text.render_group(group, pass, text_mode);
                }
            },
        );
    }

    /// Draw the debug overlay onto the swapchain texture *after* the
    /// backbuffer→surface copy. The overlay never lands on the
    /// backbuffer, so next frame's `LoadOp::Load` reads clean pixels
    /// and there's no ghost stroke. Each `bool` on `config` toggles a
    /// distinct visualization; the function is a no-op when all flags
    /// are off. Caller already filtered `DamagePaint::Skip`.
    fn draw_debug_overlay(
        &mut self,
        surface_tex: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &RenderBuffer,
        damage: DamagePaint,
        config: DebugOverlayConfig,
    ) {
        if config.damage_rect {
            let damage_rect_phys = match damage {
                DamagePaint::Partial(r) => r.scaled_by(buffer.scale, true),
                DamagePaint::Full => Rect {
                    min: glam::Vec2::ZERO,
                    size: Size::new(buffer.viewport_phys_f.x, buffer.viewport_phys_f.y),
                },
                DamagePaint::Skip => unreachable!("Skip filtered before draw_debug_overlay"),
            };
            let inset_px = (DAMAGE_OVERLAY_INSET * buffer.scale).max(1.0);
            let overlay_rect = damage_rect_phys.deflated_by(Spacing::all(inset_px));
            let stroke = Stroke {
                width: DAMAGE_OVERLAY_STROKE_WIDTH * buffer.scale,
                color: DAMAGE_OVERLAY_COLOR,
            };
            self.quad.upload_overlay(&self.queue, overlay_rect, stroke);
            let surface_view = surface_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palantir.renderer.overlay.damage_rect"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.quad.draw_overlay(&mut pass);
        }
    }

    /// Copy the persistent backbuffer onto the swapchain texture
    /// without running a render pass. Used on `DamagePaint::Skip`
    /// frames: the backbuffer already holds last frame's pixels and
    /// nothing changed, so we just need something on screen.
    fn copy_backbuffer_to_surface(&self, surface_tex: &wgpu::Texture) {
        let backbuffer = self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded");
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("palantir.renderer.skip"),
            });
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
            backbuffer.tex.size(),
        );
        self.queue.submit(std::iter::once(encoder.finish()));
    }
}

#[cfg(test)]
mod tests;
