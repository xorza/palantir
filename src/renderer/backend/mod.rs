mod mesh_pipeline;
mod quad_pipeline;
mod schedule;
mod viewport;

use self::mesh_pipeline::MeshPipeline;
use self::quad_pipeline::QuadPipeline;
use self::schedule::{RenderStep, for_each_step};
use self::viewport::build_damage_scissors;
use crate::primitives::{
    color::Color, rect::Rect, size::Size, spacing::Spacing, stroke::Stroke, urect::URect,
};
use crate::renderer::frontend::FrameState;
use crate::renderer::frontend::gradient_atlas::GradientCpuAtlas;
use crate::renderer::render_buffer::RenderBuffer;
use crate::text::TextShaper;
use crate::ui::damage::DamagePaint;
use crate::ui::damage::region::DAMAGE_RECT_CAP;
use crate::ui::debug_overlay::DebugOverlayConfig;

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
/// device/queue handles (cheap, Arc-backed). The text side holds a
/// shared handle to the same `CosmicMeasure` the Ui side measures
/// against (passed in at [`Self::new`]) so layout-time measurement
/// and rasterization hit one buffer cache. No layout, no encode, no
/// compose — those happen elsewhere and arrive here as a
/// `RenderBuffer`.
pub(crate) struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadPipeline,
    mesh: MeshPipeline,
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
    /// Per-frame damage scissors. One entry per rect in
    /// `DamagePaint::Partial(region)` after physical-px scaling plus
    /// AA padding plus viewport clamping; rects that clamp to zero
    /// area are filtered out. `Full` and `Skip` leave it empty.
    /// Bounded by [`DAMAGE_RECT_CAP`] (the merge policy guarantees
    /// the region never holds more), so inline storage suffices and
    /// no heap allocation ever runs.
    damage_scissors: tinyvec::ArrayVec<[URect; DAMAGE_RECT_CAP]>,
}

impl WgpuBackend {
    pub(crate) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        shaper: TextShaper,
    ) -> Self {
        let quad = QuadPipeline::new(&device, format);
        let mesh = MeshPipeline::new(&device, format);
        let mut text = TextRenderer::new(&device, &queue, format);
        text.set_shaper(shaper);
        Self {
            device,
            queue,
            quad,
            mesh,
            text,
            color_format: format,
            backbuffer: None,
            damage_scissors: Default::default(),
        }
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
    /// - [`DamagePaint::Full`]: a single `LoadOp::Clear(clear)` pass
    ///   paints every group at its native scissor. First frame,
    ///   post-resize, post-format-change, and coverage-above-threshold
    ///   all land here.
    /// - [`DamagePaint::Partial(region)`][DamagePaint::Partial]: one
    ///   render pass per rect in the region. Each pass `LoadOp::Load`s
    ///   the backbuffer (preserving last frame outside the rect) and
    ///   the schedule narrows every group's scissor to that pass's
    ///   damage rect. Logical-px in; the backend scales, pads for AA
    ///   bleed, and clamps to surface; rects that clamp to zero area
    ///   are filtered out.
    /// - [`DamagePaint::Skip`]: no render pass at all. The persistent
    ///   backbuffer already holds last frame's pixels, so submit just
    ///   copies it to the swapchain texture and returns.
    ///
    /// A region whose every rect clamps to zero physical-px area
    /// degrades to a single `Full` pass — correct, just wasteful.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit(
        &mut self,
        surface_tex: &wgpu::Texture,
        clear: Color,
        buffer: &RenderBuffer,
        gradient_atlas: &mut GradientCpuAtlas,
        damage: DamagePaint,
        debug_overlay: Option<DebugOverlayConfig>,
        frame_state: &FrameState,
    ) {
        // Sync gradient LUT atlas to GPU. Idle frames (no new
        // gradients) drain an empty dirty flag and do nothing; first
        // frame uploads row 0's magenta fallback plus any baked rows
        // composer queued. Has to run before the render pass starts —
        // any quad with `fill_kind.is_linear()` samples this texture.
        self.quad.upload_gradients(&self.queue, gradient_atlas);

        let use_stencil = buffer.has_rounded_clip();
        tracing::trace!(
            quads = buffer.quads.len(),
            texts = buffer.texts.len(),
            groups = buffer.groups.len(),
            viewport = ?buffer.viewport_phys,
            requested_damage = ?damage,
            rounded_clip = use_stencil,
            "wgpu_backend.submit"
        );

        // Match backbuffer to the swapchain texture. A freshly
        // (re)created backbuffer has undefined contents, so any
        // requested Partial / Skip must escalate to a full clear+paint
        // this frame. `effective_damage` is what we'll actually render;
        // `damage` is what the host asked for. The two diverge only on
        // backbuffer recreate, but the debug overlay's damage-rect
        // outline shows what we *rendered*, not what was requested, so
        // threading the renamed value through is the right semantic.
        let backbuffer_recreated = self.ensure_backbuffer(surface_tex.size(), surface_tex.format());
        let effective_damage = if backbuffer_recreated {
            DamagePaint::Full
        } else {
            damage
        };

        // Skip: nothing changed and the backbuffer already holds the
        // right pixels. Bypass the render pass entirely and just copy
        // backbuffer → swapchain so something gets presented.
        if let DamagePaint::Skip = effective_damage {
            self.copy_backbuffer_to_surface(surface_tex);
            frame_state.mark_submitted();
            return;
        }

        // Build the per-frame scissor list. `Full` → empty list →
        // single Clear+full-viewport pass. `Partial` → one entry per
        // rect in the region (see `build_damage_scissors`).
        build_damage_scissors(&mut self.damage_scissors, effective_damage, buffer);
        let clear_op = wgpu::LoadOp::Clear(wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        });
        // `dim_undamaged` debug mode: every Partial frame, before any
        // damage passes, draw one full-viewport 40%-translucent black
        // quad onto the backbuffer with `LoadOp::Load`. Each frame
        // undamaged pixels are dimmed once; damaged pixels are dimmed
        // then immediately overwritten by the fresh repaint, so they
        // stay bright. Across many frames the static background fades
        // toward black while moving content stays current — far less
        // jarring than the prior `LoadOp::Clear` flash and visually
        // pins which regions are actually repainting.
        let dim_undamaged =
            debug_overlay.is_some_and(|c| c.dim_undamaged) && !self.damage_scissors.is_empty();
        if dim_undamaged {
            self.quad
                .upload_dim(&self.queue, buffer.viewport_phys_f, 0.4);
        }

        // Stencil path activates whenever the encoded frame contains a
        // `PushClipRounded`. Lazy-init the stencil texture + pipeline
        // variants the first time we land here; thereafter both stay
        // warm. Apps that never round-clip never enter this branch.
        // After staging, `self.quad.mask_indices` parallels
        // `buffer.groups` and `render_groups` reads it directly.
        let text_mode = if use_stencil {
            text::StencilMode::Stencil
        } else {
            text::StencilMode::Plain
        };
        if use_stencil {
            self.ensure_stencil();
            self.quad.ensure_stencil(&self.device);
            self.mesh.ensure_stencil(&self.device);
            self.quad
                .stage_masks(&self.device, &self.queue, &buffer.groups);
        }

        self.quad.upload(
            &self.device,
            &self.queue,
            buffer.viewport_phys_f,
            &buffer.quads,
        );

        self.mesh.upload(
            &self.device,
            &self.queue,
            buffer.viewport_phys_f,
            &buffer.meshes.arena.vertices,
            &buffer.meshes.arena.indices,
        );

        if !self.damage_scissors.is_empty() {
            self.quad
                .upload_clear(&self.queue, buffer.viewport_phys_f, clear);
        }

        // Prepare text per-group outside the encoder/pass borrow scope so
        // glyphon can upload to the atlas + per-renderer vertex buffer
        // freely. Viewport uniform updated once for all renderers in the
        // pool — they share the atlas-bound pipeline + viewport state.
        // `prepare_group` returns `false` (no-op) when the shaper
        // passed at [`Self::new`] has no installed fonts, so the loop
        // is safe to run unconditionally.
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

        // Two paths, branching on whether the frame is a Full or
        // Partial repaint:
        // - `damage_scissors` empty ⇒ Full: single pass, Clear,
        //   no scissor.
        // - `damage_scissors` non-empty ⇒ Partial: an optional dim
        //   pre-pass (`dim_undamaged`, see above) that paints a
        //   40% black quad over the full backbuffer with
        //   `LoadOp::Load`, then one pass per damage rect using
        //   `LoadOp::Load` so prior pixels (and the dim, where it
        //   landed) survive outside the scissor. Subsequent passes
        //   load the prior pass's output the same way.
        if self.damage_scissors.is_empty() {
            tracing::trace!(of = 1, "wgpu_backend.submit.pass.full");
            self.run_one_pass(&mut encoder, buffer, None, clear_op, use_stencil, text_mode);
        } else {
            if dim_undamaged {
                tracing::trace!("wgpu_backend.submit.pass.dim");
                self.run_dim_pass(&mut encoder, use_stencil);
            }
            let n = self.damage_scissors.len();
            for (i, scissor) in self.damage_scissors.iter().copied().enumerate() {
                tracing::trace!(
                    pass = i,
                    of = n,
                    ?scissor,
                    "wgpu_backend.submit.pass.partial"
                );
                self.run_one_pass(
                    &mut encoder,
                    buffer,
                    Some(scissor),
                    wgpu::LoadOp::Load,
                    use_stencil,
                    text_mode,
                );
            }
        }

        // Copy the just-painted backbuffer onto the swapchain texture.
        // Both share format + size (`ensure_backbuffer` enforces it),
        // so this is a single direct copy — no blit pipeline required.
        let backbuffer_tex = &self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded")
            .tex;
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: backbuffer_tex,
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
            backbuffer_tex.size(),
        );

        if let Some(config) = debug_overlay {
            self.draw_debug_overlay(surface_tex, &mut encoder, buffer, effective_damage, config);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        self.quad.end_frame();
        frame_state.mark_submitted();

        if self.text.has_prepared() {
            self.text.end_frame();
        }
    }

    /// Full-viewport pass that draws one 40%-translucent black quad
    /// over the backbuffer with `LoadOp::Load`. Runs before partial
    /// damage passes when the debug `dim_undamaged` flag is on (see
    /// `dim_undamaged` in [`Self::submit`]). No stencil attachment
    /// is added even when the frame uses rounded clipping — the dim
    /// quad paints uniformly across the whole viewport and the
    /// subsequent partial passes provide their own stencil setup.
    fn run_dim_pass(&self, encoder: &mut wgpu::CommandEncoder, use_stencil: bool) {
        let backbuffer = self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded");
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("palantir.renderer.dim.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &backbuffer.view,
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
        // Caller passes `use_stencil` for the frame, but this pass
        // intentionally omits the stencil attachment. The dim quad
        // is rendered through the no-stencil base pipeline, which
        // doesn't reference the stencil state. The parameter is
        // here for symmetry with `run_one_pass` and to let us pin
        // an assert if a future refactor accidentally requires it.
        let _ = use_stencil;
        self.quad.draw_dim(&mut pass);
    }

    /// Open one render pass against the backbuffer with the given
    /// load op + scissor, then dispatch the schedule into it via
    /// [`Self::render_groups`]. Stencil attachment is added when the
    /// frame has rounded clips; cleared every pass since the
    /// schedule re-stamps masks per group.
    ///
    /// The schedule's leading `PreClear` quad always fires when a
    /// damage scissor is set (Partial frames). Full-path frames pass
    /// `scissor = None`, so the schedule's PreClear emit is gated
    /// out at the source and the `LoadOp::Clear` covers the entire
    /// backbuffer instead.
    fn run_one_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &RenderBuffer,
        scissor: Option<URect>,
        load_op: wgpu::LoadOp<wgpu::Color>,
        use_stencil: bool,
        text_mode: text::StencilMode,
    ) {
        let backbuffer = self
            .backbuffer
            .as_ref()
            .expect("ensure_backbuffer just succeeded");
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
                    // Cleared every pass — stencil contents never need
                    // to survive across passes (the cmd-buffer replays
                    // mask writes on every frame regardless of cache
                    // hits).
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
        self.render_groups(&mut pass, buffer, scissor, use_stencil, text_mode);
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
            &self.quad.mask_indices,
            use_stencil,
            |step| match step {
                RenderStep::PreClear => {
                    pass.push_debug_group("preclear");
                    self.quad.draw_clear(pass, use_stencil);
                    pass.pop_debug_group();
                }
                RenderStep::SetScissor(r) => {
                    pass.set_scissor_rect(r.x, r.y, r.w, r.h);
                }
                RenderStep::SetStencilRef(v) => {
                    pass.set_stencil_reference(v);
                }
                RenderStep::MaskQuad(mi) => {
                    pass.push_debug_group("mask");
                    self.quad.bind_mask_write(pass);
                    self.quad.draw_mask(pass, mi);
                    pass.pop_debug_group();
                }
                RenderStep::Quads { range, .. } => {
                    pass.push_debug_group("quads");
                    if use_stencil {
                        self.quad.bind_stencil_test(pass);
                    } else {
                        self.quad.bind(pass);
                    }
                    self.quad.draw_range(pass, range);
                    pass.pop_debug_group();
                }
                RenderStep::Text { group } => {
                    pass.push_debug_group("text");
                    self.text.render_group(group, pass, text_mode);
                    pass.pop_debug_group();
                }
                RenderStep::Meshes { range, .. } => {
                    pass.push_debug_group("meshes");
                    self.mesh.bind(pass, use_stencil);
                    let start = range.start as usize;
                    let end = start + range.len as usize;
                    for draw in &buffer.meshes.draws[start..end] {
                        // `draw_indexed` takes a per-call vertex
                        // offset; pass the mesh's vertex start as
                        // `base_vertex` so indices stay buffer-local.
                        self.mesh
                            .draw(pass, draw.indices.into(), draw.vertices.start as i32);
                    }
                    pass.pop_debug_group();
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
            // One stroked outline per damage rect — `Partial(region)`
            // contributes the whole region; `Full` contributes a
            // single full-viewport outline. All quads ride one
            // instanced draw inside one pass so a single
            // `queue.write_buffer` covers them (per-iteration writes
            // to the same buffer would all collapse to the last
            // value at submit time).
            let inset_px = (DAMAGE_OVERLAY_INSET * buffer.scale).max(1.0);
            let stroke = Stroke::solid(
                DAMAGE_OVERLAY_COLOR,
                DAMAGE_OVERLAY_STROKE_WIDTH * buffer.scale,
            );
            let mut overlay_rects: tinyvec::ArrayVec<[Rect; DAMAGE_RECT_CAP]> = Default::default();
            match damage {
                DamagePaint::Partial(region) => {
                    for r in region.iter_rects() {
                        overlay_rects.push(
                            r.scaled_by(buffer.scale, true)
                                .deflated_by(Spacing::all(inset_px)),
                        );
                    }
                }
                DamagePaint::Full => overlay_rects.push(
                    Rect {
                        min: glam::Vec2::ZERO,
                        size: Size::new(buffer.viewport_phys_f.x, buffer.viewport_phys_f.y),
                    }
                    .deflated_by(Spacing::all(inset_px)),
                ),
                DamagePaint::Skip => unreachable!("Skip filtered before draw_debug_overlay"),
            }
            if overlay_rects.is_empty() {
                return;
            }
            self.quad
                .upload_overlays(&self.device, &self.queue, &overlay_rects, stroke);
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
            self.quad
                .draw_overlays(&mut pass, overlay_rects.len() as u32);
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
