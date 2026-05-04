//! Cross-frame composer cache (Phase 4 of the cross-frame cache series).
//! Subtree-skip on the composer, mirroring [`EncodeCache`]: same
//! arena+snapshot shape, same in-place-on-match / append-on-mismatch
//! write path, same `live × COMPACT_RATIO` compaction trigger. See
//! `docs/composer-cache.md`.
//!
//! Storage layout: three SoA arenas — `quads_arena`, `texts_arena`,
//! `groups_arena`. Per-`WidgetId` [`ComposeSnapshot`] picks a
//! contiguous range out of each.
//!
//! **Cascade-keyed.** Unlike the encode cache, the snapshot key
//! includes a `cascade_fp` over `(current_transform, parent_scissor,
//! display.scale_factor, display.pixel_snap)` — the ancestor state
//! the subtree's physical-px output depends on. Any change in those
//! inputs (parent scroll, transform animation, DPI flip, snap toggle)
//! misses; the encoder still hits because its subtree-relative
//! storage absorbs origin shifts.
//!
//! **Subtree-relative groups.** `groups_arena` stores each
//! [`DrawGroup`]'s `quads`/`texts` ranges with the snapshot's start
//! offsets already subtracted. On replay the splicer adds the
//! current frame's `out.quads.len()` / `out.texts.len()` back, so a
//! cached snapshot survives changes to the *number* of quads/texts
//! the parent emitted before it.
//!
//! [`EncodeCache`]: crate::renderer::frontend::encoder::cache::EncodeCache

use crate::layout::cache::AvailableKey;
use crate::layout::types::span::Span;
use crate::renderer::gpu::buffer::{DrawGroup, TextRun};
use crate::renderer::gpu::quad::Quad;
use crate::tree::hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use rustc_hash::FxHashMap;

/// Per-`WidgetId` snapshot. `subtree_hash` and `available_q` and
/// `cascade_fp` must all match at lookup time for a hit.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ComposeSnapshot {
    pub(crate) subtree_hash: NodeHash,
    pub(crate) available_q: AvailableKey,
    pub(crate) cascade_fp: u64,
    pub(crate) quads: Span,
    pub(crate) texts: Span,
    pub(crate) groups: Span,
}

/// What [`ComposeCache::try_lookup`] returns on a hit. Slices borrow
/// directly into the cache arenas; the caller (composer) splices them
/// into the live `RenderBuffer`, rebasing each group's intra-snapshot
/// `quads` / `texts` range by the current frame's live offsets.
pub(crate) struct CachedCompose<'a> {
    pub(crate) quads: &'a [Quad],
    pub(crate) texts: &'a [TextRun],
    pub(crate) groups: &'a [DrawGroup],
}

const COMPACT_RATIO: usize = 2;
const COMPACT_FLOOR: usize = 64;

#[derive(Default)]
pub(crate) struct ComposeCache {
    quads_arena: Vec<Quad>,
    texts_arena: Vec<TextRun>,
    groups_arena: Vec<DrawGroup>,
    pub(crate) snapshots: FxHashMap<WidgetId, ComposeSnapshot>,
    pub(crate) live_quads: usize,
    pub(crate) live_texts: usize,
    pub(crate) live_groups: usize,
}

impl ComposeCache {
    #[inline]
    pub(crate) fn try_lookup(
        &self,
        wid: WidgetId,
        hash: NodeHash,
        avail: AvailableKey,
        cascade_fp: u64,
    ) -> Option<CachedCompose<'_>> {
        let snap = self.snapshots.get(&wid)?;
        if snap.subtree_hash != hash || snap.available_q != avail || snap.cascade_fp != cascade_fp {
            return None;
        }
        Some(CachedCompose {
            quads: &self.quads_arena[snap.quads.range()],
            texts: &self.texts_arena[snap.texts.range()],
            groups: &self.groups_arena[snap.groups.range()],
        })
    }

    /// Insert or overwrite `wid`'s snapshot from the subtree's tail
    /// slices. `tail_*` are exactly what the subtree contributed (the
    /// caller has already sliced past the parent's pre-subtree output).
    /// `rebase_q` / `rebase_t` are the live-buffer `len()`s captured at
    /// `EnterSubtree`; group ranges are stored subtree-relative by
    /// subtracting these.
    ///
    /// Hot path: same length triple ⇒ in-place rewrite. Length change
    /// (rare — only when the subtree's group count, quad count, or
    /// text count differs) marks the old ranges as garbage and
    /// appends fresh ones, triggering compaction if either arena
    /// crosses `COMPACT_RATIO`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_subtree(
        &mut self,
        wid: WidgetId,
        subtree_hash: NodeHash,
        available_q: AvailableKey,
        cascade_fp: u64,
        tail_quads: &[Quad],
        tail_texts: &[TextRun],
        tail_groups: &[DrawGroup],
        rebase_q: u32,
        rebase_t: u32,
    ) {
        let q_len = tail_quads.len() as u32;
        let t_len = tail_texts.len() as u32;
        let g_len = tail_groups.len() as u32;

        if let Some(prev) = self.snapshots.get_mut(&wid)
            && prev.quads.len == q_len
            && prev.texts.len == t_len
            && prev.groups.len == g_len
        {
            let q_range = prev.quads.range();
            let t_range = prev.texts.range();
            let g_range = prev.groups.range();
            prev.subtree_hash = subtree_hash;
            prev.available_q = available_q;
            prev.cascade_fp = cascade_fp;
            self.quads_arena[q_range].copy_from_slice(tail_quads);
            self.texts_arena[t_range].copy_from_slice(tail_texts);
            for (dst, src) in self.groups_arena[g_range]
                .iter_mut()
                .zip(tail_groups.iter())
            {
                *dst = DrawGroup {
                    scissor: src.scissor,
                    quads: (src.quads.start - rebase_q)..(src.quads.end - rebase_q),
                    texts: (src.texts.start - rebase_t)..(src.texts.end - rebase_t),
                };
            }
            return;
        }

        if let Some(prev) = self.snapshots.get(&wid) {
            self.live_quads -= prev.quads.len as usize;
            self.live_texts -= prev.texts.len as usize;
            self.live_groups -= prev.groups.len as usize;
        }
        let q_span = Span::new(self.quads_arena.len() as u32, q_len);
        let t_span = Span::new(self.texts_arena.len() as u32, t_len);
        let g_span = Span::new(self.groups_arena.len() as u32, g_len);

        self.quads_arena.extend_from_slice(tail_quads);
        self.texts_arena.extend_from_slice(tail_texts);
        self.groups_arena.reserve(g_len as usize);
        for src in tail_groups {
            self.groups_arena.push(DrawGroup {
                scissor: src.scissor,
                quads: (src.quads.start - rebase_q)..(src.quads.end - rebase_q),
                texts: (src.texts.start - rebase_t)..(src.texts.end - rebase_t),
            });
        }
        self.live_quads += q_len as usize;
        self.live_texts += t_len as usize;
        self.live_groups += g_len as usize;
        self.snapshots.insert(
            wid,
            ComposeSnapshot {
                subtree_hash,
                available_q,
                cascade_fp,
                quads: q_span,
                texts: t_span,
                groups: g_span,
            },
        );

        let q_over = self.quads_arena.len() > self.live_quads.saturating_mul(COMPACT_RATIO);
        let t_over = self.texts_arena.len() > self.live_texts.saturating_mul(COMPACT_RATIO);
        let g_over = self.groups_arena.len() > self.live_groups.saturating_mul(COMPACT_RATIO);
        if (q_over || t_over || g_over)
            && (self.live_quads > COMPACT_FLOOR
                || self.live_texts > COMPACT_FLOOR
                || self.live_groups > COMPACT_FLOOR)
        {
            self.compact();
        }
    }

    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.live_quads -= snap.quads.len as usize;
                self.live_texts -= snap.texts.len as usize;
                self.live_groups -= snap.groups.len as usize;
            }
        }
    }

    /// Drop every snapshot and free all arena storage. Used by
    /// `Ui::__clear_compose_cache` for benches.
    pub(crate) fn clear(&mut self) {
        self.quads_arena.clear();
        self.texts_arena.clear();
        self.groups_arena.clear();
        self.snapshots.clear();
        self.live_quads = 0;
        self.live_texts = 0;
        self.live_groups = 0;
    }

    fn compact(&mut self) {
        let Self {
            quads_arena,
            texts_arena,
            groups_arena,
            snapshots,
            live_quads,
            live_texts,
            live_groups,
        } = self;
        let mut new_q: Vec<Quad> = Vec::with_capacity(*live_quads);
        let mut new_t: Vec<TextRun> = Vec::with_capacity(*live_texts);
        let mut new_g: Vec<DrawGroup> = Vec::with_capacity(*live_groups);
        for snap in snapshots.values_mut() {
            let q = snap.quads.range();
            let t = snap.texts.range();
            let g = snap.groups.range();
            // Group ranges are subtree-relative — copying without
            // rewrite is sufficient, same as encode-cache `starts`.
            snap.quads.start = new_q.len() as u32;
            snap.texts.start = new_t.len() as u32;
            snap.groups.start = new_g.len() as u32;
            new_q.extend_from_slice(&quads_arena[q]);
            new_t.extend_from_slice(&texts_arena[t]);
            new_g.extend_from_slice(&groups_arena[g]);
        }
        *quads_arena = new_q;
        *texts_arena = new_t;
        *groups_arena = new_g;
    }
}

#[cfg(test)]
mod tests;
