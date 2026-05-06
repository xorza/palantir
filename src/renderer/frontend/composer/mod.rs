use super::cmd_buffer::{
    CmdKind, DrawRectPayload, DrawRectStrokedPayload, DrawTextPayload, EnterSubtreePayload,
    PushClipRoundedPayload, RenderCmdBuffer,
};
use crate::common::hash::Hasher;
use crate::layout::cache::AvailableKey;
use crate::layout::types::display::Display;
use crate::primitives::corners::Corners;
use crate::primitives::{rect::Rect, stroke::Stroke, transform::TranslateScale, urect::URect};
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::{DrawGroup, RenderBuffer, TextRun};
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use cache::ComposeCache;
use glam::UVec2;
use std::hash::Hasher as _;

pub(crate) mod cache;

/// Cold-frame state captured at `EnterSubtree` so `ExitSubtree` can
/// write the snapshot back. Mirrors `encoder::SubtreeFrame` (same
/// shape, different per-cache key fields).
struct SubtreeFrame {
    wid: WidgetId,
    subtree_hash: NodeHash,
    avail: AvailableKey,
    cascade_fp: u64,
    quads_lo: u32,
    texts_lo: u32,
    groups_lo: u32,
}

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
    rounded: Option<Corners>,
    quads_start: u32,
    texts_start: u32,
    /// `true` iff the most recent draw appended to the in-flight
    /// group was a text run. Mutated only by `GroupBuilder` methods —
    /// keep private so the text-then-quad split rule stays a struct
    /// invariant rather than caller discipline.
    last_was_text: bool,
}

impl GroupBuilder {
    /// Push the in-flight group into `out.groups` (if non-empty),
    /// rebase `quads_start` / `texts_start` onto the current `out`
    /// lengths, and clear `last_was_text`. Scissor + rounded clip are
    /// preserved.
    fn flush(&mut self, out: &mut RenderBuffer) {
        let q_end = out.quads.len() as u32;
        let t_end = out.texts.len() as u32;
        if q_end > self.quads_start || t_end > self.texts_start {
            out.groups.push(DrawGroup {
                scissor: self.current,
                rounded_clip: self.rounded,
                quads: (self.quads_start..q_end).into(),
                texts: (self.texts_start..t_end).into(),
            });
        }
        self.quads_start = q_end;
        self.texts_start = t_end;
        self.last_was_text = false;
    }

    /// Switch to a new clip (scissor + optional rounded), flushing the
    /// in-flight group only if anything differs. Always clears
    /// `last_was_text` — a clip transition is a draw boundary even
    /// when the resolved state happens to equal the current one
    /// (matches the pre-builder behavior).
    fn set_clip(
        &mut self,
        scissor: Option<URect>,
        rounded: Option<Corners>,
        out: &mut RenderBuffer,
    ) {
        if scissor != self.current || rounded != self.rounded {
            self.flush(out);
            self.current = scissor;
            self.rounded = rounded;
        }
        self.last_was_text = false;
    }

    /// Rebase `quads_start` / `texts_start` onto the current `out`
    /// lengths without pushing a group. Used after
    /// `ComposeCache::try_splice` extends `out` — the splice's own
    /// `DrawGroup`s are already sealed in `out.groups`, so we just
    /// continue from the splice tail under the unchanged scissor.
    /// Caller flushed before splicing, so `last_was_text` is already
    /// false here.
    fn rebase(&mut self, out: &RenderBuffer) {
        self.quads_start = out.quads.len() as u32;
        self.texts_start = out.texts.len() as u32;
    }

    /// Apply the text-then-quad split rule: if the prior draw in the
    /// current group was text, flush so the next quad renders
    /// *after* the text. Same scissor continues into the new group.
    fn before_quad(&mut self, out: &mut RenderBuffer) {
        if self.last_was_text {
            self.flush(out);
        }
    }

    /// Sole entry point for emitting a text run — appends to
    /// `out.texts` and flags `last_was_text` so the next quad triggers
    /// the text-then-quad split. Routing through this method keeps the
    /// flag a struct invariant.
    fn push_text(&mut self, out: &mut RenderBuffer, run: TextRun) {
        out.texts.push(run);
        self.last_was_text = true;
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
    subtree_stack: Vec<SubtreeFrame>,
    pub(crate) cache: ComposeCache,
    pub(crate) buffer: RenderBuffer,
}

#[derive(Clone, Copy)]
struct ClipFrame {
    scissor: URect,
    rounded: Option<Corners>,
}

impl Composer {
    /// Consume a logical-px command stream → physical-px `Quad`s +
    /// `TextRun`s + draw groups (scissor ranges) into the composer's
    /// owned buffer, and return a borrow of the freshly-composed
    /// result. Pure: no device, no queue.
    pub(crate) fn compose(&mut self, cmds: &RenderCmdBuffer, display: &Display) -> &RenderBuffer {
        let out = &mut self.buffer;
        let scale = display.scale_factor;
        let snap = display.pixel_snap;
        let viewport_phys = display.physical;
        let viewport_phys_f = viewport_phys.as_vec2();

        out.quads.clear();
        out.texts.clear();
        out.groups.clear();
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;
        out.scale = scale;
        out.has_rounded_clip = false;

        self.clip_stack.clear();
        self.transform_stack.clear();
        self.subtree_stack.clear();
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
                            out.has_rounded_clip = true;
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
                        Some(logical_radius.scaled_by(phys_scale))
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
                    let (rect, radius, fill, stroke) = match kind {
                        CmdKind::DrawRect => {
                            let p: DrawRectPayload = cmds.read(start);
                            (p.rect, p.radius, p.fill, None)
                        }
                        _ => {
                            let p: DrawRectStrokedPayload = cmds.read(start);
                            (p.rect, p.radius, p.fill, Some(p.stroke))
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
                    group.before_quad(out);
                    let world_radius = radius.scaled_by(current_transform.scale);
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let phys_radius = world_radius.scaled_by(scale);
                    let phys_stroke = stroke.map(|s| Stroke {
                        width: s.width * current_transform.scale * scale,
                        color: s.color,
                    });
                    out.quads
                        .push(Quad::new(phys_rect, fill, phys_radius, phys_stroke));
                }
                CmdKind::EnterSubtree => {
                    let payload: EnterSubtreePayload = cmds.read(start);
                    let wid = payload.wid;
                    let subtree_hash = payload.subtree_hash;
                    let avail = payload.avail;
                    let cascade_fp = cascade_fingerprint(
                        current_transform,
                        self.clip_stack.last().map(|f| f.scissor),
                        scale,
                        snap,
                        viewport_phys,
                    );

                    // Finalize the parent's accumulated group BEFORE
                    // splicing or recording a fresh subtree. Without
                    // this, the cached subtree's first group would
                    // merge with the parent's tail and break the
                    // splice's group ranges.
                    group.flush(out);

                    if self
                        .cache
                        .try_splice(wid, subtree_hash, avail, cascade_fp, out)
                    {
                        // Splice's `DrawGroup`s are sealed; continue
                        // the open group from the post-splice tail
                        // under the unchanged scissor. Fast-forward
                        // past the cached cmd range to its matching
                        // `ExitSubtree` at `payload.exit_idx`.
                        group.rebase(out);
                        i = payload.exit_idx as usize + 1;
                        continue;
                    }

                    // Miss: record where the subtree's contributions
                    // start so `ExitSubtree` can snapshot them.
                    self.subtree_stack.push(SubtreeFrame {
                        wid,
                        subtree_hash,
                        avail,
                        cascade_fp,
                        quads_lo: out.quads.len() as u32,
                        texts_lo: out.texts.len() as u32,
                        groups_lo: out.groups.len() as u32,
                    });
                }
                CmdKind::ExitSubtree => {
                    // Finalize the inner subtree's last open group so
                    // its full output is captured before snapshotting.
                    group.flush(out);

                    if let Some(frame) = self.subtree_stack.pop() {
                        self.cache.write_subtree(
                            frame.wid,
                            frame.subtree_hash,
                            frame.avail,
                            frame.cascade_fp,
                            &out.quads[frame.quads_lo as usize..],
                            &out.texts[frame.texts_lo as usize..],
                            &out.groups[frame.groups_lo as usize..],
                            frame.quads_lo,
                            frame.texts_lo,
                        );
                    }
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
                        },
                    );
                }
            }
            i += 1;
        }
        group.flush(out);

        &self.buffer
    }

    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        self.cache.sweep_removed(removed);
    }
}

/// Hash the cascade inputs that the subtree's physical-px output
/// depends on. Any change here misses the cache; equality round-trips
/// to a byte-identical splice. `f32` fields hash by bit-pattern so
/// `-0.0 != 0.0` distinctions don't get folded. `viewport` is in the
/// key because `scissor_from_logical` clamps text-run bounds against
/// it — a window resize at constant DPI changes those clamps without
/// touching `scale`, so without this a stale snapshot would splice.
#[inline]
fn cascade_fingerprint(
    t: TranslateScale,
    parent_scissor: Option<URect>,
    scale: f32,
    snap: bool,
    viewport: UVec2,
) -> u64 {
    let mut h = Hasher::new();
    h.pod(&t);
    match parent_scissor {
        None => h.write_u8(0),
        // `URect` lacks bytemuck derives; pod a fixed-size view of its
        // four u32s instead.
        Some(r) => {
            h.write_u8(1);
            h.pod(&[r.x, r.y, r.w, r.h]);
        }
    }
    h.write_u32(scale.to_bits());
    h.write_u8(snap as u8);
    h.pod(&viewport);
    h.finish()
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
