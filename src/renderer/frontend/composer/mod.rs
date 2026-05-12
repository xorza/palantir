use super::cmd_buffer::{
    CmdKind, DrawMeshPayload, DrawPolylinePayload, DrawRectPayload, DrawTextPayload,
    PushClipPayload, RenderCmdBuffer,
};
use crate::layout::types::display::Display;
use crate::primitives::approx::EPS;
use crate::primitives::color::Color;
use crate::primitives::mesh::MeshVertex;
use crate::primitives::stroke_tessellate::{StrokeStyle, tessellate_polyline_aa};
use crate::primitives::{rect::Rect, transform::TranslateScale, urect::URect};
use crate::renderer::gradient_atlas::LutRow;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{DrawGroup, MeshDraw, RenderBuffer, RoundedClip, TextRun};
use glam::{UVec2, Vec2};

/// CPU-only compose engine: turns a `RenderCmdBuffer` stream into a `RenderBuffer`
/// (physical-px quads + text runs + scissor groups). Owns its output buffer
/// + compose-time scratch stacks so steady-state rendering is alloc-free.
///
/// Composer doesn't know about `Tree` or `encode` — it's pure algorithm +
/// scratch + output. [`Frontend`](crate::renderer::frontend::Frontend) orchestrates
/// encode + compose.
///
/// Render order *within* a group is fixed by the backend:
/// **quads → text → meshes**. That's safe iff for every prior draw of
/// a higher kind, no later draw of a lower kind overlaps it — a draw
/// that violates the rule forces a [`Self::flush`] so record order is
/// honored. The check uses [`text_rects`](Self::text_rects) /
/// [`mesh_rects`](Self::mesh_rects) accumulated for the in-flight group.
#[derive(Default)]
pub(crate) struct Composer {
    /// Compose-time scratch — bounded by tree depth (typically <8).
    /// Pairs the resolved scissor with its rounded-clip data (when the
    /// push carried a non-zero radius); both ride together so a `PopClip`
    /// restores them as a unit.
    clip_stack: Vec<ClipFrame>,
    transform_stack: Vec<TranslateScale>,
    /// Scratch for `DrawPolyline`: transformed physical-px points
    /// fed to the stroke tessellator. Cleared per cmd, capacity
    /// reused — keeps steady-state alloc-free.
    polyline_scratch: Vec<Vec2>,
    /// Per-group physical-px AABBs of the text runs and mesh draws
    /// already pushed into the in-flight group. Used to decide whether
    /// a later lower-kind draw (quad after text/mesh, text after mesh)
    /// can be reordered into the group's quad/text batch without
    /// changing the painted result. Cleared on every flush /
    /// clip transition. Capacity reused across frames.
    text_rects: Vec<URect>,
    mesh_rects: Vec<URect>,
    /// In-flight group state. `*_start` cursors mark where the open
    /// group's `quads`/`texts`/`meshes` slice begins in `out`;
    /// [`Self::flush`] closes the slice and advances them.
    current_scissor: Option<URect>,
    current_rounded: Option<RoundedClip>,
    quads_start: u32,
    texts_start: u32,
    meshes_start: u32,
}

#[derive(Clone, Copy)]
struct ClipFrame {
    scissor: URect,
    rounded: Option<RoundedClip>,
}

impl Composer {
    /// Close the in-flight group: if anything was emitted into it,
    /// push a `DrawGroup` covering the open slice; either way advance
    /// the per-kind cursors and clear the overlap scratches. Scissor
    /// + rounded clip are preserved for the next group.
    fn flush(&mut self, out: &mut RenderBuffer) {
        let q_end = out.quads.len() as u32;
        let t_end = out.texts.len() as u32;
        let m_end = out.meshes.draws.len() as u32;
        if q_end > self.quads_start || t_end > self.texts_start || m_end > self.meshes_start {
            out.groups.push(DrawGroup {
                scissor: self.current_scissor,
                rounded_clip: self.current_rounded,
                quads: (self.quads_start..q_end).into(),
                texts: (self.texts_start..t_end).into(),
                meshes: (self.meshes_start..m_end).into(),
            });
        }
        self.quads_start = q_end;
        self.texts_start = t_end;
        self.meshes_start = m_end;
        self.text_rects.clear();
        self.mesh_rects.clear();
    }

    /// Switch to a new clip (scissor + optional rounded), flushing
    /// the in-flight group only if anything actually differs. A
    /// same-clip Push/Pop is a no-op so accumulated overlap state
    /// persists through redundant clip transitions.
    fn set_clip(
        &mut self,
        scissor: Option<URect>,
        rounded: Option<RoundedClip>,
        out: &mut RenderBuffer,
    ) {
        if scissor != self.current_scissor || rounded != self.current_rounded {
            self.flush(out);
            self.current_scissor = scissor;
            self.current_rounded = rounded;
        }
    }

    /// Consume a logical-px command stream → physical-px `Quad`s +
    /// `TextRun`s + draw groups (scissor ranges) into the caller-
    /// provided `out` buffer. Pure: no device, no queue.
    ///
    /// `out.gradient_atlas` is mutated in place: each `Brush::Linear`
    /// encountered registers a row and the returned row id is packed
    /// into the emitted `Quad`. Idempotent across frames — the same
    /// gradient hashes to the same row and reuses it.
    #[profiling::function]
    pub(crate) fn compose(
        &mut self,
        cmds: &RenderCmdBuffer,
        display: Display,
        out: &mut RenderBuffer,
    ) {
        let scale = display.scale_factor;
        let snap = display.pixel_snap;
        let viewport_phys = display.physical;
        let viewport_phys_f = viewport_phys.as_vec2();

        out.quads.clear();
        out.texts.clear();
        out.meshes.clear();
        out.groups.clear();
        out.has_rounded_clip = false;
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;
        out.scale = scale;

        self.clip_stack.clear();
        self.transform_stack.clear();
        self.text_rects.clear();
        self.mesh_rects.clear();
        self.current_scissor = None;
        self.current_rounded = None;
        self.quads_start = 0;
        self.texts_start = 0;
        self.meshes_start = 0;
        let mut current_transform = TranslateScale::IDENTITY;

        for i in 0..cmds.kinds.len() {
            let kind = cmds.kinds[i];
            let start = cmds.starts[i];
            match kind {
                CmdKind::PushClip => {
                    let p: PushClipPayload = cmds.read(start);
                    let logical_radius = (!p.radius.approx_zero()).then_some(p.radius);
                    let world = current_transform.apply_rect(p.rect);
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
                        out.has_rounded_clip = true;
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
                    self.set_clip(Some(scissor), rounded, out);
                }
                CmdKind::PopClip => {
                    self.clip_stack
                        .pop()
                        .expect("PopClip without matching PushClip");
                    let parent = self.clip_stack.last().copied();
                    self.set_clip(
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
                CmdKind::DrawRect => {
                    let p: DrawRectPayload = cmds.read(start);
                    let world_rect = current_transform.apply_rect(p.rect);
                    let quad_urect = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    // Clip-cull: skip emitting the quad when it sits
                    // entirely outside the active scissor. The GPU
                    // would scissor it away anyway; this saves the
                    // `quads.push` + per-quad math.
                    if let Some(active) = self.clip_stack.last()
                        && quad_urect.intersect(active.scissor).is_none()
                    {
                        continue;
                    }
                    // Overlap-aware kind transition: quad is the lowest
                    // kind, so anything higher already in the group
                    // (text, mesh) that this quad overlaps would paint
                    // *under* it after kind-reorder — flush to keep
                    // record order.
                    if any_overlap(&self.text_rects, quad_urect)
                        || any_overlap(&self.mesh_rects, quad_urect)
                    {
                        self.flush(out);
                    }
                    let world_radius = p.radius.scaled_by(current_transform.scale);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let phys_radius = world_radius.scaled_by(scale);
                    let fill_lut_row = if p.fill_kind.is_gradient() {
                        let key = &cmds.gradient_lut_keys[p.fill_grad_idx as usize];
                        out.gradient_atlas.register_stops(&key.stops, key.interp)
                    } else {
                        LutRow::FALLBACK
                    };
                    out.quads.push(Quad {
                        rect: phys_rect,
                        fill: p.fill,
                        radius: phys_radius,
                        stroke_color: p.stroke_color,
                        stroke_width: p.stroke_width * current_transform.scale * scale,
                        fill_kind: p.fill_kind,
                        fill_lut_row,
                        fill_axis: p.fill_axis,
                    });
                }
                CmdKind::DrawMesh => {
                    let p: DrawMeshPayload = cmds.read(start);
                    // Per-vertex transform: apply the active transform,
                    // then DPI-scale. Verts are already in logical-px
                    // world coords (encoder pre-translated). No
                    // pixel-snap: the mesh is user geometry; snapping
                    // arbitrary vertices changes shape.
                    let v_start = p.v_start as usize;
                    let v_end = v_start + p.v_len as usize;
                    let i_start = p.i_start as usize;
                    let i_end = i_start + p.i_len as usize;
                    let phys_v_start = out.meshes.arena.vertices.len() as u32;
                    let tint = p.tint;
                    let mut min = Vec2::splat(f32::INFINITY);
                    let mut max = Vec2::splat(f32::NEG_INFINITY);
                    for v in &cmds.shape_payloads.meshes.vertices[v_start..v_end] {
                        let world = current_transform.apply_point(v.pos);
                        let pos = world * scale;
                        min = min.min(pos);
                        max = max.max(pos);
                        // Premultiplied-alpha tinting: component-wise
                        // multiply works for both rgb and alpha. The
                        // backend pipeline doesn't take a tint uniform
                        // — it's baked in here.
                        let c = v.color;
                        out.meshes.arena.vertices.push(MeshVertex {
                            pos,
                            color: Color {
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
                    // Mesh is the highest kind: never flushes for
                    // overlap. Just append to the open group and
                    // record the AABB for any later quad / text that
                    // might need to flush against it.
                    out.meshes.draws.push(MeshDraw {
                        vertices: (phys_v_start..phys_v_start + p.v_len).into(),
                        indices: (phys_i_start..phys_i_start + p.i_len).into(),
                    });
                    if p.v_len > 0 {
                        // Inflate by 0.5 phys-px to match polyline's
                        // AA-fringe policy. Mesh today paints inside
                        // its vertex hull, but a future AA edge or
                        // displacement shader would silently produce
                        // false negatives — and false negatives in
                        // the overlap test reorder paint.
                        let fringe = Vec2::splat(0.5);
                        self.mesh_rects.push(urect_from_phys(
                            min - fringe,
                            max + fringe,
                            viewport_phys,
                        ));
                    }
                }
                CmdKind::DrawPolyline => {
                    let p: DrawPolylinePayload = cmds.read(start);
                    let mode = p.color_mode.get();
                    let cap = p.cap.get();
                    let join = p.join.get();
                    let width_phys = p.width * current_transform.scale * scale;

                    // Compute the inflated physical-px AABB once and
                    // reuse it for cull and overlap tracking. Encoder
                    // shipped a logical-px AABB over the cmd-buffer
                    // points; `TranslateScale` is uniform-scale so the
                    // transformed rect stays axis-aligned. Inflate
                    // by the tessellator's outer-fringe offset
                    // (`max(w/2, 0.5) + 0.5` in *phys* px) so the
                    // cull never trims a pixel the stroke would
                    // reach. Short-circuits before transforming the
                    // full point list — the win for long flattened
                    // curves.
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
                    let bbox_scissor = scissor_from_logical(inflated, scale, false, viewport_phys);
                    if let Some(active) = self.clip_stack.last()
                        && bbox_scissor.intersect(active.scissor).is_none()
                    {
                        continue;
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
                        continue;
                    }
                    out.meshes.draws.push(MeshDraw {
                        vertices: (phys_v_start..phys_v_start + v_len).into(),
                        indices: (phys_i_start..phys_i_start + i_len).into(),
                    });
                    self.mesh_rects.push(bbox_scissor);
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
                        continue;
                    }
                    // Text sits below mesh in the kind order — flush
                    // if any prior mesh in the group overlaps so this
                    // text doesn't get reordered above it. (No need
                    // to check quads: text paints over quads anyway.)
                    if any_overlap(&self.mesh_rects, bounds) {
                        self.flush(out);
                    }
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    out.texts.push(TextRun {
                        origin: phys_rect.min,
                        bounds,
                        color: t.color,
                        key: t.key,
                        // Snap the ancestor-transform component of the
                        // text scale to discrete 2.5% steps. Continuous
                        // zoom would otherwise mint a fresh glyphon
                        // cache key every frame (subpixel font size +
                        // bin shift), forcing swash to re-rasterize
                        // every glyph. Snapping stabilizes the key
                        // across small zoom deltas so the atlas hits.
                        // Quads/meshes keep continuous scale — only
                        // text glyph crispness "steps."
                        scale: snap_text_scale(current_transform.scale),
                    });
                    self.text_rects.push(bounds);
                }
            }
        }
        self.flush(out);
    }
}

/// Step size of the text-scale ladder. 2.5% additive — empirically
/// fine-grained enough that crispness "stepping" isn't visible during
/// typical zoom gestures, coarse enough that consecutive zoom frames
/// hash to the same glyph cache key and reuse rasterized atlas slots.
const TEXT_SCALE_STEP: f32 = 0.025;

/// Snap the ancestor-transform component of a text run's scale to the
/// 2.5% ladder. Identity is preserved exactly so non-zoom UIs stay on
/// the trivial path. See call-site comment in `DrawText` for rationale.
fn snap_text_scale(s: f32) -> f32 {
    if (s - 1.0).abs() < EPS {
        return 1.0;
    }
    (s / TEXT_SCALE_STEP).round() * TEXT_SCALE_STEP
}

/// Conservative overlap test: any non-empty intersection counts.
/// False positives are correctness-safe (extra flush, costs a
/// drawcall); false negatives would reorder paint and corrupt the
/// frame.
fn any_overlap(slots: &[URect], r: URect) -> bool {
    slots.iter().any(|s| s.intersect(r).is_some())
}

/// Clamp a physical-px AABB to the viewport, returning the
/// non-negative `URect` the GPU can consume. NaN/non-finite inputs
/// collapse to `URect::default()` (zero-sized).
fn urect_from_phys(min: Vec2, max: Vec2, viewport: UVec2) -> URect {
    if !(min.x.is_finite() && min.y.is_finite() && max.x.is_finite() && max.y.is_finite()) {
        return URect::default();
    }
    let x = (min.x.max(0.0) as u32).min(viewport.x);
    let y = (min.y.max(0.0) as u32).min(viewport.y);
    let right = (max.x.max(0.0) as u32).min(viewport.x);
    let bottom = (max.y.max(0.0) as u32).min(viewport.y);
    URect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

fn scissor_from_logical(r: Rect, scale: f32, snap: bool, viewport: UVec2) -> URect {
    let phys = r.scaled_by(scale, snap);
    urect_from_phys(phys.min, phys.max(), viewport)
}

#[cfg(test)]
mod tests;
