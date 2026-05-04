use super::cmd_buffer::{
    CmdKind, DrawRectPayload, DrawRectStrokedPayload, DrawTextPayload, EnterSubtreePayload,
    RenderCmdBuffer,
};
use crate::layout::cache::AvailableKey;
use crate::layout::types::display::Display;
use crate::primitives::{rect::Rect, stroke::Stroke, transform::TranslateScale, urect::URect};
use crate::renderer::gpu::buffer::{DrawGroup, RenderBuffer, TextRun};
use crate::renderer::gpu::quad::Quad;
use crate::tree::hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use cache::ComposeCache;
use rustc_hash::FxHasher;
use std::hash::{Hash, Hasher};

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
    clip_stack: Vec<URect>,
    transform_stack: Vec<TranslateScale>,
    subtree_stack: Vec<SubtreeFrame>,
    pub(crate) cache: ComposeCache,
    pub(crate) buffer: RenderBuffer,
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
        let viewport_phys_f = [display.physical.x as f32, display.physical.y as f32];
        let viewport_phys = [display.physical.x, display.physical.y];

        out.quads.clear();
        out.texts.clear();
        out.groups.clear();
        out.viewport_phys = viewport_phys;
        out.viewport_phys_f = viewport_phys_f;
        out.scale = scale;

        self.clip_stack.clear();
        self.transform_stack.clear();
        self.subtree_stack.clear();
        let mut current_transform = TranslateScale::IDENTITY;
        let mut current: Option<URect> = None;
        let mut quads_start: u32 = 0;
        let mut texts_start: u32 = 0;
        // Tracks whether the most recent draw in the active group was
        // text. A subsequent quad must start a new group so the prior
        // group's text renders BEFORE that quad — otherwise the quad
        // and the prior group's quads batch together and the text
        // floats on top. Reset on scissor switch (group already
        // flushed) and on flush.
        let mut last_was_text = false;

        let n = cmds.kinds.len();
        let mut i = 0usize;
        while i < n {
            let kind = cmds.kinds[i];
            let start = cmds.starts[i];
            match kind {
                CmdKind::PushClip => {
                    let r: Rect = cmds.read(start);
                    let world = current_transform.apply_rect(r);
                    let me = scissor_from_logical(world, scale, snap, viewport_phys);
                    let new = match self.clip_stack.last() {
                        Some(parent) => me.clamp_to(*parent),
                        None => me,
                    };
                    self.clip_stack.push(new);
                    switch_group(
                        Some(new),
                        &mut current,
                        &mut quads_start,
                        &mut texts_start,
                        out.quads.len() as u32,
                        out.texts.len() as u32,
                        &mut out.groups,
                    );
                    last_was_text = false;
                }
                CmdKind::PopClip => {
                    self.clip_stack
                        .pop()
                        .expect("PopClip without matching PushClip");
                    switch_group(
                        self.clip_stack.last().copied(),
                        &mut current,
                        &mut quads_start,
                        &mut texts_start,
                        out.quads.len() as u32,
                        out.texts.len() as u32,
                        &mut out.groups,
                    );
                    last_was_text = false;
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
                    if last_was_text {
                        // Flush the current group so this quad renders
                        // *after* the prior group's text. Same scissor
                        // continues into the new group.
                        flush_group(
                            current,
                            quads_start,
                            out.quads.len() as u32,
                            texts_start,
                            out.texts.len() as u32,
                            &mut out.groups,
                        );
                        quads_start = out.quads.len() as u32;
                        texts_start = out.texts.len() as u32;
                        last_was_text = false;
                    }
                    let world_rect = current_transform.apply_rect(rect);
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
                        self.clip_stack.last().copied(),
                        scale,
                        snap,
                    );

                    // Finalize the parent's accumulated group BEFORE we
                    // either splice cached output or start recording a
                    // fresh subtree. Without this, the cached subtree's
                    // first group would be merged with the parent's
                    // tail, breaking the splice's group ranges.
                    flush_group(
                        current,
                        quads_start,
                        out.quads.len() as u32,
                        texts_start,
                        out.texts.len() as u32,
                        &mut out.groups,
                    );
                    quads_start = out.quads.len() as u32;
                    texts_start = out.texts.len() as u32;
                    last_was_text = false;

                    if self
                        .cache
                        .try_splice(wid, subtree_hash, avail, cascade_fp, out)
                    {
                        // Open group continues from the splice tail
                        // under the parent's `current` scissor.
                        quads_start = out.quads.len() as u32;
                        texts_start = out.texts.len() as u32;
                        // Fast-forward past the cached cmd range — the
                        // matching `ExitSubtree` is at `payload.exit_idx`.
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
                    flush_group(
                        current,
                        quads_start,
                        out.quads.len() as u32,
                        texts_start,
                        out.texts.len() as u32,
                        &mut out.groups,
                    );
                    quads_start = out.quads.len() as u32;
                    texts_start = out.texts.len() as u32;
                    last_was_text = false;

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
                    let phys_rect = world_rect.scaled_by(scale, snap);
                    let bounds = scissor_from_logical(world_rect, scale, snap, viewport_phys);
                    out.texts.push(TextRun {
                        origin: phys_rect.min,
                        bounds,
                        color: t.color,
                        key: t.key,
                    });
                    last_was_text = true;
                }
            }
            i += 1;
        }
        flush_group(
            current,
            quads_start,
            out.quads.len() as u32,
            texts_start,
            out.texts.len() as u32,
            &mut out.groups,
        );

        &self.buffer
    }
}

/// Hash the cascade inputs that the subtree's physical-px output
/// depends on. Any change here misses the cache; equality round-trips
/// to a byte-identical splice. `f32` fields hash by bit-pattern so
/// `-0.0 != 0.0` distinctions don't get folded.
#[inline]
fn cascade_fingerprint(
    t: TranslateScale,
    parent_scissor: Option<URect>,
    scale: f32,
    snap: bool,
) -> u64 {
    let mut h = FxHasher::default();
    t.translation.x.to_bits().hash(&mut h);
    t.translation.y.to_bits().hash(&mut h);
    t.scale.to_bits().hash(&mut h);
    match parent_scissor {
        None => 0u8.hash(&mut h),
        Some(r) => {
            1u8.hash(&mut h);
            r.x.hash(&mut h);
            r.y.hash(&mut h);
            r.w.hash(&mut h);
            r.h.hash(&mut h);
        }
    }
    scale.to_bits().hash(&mut h);
    snap.hash(&mut h);
    h.finish()
}

#[allow(clippy::too_many_arguments)]
fn switch_group(
    target: Option<URect>,
    current: &mut Option<URect>,
    quads_start: &mut u32,
    texts_start: &mut u32,
    quads_end: u32,
    texts_end: u32,
    groups: &mut Vec<DrawGroup>,
) {
    if target != *current {
        flush_group(
            *current,
            *quads_start,
            quads_end,
            *texts_start,
            texts_end,
            groups,
        );
        *current = target;
        *quads_start = quads_end;
        *texts_start = texts_end;
    }
}

fn flush_group(
    scissor: Option<URect>,
    quads_start: u32,
    quads_end: u32,
    texts_start: u32,
    texts_end: u32,
    groups: &mut Vec<DrawGroup>,
) {
    if quads_end > quads_start || texts_end > texts_start {
        groups.push(DrawGroup {
            scissor,
            quads: quads_start..quads_end,
            texts: texts_start..texts_end,
        });
    }
}

fn scissor_from_logical(r: Rect, scale: f32, snap: bool, viewport: [u32; 2]) -> URect {
    let phys = r.scaled_by(scale, snap);
    let x = (phys.min.x.max(0.0) as u32).min(viewport[0]);
    let y = (phys.min.y.max(0.0) as u32).min(viewport[1]);
    let right = ((phys.min.x + phys.size.w).max(0.0) as u32).min(viewport[0]);
    let bottom = ((phys.min.y + phys.size.h).max(0.0) as u32).min(viewport[1]);
    URect {
        x,
        y,
        w: right.saturating_sub(x),
        h: bottom.saturating_sub(y),
    }
}

#[cfg(test)]
mod tests;
