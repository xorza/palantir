use super::cmd_buffer::{
    CmdKind, DrawMeshPayload, DrawPolylinePayload, DrawRectPayload, DrawTextPayload,
    PushClipPayload, RenderCmdBuffer,
};
use crate::common::frame_arena::FrameArena;
use crate::layout::types::display::Display;
use crate::primitives::approx::EPS;
use crate::primitives::color::Color;
use crate::primitives::stroke_tessellate::{StrokeStyle, tessellate_polyline_aa};
use crate::primitives::{rect::Rect, transform::TranslateScale, urect::URect};
use crate::renderer::gradient_atlas::LutRow;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{
    DrawGroup, MeshBatch, MeshDraw, MeshInstance, RenderBuffer, RoundedClip, TextBatch, TextRun,
};
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
    /// Physical-px AABBs of the text runs accumulated in the current
    /// text-batch (potentially across multiple groups). A new quad
    /// overlapping any of these would paint *under* the merged
    /// batch text — closes the batch (and flushes the group) so
    /// paint order is preserved. Cleared in [`Self::close_batch`].
    /// Subsumes the old per-group `text_rects` since intra-group
    /// overlap is a special case of intra-batch overlap.
    batch_text_rects: Vec<URect>,
    /// Per-group mesh AABBs. Used by the intra-group text-after-mesh
    /// check (text recorded after a same-group mesh paints under it
    /// under the kind reorder, so flush). Cleared per flush —
    /// independent of batch state since mesh forces batch close.
    mesh_rects: Vec<URect>,
    /// In-flight group state. `*_start` cursors mark where the open
    /// group's `quads`/`texts`/`meshes` slice begins in `out`;
    /// [`Self::flush`] closes the slice and advances them.
    current_scissor: Option<URect>,
    current_rounded: Option<RoundedClip>,
    quads_start: u32,
    texts_start: u32,
    meshes_start: u32,
    /// Bundled state for the currently-open text batch — `Some` while
    /// the composer is accumulating runs into a batch, `None`
    /// between batches. The rect scratch lives outside in
    /// `batch_text_rects` so its `Vec` capacity is retained across
    /// open/close cycles (steady-state alloc-free).
    open_batch: Option<OpenBatch>,
}

#[derive(Clone, Copy)]
struct ClipFrame {
    scissor: URect,
    rounded: Option<RoundedClip>,
}

/// State carried while a text batch is mid-accumulation. Pushed onto
/// `out.text_batches` as a [`TextBatch`] when [`Composer::close_batch`]
/// finalizes it.
#[derive(Clone, Copy)]
struct OpenBatch {
    /// Cursor into `out.texts` where this batch's run span begins.
    /// Combined with `out.texts.len()` at close-time to compute the
    /// finalized [`Span`].
    texts_start: u32,
    /// Index (into `out.groups`) of the last group whose text
    /// contributed to this batch. Refreshed on every text push (the
    /// in-flight group's eventual index is `out.groups.len()`).
    /// Tells the schedule where to emit the merged render step.
    last_group: u32,
    /// Union AABB of every rect in `Composer.batch_text_rects` for
    /// this batch. The first reject for a new quad's overlap test —
    /// O(1) instead of the linear scan.
    text_union: URect,
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
            // Push the mesh batch BEFORE the group itself so its
            // `last_group` matches the in-flight group's eventual
            // index (= current `out.groups.len()`).
            if m_end > self.meshes_start {
                out.mesh_batches.push(MeshBatch {
                    meshes: (self.meshes_start..m_end).into(),
                    last_group: out.groups.len() as u32,
                });
            }
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
        self.mesh_rects.clear();
    }

    /// Finalize the open text batch (if any): push a [`TextBatch`]
    /// entry covering `batch_texts_start..out.texts.len()` and clear
    /// batch-scoped scratch. No-op when no batch is active. Called
    /// at batch-split events — rounded-clip change, mesh/polyline
    /// append, or a quad that would paint under accumulated batch
    /// text. The id used by the just-pushed batch matches what
    /// [`Self::open_batch`] stamped on contributing groups.
    fn close_batch(&mut self, out: &mut RenderBuffer) {
        let Some(b) = self.open_batch.take() else {
            return;
        };
        let texts_end = out.texts.len() as u32;
        // Invariants the schedule cursor relies on: batches are pushed
        // in walk order so `last_group` is monotonically non-decreasing
        // (multiple batches can anchor to the same group when a mesh
        // splits mid-group), and their `texts` spans concatenate
        // without gaps in `out.texts`.
        debug_assert!(
            out.text_batches
                .last()
                .is_none_or(|prev| prev.last_group <= b.last_group),
        );
        debug_assert!(
            out.text_batches
                .last()
                .is_none_or(|prev| prev.texts.start + prev.texts.len == b.texts_start),
        );
        out.text_batches.push(TextBatch {
            texts: (b.texts_start..texts_end).into(),
            last_group: b.last_group,
        });
        self.batch_text_rects.clear();
    }

    /// Return a mutable handle to the open batch, opening a fresh one
    /// when none exists. Idempotent within a batch — repeated calls
    /// reuse the same `OpenBatch` and only refresh `last_group` to
    /// the in-flight group's eventual index.
    fn open_batch(&mut self, out: &RenderBuffer) -> &mut OpenBatch {
        let b = self.open_batch.get_or_insert(OpenBatch {
            texts_start: out.texts.len() as u32,
            last_group: 0,
            text_union: URect::default(),
        });
        b.last_group = out.groups.len() as u32;
        b
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
        if rounded != self.current_rounded {
            // Stencil ref is tied to the active rounded clip; batched
            // text under the wrong mask would either over- or
            // under-clip. Close before the group transition.
            self.close_batch(out);
        }
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
        arena: &mut FrameArena,
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
        out.text_batches.clear();
        out.mesh_batches.clear();
        out.has_rounded_clip = false;
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;
        out.scale = scale;

        self.clip_stack.clear();
        self.transform_stack.clear();
        self.batch_text_rects.clear();
        self.mesh_rects.clear();
        self.current_scissor = None;
        self.current_rounded = None;
        self.quads_start = 0;
        self.texts_start = 0;
        self.meshes_start = 0;
        self.open_batch = None;
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
                    // record order. Text overlap is checked against the
                    // whole open batch (which may span multiple groups);
                    // a hit also closes the batch so the merged text
                    // doesn't paint over this quad at end-of-batch.
                    // Coarse reject first against the batch's union AABB
                    // before scanning per-rect — the common case in a
                    // large batch is "quad far from any text," so the
                    // O(n) scan is wasted work without this.
                    //
                    // Shadow quads use a 2σ-deflated rect for the
                    // overlap check: the outer 2σ rim of a Gaussian
                    // contributes <5% alpha and is visually
                    // indistinguishable from the background, so we
                    // shouldn't force a batch flush for that ring.
                    // Keeps adjacent text in the same batch when a
                    // soft drop shadow sits 1–2σ away from text.
                    let overlap_urect = if p.fill_kind.is_shadow() {
                        let sigma_phys =
                            p.fill_axis.t0().max(0.0) * current_transform.scale * scale;
                        quad_urect.deflated((2.0 * sigma_phys) as u32)
                    } else {
                        quad_urect
                    };
                    let batch_text_hit = self
                        .open_batch
                        .as_ref()
                        .is_some_and(|b| b.text_union.intersect(overlap_urect).is_some())
                        && any_overlap(&self.batch_text_rects, overlap_urect);
                    if batch_text_hit {
                        self.close_batch(out);
                        self.flush(out);
                    } else if any_overlap(&self.mesh_rects, overlap_urect) {
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
                    // Shadow params (offset, σ) live in fill_axis as
                    // logical-px scalars; scale to physical px so the
                    // shader's `local` (physical px from vs) lines up.
                    // Gradient axis is 0..1 local — never scaled.
                    let fill_axis = if p.fill_kind.is_shadow() {
                        p.fill_axis.scaled(current_transform.scale * scale)
                    } else {
                        p.fill_axis
                    };
                    out.quads.push(Quad {
                        rect: phys_rect,
                        fill: p.fill,
                        radius: phys_radius,
                        stroke_color: p.stroke_color,
                        stroke_width: p.stroke_width * current_transform.scale * scale,
                        fill_kind: p.fill_kind,
                        fill_lut_row,
                        fill_axis,
                    });
                }
                CmdKind::DrawMesh => {
                    // Mesh paints above text in the kind order. With
                    // text batching, the batch render emits at the END
                    // of its last group — past this mesh in schedule
                    // walk if the batch were left open. Close so the
                    // batch's text emits before this group's mesh and
                    // mesh-over-text is preserved.
                    self.close_batch(out);
                    let p: DrawMeshPayload = cmds.read(start);
                    // Verts already live in FrameArena owner-local;
                    // span passes through to `MeshDraw` verbatim. The
                    // per-instance translate folds in both the owner
                    // origin and the active push-transform stack so the
                    // shader produces physical coords. Phase 1's
                    // transform/tint move plus this slice eliminates
                    // both the per-vertex CPU multiply and the
                    // per-frame vertex copy.
                    let phys_scale = current_transform.scale * scale;
                    let phys_translate = (current_transform.scale * p.origin
                        + current_transform.translation)
                        * scale;
                    out.meshes.draws.push(MeshDraw {
                        vertices: (p.v_start..p.v_start + p.v_len).into(),
                        indices: (p.i_start..p.i_start + p.i_len).into(),
                    });
                    let tint_color: Color = p.tint.into();
                    out.meshes.instances.push(MeshInstance {
                        translate: phys_translate,
                        scale: phys_scale,
                        tint: tint_color.into(),
                        ..bytemuck::Zeroable::zeroed()
                    });
                    if p.v_len > 0 {
                        // Inflate by 0.5 phys-px to match polyline's
                        // AA-fringe policy. Mesh today paints inside
                        // its vertex hull, but a future AA edge or
                        // displacement shader would silently produce
                        // false negatives — and false negatives in
                        // the overlap test reorder paint.
                        let world_bbox = current_transform.apply_rect(Rect {
                            min: p.bbox.min + p.origin,
                            size: p.bbox.size,
                        });
                        let min = world_bbox.min * scale;
                        let max = world_bbox.max() * scale;
                        let fringe = Vec2::splat(0.5);
                        self.mesh_rects.push(urect_from_phys(
                            min - fringe,
                            max + fringe,
                            viewport_phys,
                        ));
                    }
                }
                CmdKind::DrawPolyline => {
                    // Polyline tessellates to a mesh — same paint-order
                    // rule as DrawMesh. Close any open text batch
                    // before appending so batched text emits earlier.
                    self.close_batch(out);
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
                    let world_bbox = current_transform.apply_rect(Rect {
                        min: p.bbox.min + p.origin,
                        size: p.bbox.size,
                    });
                    let inflate_phys = (width_phys * 0.5).max(0.5) + 0.5;
                    let inflate_logical = inflate_phys / scale;
                    let inflated = Rect {
                        min: world_bbox.min - Vec2::splat(inflate_logical),
                        size: crate::primitives::size::Size {
                            w: world_bbox.size.w + 2.0 * inflate_logical,
                            h: world_bbox.size.h + 2.0 * inflate_logical,
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
                    let src_points = &arena.polyline_points[pts_start..pts_end];
                    let src_colors = &arena.polyline_colors[cs_start..cs_end];

                    // Transform points into physical-px. Owner-local
                    // origin is folded in here so points stay owner-
                    // local in the arena (cross-frame stable). No
                    // pixel-snap — snapping stroke verts shifts thin
                    // lines off-axis. Hairline regime (<1 phys px)
                    // handled inside the tessellator.
                    self.polyline_scratch.clear();
                    self.polyline_scratch.extend(
                        src_points
                            .iter()
                            .map(|&q| current_transform.apply_point(q + p.origin) * scale),
                    );

                    let phys_v_start = arena.meshes.vertices.len() as u32;
                    let phys_i_start = arena.meshes.indices.len() as u32;
                    tessellate_polyline_aa(
                        &self.polyline_scratch,
                        src_colors,
                        StrokeStyle {
                            mode,
                            cap,
                            join,
                            width_phys,
                        },
                        &mut arena.meshes.vertices,
                        &mut arena.meshes.indices,
                    );
                    let v_len = arena.meshes.vertices.len() as u32 - phys_v_start;
                    let i_len = arena.meshes.indices.len() as u32 - phys_i_start;
                    if v_len == 0 {
                        continue;
                    }
                    out.meshes.draws.push(MeshDraw {
                        vertices: (phys_v_start..phys_v_start + v_len).into(),
                        indices: (phys_i_start..phys_i_start + i_len).into(),
                    });
                    // Polyline points are pre-transformed to physical-px
                    // on CPU (the tessellator needs phys-px width to pick
                    // its AA fringe), so the shader's per-instance state
                    // is identity. Tint is white — colors live per-vertex.
                    out.meshes.instances.push(MeshInstance {
                        translate: Vec2::ZERO,
                        scale: 1.0,
                        tint: crate::primitives::color::ColorU8::WHITE,
                        ..bytemuck::Zeroable::zeroed()
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
                    // open_batch must run BEFORE the text push so the
                    // batch's `texts_start` captures this run's index.
                    let b = self.open_batch(out);
                    b.text_union = b.text_union.union(bounds);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    out.texts.push(TextRun {
                        origin: phys_rect.min,
                        bounds,
                        // Glyphon's `ColorMode::Accurate` decodes sRGB→linear
                        // in its shader, so encode once here rather than per
                        // frame in the backend. Also makes `TextRun` Pod for
                        // byte-slice hashing in the hash-skip fast path.
                        // Cmd buffer stores `ColorF16` (linear); glyphon
                        // expects `ColorU8`. Decode f16→linear then encode
                        // linear→sRGB once at the boundary.
                        // Glyphon is the one sRGB special case: its API
                        // expects sRGB-encoded u8. Everything else
                        // downstream (Quad fill/stroke, vertex colours,
                        // gradient LUT) stays linear.
                        color: Color::from(t.color).to_srgb_u8(),
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
                    self.batch_text_rects.push(bounds);
                }
            }
        }
        self.close_batch(out);
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
