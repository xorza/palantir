//! Cross-frame composer cache (Phase 4 of the cross-frame cache series).
//! Subtree-skip on the composer, mirroring [`EncodeCache`]: same
//! arena+snapshot shape, same in-place-on-match / append-on-mismatch
//! write path, same `live × COMPACT_RATIO` compaction trigger. See
//! `src/renderer/frontend/composer/compose-cache.md`.
//!
//! Storage layout: three SoA arenas — `quads`, `texts`, `groups` (each
//! a [`LiveArena`]). Per-`WidgetId` [`ComposeSnapshot`] picks a
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
//! **Subtree-relative groups.** `groups` stores each [`DrawGroup`]'s
//! `quads`/`texts` ranges with the snapshot's start offsets already
//! subtracted. On replay the splicer adds the current frame's
//! `out.quads.len()` / `out.texts.len()` back, so a cached snapshot
//! survives changes to the *number* of quads/texts the parent emitted
//! before it.
//!
//! [`EncodeCache`]: crate::renderer::frontend::encoder::cache::EncodeCache

use crate::layout::cache::AvailableKey;
use crate::layout::types::span::Span;
use crate::renderer::frontend::cache_arena::LiveArena;
use crate::renderer::gpu::buffer::{DrawGroup, RenderBuffer, TextRun};
use crate::renderer::gpu::quad::Quad;
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use rustc_hash::FxHashMap;

/// Per-`WidgetId` snapshot. `subtree_hash`, `available_q`, and
/// `cascade_fp` must all match at lookup time for a hit. A snapshot
/// exists only for nodes the encoder bracketed with
/// `EnterSubtree`/`ExitSubtree` markers — i.e. the encoder confirmed
/// layout had a known `available_q` — so a `wid` being present in
/// `snapshots` implies layout has a known available size.
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
/// directly into the cache arenas. Internal — production calls go
/// through [`ComposeCache::try_splice`].
struct CachedCompose<'a> {
    quads: &'a [Quad],
    texts: &'a [TextRun],
    groups: &'a [DrawGroup],
}

#[derive(Default)]
pub(crate) struct ComposeCache {
    pub(crate) quads: LiveArena<Quad>,
    pub(crate) texts: LiveArena<TextRun>,
    pub(crate) groups: LiveArena<DrawGroup>,
    pub(crate) snapshots: FxHashMap<WidgetId, ComposeSnapshot>,
}

impl ComposeCache {
    #[inline]
    fn try_lookup(
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
            quads: &self.quads.items[snap.quads.range()],
            texts: &self.texts.items[snap.texts.range()],
            groups: &self.groups.items[snap.groups.range()],
        })
    }

    /// Splice `wid`'s cached subtree into `out`, rebasing each
    /// snapshot group's intra-snapshot `quads`/`texts` ranges by the
    /// current frame's live offsets. Returns `true` on hit (output
    /// appended), `false` on miss (`out` untouched).
    ///
    /// Caller is responsible for the surrounding bookkeeping: flushing
    /// any open group before calling, then resetting its
    /// `quads_start`/`texts_start` markers from `out.quads.len()` /
    /// `out.texts.len()` after, and fast-forwarding past the cached
    /// cmd range.
    #[inline]
    pub(crate) fn try_splice(
        &self,
        wid: WidgetId,
        hash: NodeHash,
        avail: AvailableKey,
        cascade_fp: u64,
        out: &mut RenderBuffer,
    ) -> bool {
        let Some(hit) = self.try_lookup(wid, hash, avail, cascade_fp) else {
            return false;
        };
        let base_q = out.quads.len() as u32;
        let base_t = out.texts.len() as u32;
        out.quads.extend_from_slice(hit.quads);
        out.texts.extend_from_slice(hit.texts);
        out.groups.reserve(hit.groups.len());
        for g in hit.groups {
            out.groups.push(DrawGroup {
                scissor: g.scissor,
                quads: (g.quads.start + base_q)..(g.quads.end + base_q),
                texts: (g.texts.start + base_t)..(g.texts.end + base_t),
            });
        }
        true
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

        // Single hashmap probe: hot path takes it for in-place rewrite,
        // slow path captures the prior snapshot's lengths so we can
        // decrement live counters without re-probing before the append.
        let prev_lens = if let Some(prev) = self.snapshots.get_mut(&wid) {
            if prev.quads.len == q_len && prev.texts.len == t_len && prev.groups.len == g_len {
                let q_range = prev.quads.range();
                let t_range = prev.texts.range();
                let g_range = prev.groups.range();
                prev.subtree_hash = subtree_hash;
                prev.available_q = available_q;
                prev.cascade_fp = cascade_fp;
                self.quads.items[q_range].copy_from_slice(tail_quads);
                self.texts.items[t_range].copy_from_slice(tail_texts);
                for (dst, src) in self.groups.items[g_range]
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
            Some((prev.quads.len, prev.texts.len, prev.groups.len))
        } else {
            None
        };

        if let Some((q, t, g)) = prev_lens {
            self.quads.release(q);
            self.texts.release(t);
            self.groups.release(g);
        }
        let q_span = Span::new(self.quads.items.len() as u32, q_len);
        let t_span = Span::new(self.texts.items.len() as u32, t_len);
        let g_span = Span::new(self.groups.items.len() as u32, g_len);

        self.quads.items.extend_from_slice(tail_quads);
        self.texts.items.extend_from_slice(tail_texts);
        self.groups.items.reserve(g_len as usize);
        for src in tail_groups {
            self.groups.items.push(DrawGroup {
                scissor: src.scissor,
                quads: (src.quads.start - rebase_q)..(src.quads.end - rebase_q),
                texts: (src.texts.start - rebase_t)..(src.texts.end - rebase_t),
            });
        }
        self.quads.live += q_len as usize;
        self.texts.live += t_len as usize;
        self.groups.live += g_len as usize;
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

        let any_over =
            self.quads.is_overgrown() || self.texts.is_overgrown() || self.groups.is_overgrown();
        let any_floor =
            self.quads.over_floor() || self.texts.over_floor() || self.groups.over_floor();
        if any_over && any_floor {
            self.compact();
        }
    }

    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.quads.release(snap.quads.len);
                self.texts.release(snap.texts.len);
                self.groups.release(snap.groups.len);
            }
        }
    }

    /// Drop every snapshot and free all arena storage. Used by
    /// `Ui::__clear_compose_cache` for benches.
    pub(crate) fn clear(&mut self) {
        self.quads.clear();
        self.texts.clear();
        self.groups.clear();
        self.snapshots.clear();
    }

    /// Rare path; same reuse-vs-fresh tradeoff as
    /// `EncodeCache::compact` — revisit only if profile shows it.
    fn compact(&mut self) {
        let mut new_q: Vec<Quad> = Vec::with_capacity(self.quads.live);
        let mut new_t: Vec<TextRun> = Vec::with_capacity(self.texts.live);
        let mut new_g: Vec<DrawGroup> = Vec::with_capacity(self.groups.live);
        for snap in self.snapshots.values_mut() {
            let q = snap.quads.range();
            let t = snap.texts.range();
            let g = snap.groups.range();
            // Group ranges are subtree-relative — copying without
            // rewrite is sufficient, same as encode-cache `starts`.
            snap.quads.start = new_q.len() as u32;
            snap.texts.start = new_t.len() as u32;
            snap.groups.start = new_g.len() as u32;
            new_q.extend_from_slice(&self.quads.items[q]);
            new_t.extend_from_slice(&self.texts.items[t]);
            new_g.extend_from_slice(&self.groups.items[g]);
        }
        self.quads.items = new_q;
        self.texts.items = new_t;
        self.groups.items = new_g;
    }
}

#[cfg(test)]
mod tests;
