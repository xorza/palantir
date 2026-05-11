use super::cmd_buffer::{
    CmdKind, DrawMeshPayload, DrawPolylinePayload, DrawRectPayload, DrawRectStrokedPayload,
    DrawTextPayload, PushClipRoundedPayload, RenderCmdBuffer,
};
use crate::layout::types::display::Display;
use crate::primitives::color::Color;
use crate::primitives::mesh::MeshVertex;
use crate::primitives::stroke_tessellate::{StrokeStyle, tessellate_polyline_aa};
use crate::primitives::{rect::Rect, transform::TranslateScale, urect::URect};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{DrawGroup, MeshDraw, RenderBuffer, RoundedClip, TextRun};
use glam::{UVec2, Vec2};

/// Owns the four-variable invariant that drives `out.groups`
/// emission: `current` scissor, the open group's `quads_start` /
/// `texts_start` markers, and the `last_was_text` flag for the
/// text-then-quad split rule. Every group transition routes through
/// this struct so the rule is enforced in one place.
///
/// Reset rule: every flush / scissor switch clears `last_was_text`.
/// The flag is set only by `push_text`, the sole text-emission entry
/// point.
#[derive(Default)]
struct GroupBuilder {
    current: Option<URect>,
    rounded: Option<RoundedClip>,
    quads_start: u32,
    texts_start: u32,
    meshes_start: u32,
    /// Tracks the most recent draw kind in the in-flight group. A
    /// draw-kind transition (quad↔text↔mesh) flushes so paint order
    /// within a group matches record order — simplest correct
    /// behavior; profile later if it shows up as a hotspot.
    last_kind: Option<DrawKind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrawKind {
    Quad,
    Text,
    Mesh,
}

impl DrawKind {
    /// Render order within a group: quads paint first, then text, then
    /// meshes. A kind transition that would emit something *before*
    /// what's already in the group (lower order than `last_kind`)
    /// forces a flush. Equal or higher order is fine — the natural
    /// per-group draw sequence preserves record order.
    fn order(self) -> u8 {
        match self {
            DrawKind::Quad => 0,
            DrawKind::Text => 1,
            DrawKind::Mesh => 2,
        }
    }
}

impl GroupBuilder {
    /// Push the in-flight group into `out.groups` (if non-empty),
    /// rebase `quads_start` / `texts_start` onto the current `out`
    /// lengths, and clear `last_was_text`. Scissor + rounded clip are
    /// preserved.
    fn flush(&mut self, out: &mut RenderBuffer) {
        let q_end = out.quads.len() as u32;
        let t_end = out.texts.len() as u32;
        let m_end = out.meshes.draws.len() as u32;
        if q_end > self.quads_start || t_end > self.texts_start || m_end > self.meshes_start {
            out.groups.push(DrawGroup {
                scissor: self.current,
                rounded_clip: self.rounded,
                quads: (self.quads_start..q_end).into(),
                texts: (self.texts_start..t_end).into(),
                meshes: (self.meshes_start..m_end).into(),
            });
        }
        self.quads_start = q_end;
        self.texts_start = t_end;
        self.meshes_start = m_end;
        self.last_kind = None;
    }

    /// Switch to a new clip (scissor + optional rounded), flushing the
    /// in-flight group only if anything differs. Always clears
    /// `last_was_text` — a clip transition is a draw boundary even
    /// when the resolved state happens to equal the current one
    /// (matches the pre-builder behavior).
    fn set_clip(
        &mut self,
        scissor: Option<URect>,
        rounded: Option<RoundedClip>,
        out: &mut RenderBuffer,
    ) {
        if scissor != self.current || rounded != self.rounded {
            self.flush(out);
            self.current = scissor;
            self.rounded = rounded;
        }
        self.last_kind = None;
    }

    /// Apply the kind-transition split rule: if the prior draw in the
    /// in-flight group was a different kind, flush so paint order
    /// matches record order. Same scissor continues into the new group.
    fn before_draw(&mut self, kind: DrawKind, out: &mut RenderBuffer) {
        if let Some(prev) = self.last_kind
            && kind.order() < prev.order()
        {
            self.flush(out);
        }
        self.last_kind = Some(kind);
    }

    fn push_quad(&mut self, out: &mut RenderBuffer, quad: Quad) {
        self.before_draw(DrawKind::Quad, out);
        out.quads.push(quad);
    }

    fn push_text(&mut self, out: &mut RenderBuffer, run: TextRun) {
        self.before_draw(DrawKind::Text, out);
        out.texts.push(run);
    }

    fn push_mesh(&mut self, out: &mut RenderBuffer, draw: MeshDraw) {
        self.before_draw(DrawKind::Mesh, out);
        out.meshes.draws.push(draw);
    }
}

/// CPU-only compose engine: turns a `RenderCmdBuffer` stream into a `RenderBuffer`
/// (physical-px quads + text runs + scissor groups). Owns its output buffer
/// + compose-time scratch stacks so steady-state rendering is alloc-free.
///
/// Composer doesn't know about `Tree` or `encode` — it's pure algorithm +
/// scratch + output. [`Frontend`](crate::renderer::frontend::Frontend) orchestrates
/// encode + compose.
#[derive(Default)]
pub(crate) struct Composer {
    /// Compose-time scratch — bounded by tree depth (typically <8).
    /// Pairs the resolved scissor with its rounded-clip data (when the
    /// push was `PushClipRounded`); both ride together so a `PopClip`
    /// restores them as a unit.
    clip_stack: Vec<ClipFrame>,
    transform_stack: Vec<TranslateScale>,
    /// Scratch for `DrawPolyline`: transformed physical-px points
    /// fed to the stroke tessellator. Cleared per cmd, capacity
    /// reused — keeps steady-state alloc-free.
    polyline_scratch: Vec<Vec2>,
    pub(crate) buffer: RenderBuffer,
}

#[derive(Clone, Copy)]
struct ClipFrame {
    scissor: URect,
    rounded: Option<RoundedClip>,
}

impl Composer {
    /// Consume a logical-px command stream → physical-px `Quad`s +
    /// `TextRun`s + draw groups (scissor ranges) into the composer's
    /// owned buffer, and return a borrow of the freshly-composed
    /// result. Pure: no device, no queue.
    ///
    /// `gradient_atlas` is borrowed mutably so the composer can
    /// register each `Brush::Linear` it encounters and pack the
    /// returned row id into the emitted `Quad`. Idempotent for repeat
    /// content — the same gradient hashes to the same row and reuses
    /// it across frames.
    pub(crate) fn compose(
        &mut self,
        cmds: &RenderCmdBuffer,
        display: &Display,
    ) -> &mut RenderBuffer {
        let out = &mut self.buffer;
        let scale = display.scale_factor;
        let snap = display.pixel_snap;
        let viewport_phys = display.physical;
        let viewport_phys_f = viewport_phys.as_vec2();

        out.quads.clear();
        out.texts.clear();
        out.meshes.clear();
        out.groups.clear();
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;
        out.scale = scale;

        self.clip_stack.clear();
        self.transform_stack.clear();
        let mut current_transform = TranslateScale::IDENTITY;
        let mut group = GroupBuilder::default();

        let n = cmds.kinds.len();
        let mut i = 0usize;
        while i < n {
            let kind = cmds.kinds[i];
            let start = cmds.starts[i];
            match kind {
                CmdKind::PushClip | CmdKind::PushClipRounded => {
                    let (r, logical_radius) = match kind {
                        CmdKind::PushClip => (cmds.read::<Rect>(start), None),
                        _ => {
                            let p: PushClipRoundedPayload = cmds.read(start);
                            (p.rect, Some(p.radius))
                        }
                    };
                    let world = current_transform.apply_rect(r);
                    let me = scissor_from_logical(world, scale, snap, viewport_phys);
                    let scissor = match self.clip_stack.last() {
                        Some(parent) => me.clamp_to(parent.scissor),
                        None => me,
                    };
                    let rounded = if let Some(logical_radius) = logical_radius {
                        // Combine current transform's uniform scale with DPR
                        // so radii match the painted SDF's physical size.
                        let phys_scale = current_transform.scale * scale;
                        // `mask_rect` stays unclamped — the SDF needs the
                        // rect's true edges, otherwise corner curves
                        // would shift inward when the clip partially
                        // leaves the viewport.
                        Some(RoundedClip {
                            mask_rect: world.scaled_by(scale, snap),
                            radius: logical_radius.scaled_by(phys_scale),
                        })
                    } else {
                        // Rect clip nested inside a rounded ancestor: inherit
                        // the ancestor's rounded data so children stay
                        // stencil-tested against the active mask. Without
                        // this, the child group would draw with ref=0 over
                        // pixels already stenciled to 1 by the parent's
                        // mask, and the stencil_test pipeline would discard
                        // every fragment.
                        self.clip_stack.last().and_then(|f| f.rounded)
                    };
                    self.clip_stack.push(ClipFrame { scissor, rounded });
                    group.set_clip(Some(scissor), rounded, out);
                }
                CmdKind::PopClip => {
                    self.clip_stack
                        .pop()
                        .expect("PopClip without matching PushClip");
                    let parent = self.clip_stack.last().copied();
                    group.set_clip(
                        parent.map(|f| f.scissor),
                        parent.and_then(|f| f.rounded),
                        out,
                    );
                }
                CmdKind::PushTransform => {
                    let t: TranslateScale = cmds.read(start);
                    self.transform_stack.push(current_transform);
                    current_transform = current_transform.compose(t);
                }
                CmdKind::PopTransform => {
                    current_transform = self
                        .transform_stack
                        .pop()
                        .expect("PopTransform without matching PushTransform");
                }
                kind @ (CmdKind::DrawRect | CmdKind::DrawRectStroked) => {
                    let (
                        rect,
                        radius,
                        fill,
                        stroke_color,
                        stroke_width,
                        fill_kind,
                        fill_grad_idx,
                        fill_axis,
                    ) = match kind {
                        CmdKind::DrawRect => {
                            let p: DrawRectPayload = cmds.read(start);
                            (
                                p.rect,
                                p.radius,
                                p.fill,
                                Color::TRANSPARENT,
                                0.0,
                                p.fill_kind,
                                p.fill_grad_idx,
                                p.fill_axis,
                            )
                        }
                        _ => {
                            let p: DrawRectStrokedPayload = cmds.read(start);
                            (
                                p.rect,
                                p.radius,
                                p.fill,
                                p.stroke_color,
                                p.stroke_width,
                                p.fill_kind,
                                p.fill_grad_idx,
                                p.fill_axis,
                            )
                        }
                    };
                    let world_rect = current_transform.apply_rect(rect);
                    // Clip-cull: skip emitting the quad when it sits
                    // entirely outside the active scissor. The GPU
                    // would scissor it away anyway; this saves the
                    // `quads.push` + per-quad math. Keeps the encode
                    // cache shape-stable (cull lives only here, not in
                    // the encoder).
                    if let Some(active) = self.clip_stack.last() {
                        let me = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                        if me.intersect(active.scissor).is_none() {
                            i += 1;
                            continue;
                        }
                    }
                    let world_radius = radius.scaled_by(current_transform.scale);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let phys_radius = world_radius.scaled_by(scale);
                    // Linear brushes register with the atlas on first
                    // sight and reuse the row across subsequent quads
                    // pointing at the same content (same gradient on
                    // multiple panels → one bake). Solid passes
                    // through with the row sentinel'd to 0.
                    let fill_lut_row = if fill_kind.is_linear() {
                        let g = &cmds.linear_gradients[fill_grad_idx as usize];
                        out.gradient_atlas.register(g)
                    } else {
                        0
                    };
                    group.push_quad(
                        out,
                        Quad {
                            rect: phys_rect,
                            fill,
                            radius: phys_radius,
                            stroke_color,
                            stroke_width: stroke_width * current_transform.scale * scale,
                            fill_kind,
                            fill_lut_row,
                            fill_axis,
                        },
                    );
                }
                CmdKind::DrawMesh => {
                    let p: DrawMeshPayload = cmds.read(start);
                    // Per-vertex transform: shift by `origin` (logical
                    // px owner-relative → world), apply the active
                    // transform, then DPI-scale. No pixel-snap: the
                    // mesh is user geometry; snapping arbitrary
                    // vertices changes shape.
                    let v_start = p.v_start as usize;
                    let v_end = v_start + p.v_len as usize;
                    let i_start = p.i_start as usize;
                    let i_end = i_start + p.i_len as usize;
                    let phys_v_start = out.meshes.arena.vertices.len() as u32;
                    let tint = p.tint;
                    for v in &cmds.shape_payloads.meshes.vertices[v_start..v_end] {
                        let world = current_transform.apply_point(v.pos + p.origin);
                        // Premultiplied-alpha tinting: component-wise
                        // multiply works for both rgb and alpha. The
                        // backend pipeline doesn't take a tint uniform
                        // — it's baked in here.
                        let c = v.color;
                        out.meshes.arena.vertices.push(MeshVertex {
                            pos: world * scale,
                            color: crate::primitives::color::Color {
                                r: c.r * tint.r,
                                g: c.g * tint.g,
                                b: c.b * tint.b,
                                a: c.a * tint.a,
                            },
                        });
                    }
                    let phys_i_start = out.meshes.arena.indices.len() as u32;
                    out.meshes
                        .arena
                        .indices
                        .extend_from_slice(&cmds.shape_payloads.meshes.indices[i_start..i_end]);
                    group.push_mesh(
                        out,
                        MeshDraw {
                            vertices: (phys_v_start..phys_v_start + p.v_len).into(),
                            indices: (phys_i_start..phys_i_start + p.i_len).into(),
                        },
                    );
                }
                CmdKind::DrawPolyline => {
                    let p: DrawPolylinePayload = cmds.read(start);
                    let mode = p.color_mode.get();
                    let cap = p.cap.get();
                    let join = p.join.get();
                    let width_phys = p.width * current_transform.scale * scale;

                    // Pre-transform scissor cull. Encoder shipped a
                    // logical-px AABB over the cmd-buffer points;
                    // `TranslateScale` is uniform-scale so the
                    // transformed rect stays axis-aligned. Inflate
                    // by the tessellator's outer-fringe offset
                    // (`max(w/2, 0.5) + 0.5` in *phys* px) so the
                    // cull never trims a pixel the stroke would
                    // reach. Short-circuits before transforming the
                    // full point list — the win for long flattened
                    // curves.
                    if let Some(active) = self.clip_stack.last() {
                        let world = current_transform.apply_rect(p.bbox);
                        let inflate_phys = (width_phys * 0.5).max(0.5) + 0.5;
                        let inflate_logical = inflate_phys / scale;
                        let inflated = Rect {
                            min: world.min - Vec2::splat(inflate_logical),
                            size: crate::primitives::size::Size {
                                w: world.size.w + 2.0 * inflate_logical,
                                h: world.size.h + 2.0 * inflate_logical,
                            },
                        };
                        let bbox_scissor =
                            scissor_from_logical(inflated, scale, false, viewport_phys);
                        if bbox_scissor.intersect(active.scissor).is_none() {
                            i += 1;
                            continue;
                        }
                    }

                    let pts_start = p.points_start as usize;
                    let pts_end = pts_start + p.points_len as usize;
                    let cs_start = p.colors_start as usize;
                    let cs_end = cs_start + p.colors_len as usize;
                    let src_points = &cmds.shape_payloads.polyline_points[pts_start..pts_end];
                    let src_colors = &cmds.shape_payloads.polyline_colors[cs_start..cs_end];

                    // Transform points into physical-px. No
                    // pixel-snap — snapping stroke verts shifts
                    // thin lines off-axis. Hairline regime
                    // (<1 phys px) handled inside the tessellator.
                    self.polyline_scratch.clear();
                    self.polyline_scratch.extend(
                        src_points
                            .iter()
                            .map(|&q| current_transform.apply_point(q) * scale),
                    );

                    let phys_v_start = out.meshes.arena.vertices.len() as u32;
                    let phys_i_start = out.meshes.arena.indices.len() as u32;
                    tessellate_polyline_aa(
                        &self.polyline_scratch,
                        src_colors,
                        StrokeStyle {
                            mode,
                            cap,
                            join,
                            width_phys,
                        },
                        &mut out.meshes.arena.vertices,
                        &mut out.meshes.arena.indices,
                    );
                    let v_len = out.meshes.arena.vertices.len() as u32 - phys_v_start;
                    let i_len = out.meshes.arena.indices.len() as u32 - phys_i_start;
                    if v_len == 0 {
                        i += 1;
                        continue;
                    }
                    group.push_mesh(
                        out,
                        MeshDraw {
                            vertices: (phys_v_start..phys_v_start + v_len).into(),
                            indices: (phys_i_start..phys_i_start + i_len).into(),
                        },
                    );
                }
                CmdKind::DrawText => {
                    let t: DrawTextPayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(t.rect);
                    // Glyphon clips per-`TextArea` against the run's own
                    // `bounds`, ignoring whatever `wgpu` scissor is active.
                    // Intersect with the active clip-stack top so ancestor
                    // `clip = true` panels actually clip glyphs; an empty
                    // intersection means the run can't reach pixels — skip
                    // the push entirely (cull).
                    let mut bounds = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    if let Some(parent) = self.clip_stack.last() {
                        bounds = bounds.clamp_to(parent.scissor);
                    }
                    if bounds.w == 0 || bounds.h == 0 {
                        i += 1;
                        continue;
                    }
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    group.push_text(
                        out,
                        TextRun {
                            origin: phys_rect.min,
                            bounds,
                            color: t.color,
                            key: t.key,
                            scale: current_transform.scale,
                        },
                    );
                }
            }
            i += 1;
        }
        group.flush(out);

        &mut self.buffer
    }
}

fn scissor_from_logical(r: Rect, scale: f32, snap: bool, viewport: UVec2) -> URect {
    let phys = r.scaled_by(scale, snap);
    let x = (phys.min.x.max(0.0) as u32).min(viewport.x);
    let y = (phys.min.y.max(0.0) as u32).min(viewport.y);
    let right = ((phys.min.x + phys.size.w).max(0.0) as u32).min(viewport.x);
    let bottom = ((phys.min.y + phys.size.h).max(0.0) as u32).min(viewport.y);
    URect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

#[cfg(test)]
mod tests;
