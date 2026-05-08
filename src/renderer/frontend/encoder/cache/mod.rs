//! Cross-frame encode cache (Phase 3 of the cross-frame cache series).
//! Subtree-skip on the encoder, mirroring [`MeasureCache`]: same
//! arena+snapshot shape, same in-place-on-match / append-on-mismatch
//! write path, same `live × COMPACT_RATIO` compaction trigger. See
//! `src/renderer/frontend/encoder/encode-cache.md` and
//! `src/layout/measure-cache.md`.
//!
//! Storage layout: three SoA arenas — `kinds` and `starts` (parallel,
//! length = total cached cmds across all snapshots) and `data`
//! (length = total cached payload words). Per-`WidgetId`
//! [`EncodeSnapshot`] picks two contiguous ranges out of those.
//! Bookkeeping (`live` count, compaction trigger) uses [`LiveArena`];
//! `starts` rides on `kinds`'s live count by the parallel-length
//! invariant.
//!
//! **Subtree-relative storage**: `data` stores `rect.min` with the
//! snapshot root's `origin` already subtracted. On replay the caller
//! (encoder) translates back by the *current* frame's root origin, so
//! a cached subtree survives parent origin shifts (scroll, resize,
//! reflowed siblings) without invalidating. Net offset over an
//! unchanged frame is zero — replay is byte-identical to a cold encode.
//!
//! `starts` stores **subtree-relative** payload offsets — i.e. offsets
//! into `data[snap.data.range()]` rather than into the whole arena.
//! Compaction can therefore move a snapshot's range without touching
//! the starts.
//!
//! [`MeasureCache`]: crate::layout::cache::MeasureCache
//! [`EncodeSnapshot`]: EncodeSnapshot

use crate::common::cache_arena::LiveArena;
use crate::layout::cache::AvailableKey;
use crate::layout::types::span::Span;
use crate::renderer::frontend::cmd_buffer::{CmdKind, EnterSubtreePayload, RenderCmdBuffer};
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::Vec2;
use rustc_hash::FxHashMap;

/// 32-byte snapshot. `cmds` indexes the parallel (`kinds`, `starts`);
/// `data` indexes `data`. Both `subtree_hash` and `available_q` are
/// required equal at lookup time. A snapshot exists only for nodes
/// where `LayoutResult::available_q(id)` was `Some` — so a `wid`
/// being present in `snapshots` implies layout has a known available
/// size for it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodeSnapshot {
    pub(crate) subtree_hash: NodeHash,
    pub(crate) available_q: AvailableKey,
    pub(crate) cmds: Span,
    pub(crate) data: Span,
}

/// What [`EncodeCache::try_lookup`] returns on a hit. Slices borrow
/// directly into the cache arenas. Internal — production calls go
/// through [`EncodeCache::try_replay`].
struct CachedEncode<'a> {
    kinds: &'a [CmdKind],
    starts: &'a [u32],
    data: &'a [u32],
}

#[derive(Default)]
pub(crate) struct EncodeCache {
    pub(crate) kinds: LiveArena<CmdKind>,
    // `starts.items` is parallel to `kinds.items` (same length always);
    // its `live` count rides on `kinds.live` by invariant.
    pub(crate) starts: Vec<u32>,
    pub(crate) data: LiveArena<u32>,
    pub(crate) snapshots: FxHashMap<WidgetId, EncodeSnapshot>,
}

impl EncodeCache {
    #[inline]
    fn try_lookup(
        &self,
        wid: WidgetId,
        curr_hash: NodeHash,
        curr_avail: AvailableKey,
    ) -> Option<CachedEncode<'_>> {
        let snap = self.snapshots.get(&wid)?;
        if snap.subtree_hash != curr_hash || snap.available_q != curr_avail {
            return None;
        }
        Some(CachedEncode {
            kinds: &self.kinds.items[snap.cmds.range()],
            starts: &self.starts[snap.cmds.range()],
            data: &self.data.items[snap.data.range()],
        })
    }

    /// Replay `wid`'s cached subtree into `buf` at `offset`. Returns
    /// `true` on hit (cmds appended), `false` on miss (`buf`
    /// untouched). Single-method replay — encoder doesn't need to see
    /// the [`CachedEncode`] borrow.
    ///
    /// On a hit: appends `kinds` / `starts` / `data` to `buf`,
    /// rebasing each `start` onto `buf`'s data arena and shifting
    /// every rect-bearing payload's `rect.min` by `offset`. Pops carry
    /// no payload; PushTransform carries a `TranslateScale` that is
    /// subtree-local — both pass through untouched.
    #[inline]
    pub(crate) fn try_replay(
        &self,
        wid: WidgetId,
        hash: NodeHash,
        avail: AvailableKey,
        buf: &mut RenderCmdBuffer,
        offset: Vec2,
    ) -> bool {
        let Some(hit) = self.try_lookup(wid, hash, avail) else {
            return false;
        };
        let kinds_base = buf.kinds.len() as u32;
        let dest_data_base = buf.data.len() as u32;
        buf.data.extend_from_slice(hit.data);
        buf.kinds.extend_from_slice(hit.kinds);
        buf.starts.reserve(hit.starts.len());
        // Stored starts are subtree-relative offsets into `hit.data`,
        // bounded at write time. Debug-only check — paying a comparison
        // per cmd in release would dominate the replay loop on hit-heavy
        // frames.
        for &s in hit.starts {
            debug_assert!(s as usize <= hit.data.len());
            buf.starts.push(s + dest_data_base);
        }
        let n = hit.kinds.len();
        let appended_starts = &buf.starts[buf.starts.len() - n..];
        bump_rect_min(hit.kinds, appended_starts, &mut buf.data, offset);
        // Snapshot stores exit_idx as snapshot-relative; rebase to live
        // kind-index by adding the position the snapshot was appended at.
        bump_exit_idx(hit.kinds, appended_starts, &mut buf.data, kinds_base as i64);
        true
    }

    /// Insert or overwrite `wid`'s snapshot from `src`'s freshly-encoded
    /// `src_cmds` / `src_data` spans. `origin` is the snapshot root's
    /// arranged `min` — subtracted from each rect-bearing payload's
    /// `rect.min` so storage is subtree-relative.
    ///
    /// Hot path: same `subtree_hash` ⇒ identical cmd shape and payload
    /// sizes, so the existing arena ranges fit byte-for-byte and we
    /// rewrite in place. Size mismatch (rare — only when authoring
    /// changes the subtree's structure) marks the old ranges as garbage
    /// and appends fresh ones.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn write_subtree(
        &mut self,
        wid: WidgetId,
        subtree_hash: NodeHash,
        available_q: AvailableKey,
        src: &RenderCmdBuffer,
        src_cmds: Span,
        src_data: Span,
        origin: Vec2,
    ) {
        let neg_origin = -origin;
        let src_cmd_range = src_cmds.range();
        let src_data_range = src_data.range();

        // Single hashmap probe: hot path takes it for in-place rewrite,
        // slow path captures the prior snapshot's lengths so we can
        // decrement live counters without re-probing before the append.
        let prev_lens = if let Some(prev) = self.snapshots.get_mut(&wid) {
            // Hot path: same `(subtree_hash, available_q)` ⇒ same wrap
            // targets ⇒ same cmd shape and payload sizes, so the
            // existing arena ranges fit byte-for-byte. Hash equality
            // is the invariant the kind-shape match relies on; with
            // different hash, kinds may legitimately swap (e.g. a
            // TextEdit replacing its placeholder Text with a focused-
            // caret Overlay — same count, different variants), so we
            // must fall through to the slow append.
            let same_key = prev.subtree_hash == subtree_hash && prev.available_q == available_q;
            if same_key && prev.cmds.len == src_cmds.len && prev.data.len == src_data.len {
                let cmds = prev.cmds.range();
                let data = prev.data.range();
                let src_kinds = &src.kinds[src_cmd_range.clone()];
                // Debug-only kind-shape check. The only failure mode is a
                // 64-bit FxHash collision (~1 in 2^64) or a future hash
                // bug — not worth a slice memcmp per in-place write in
                // release.
                debug_assert_eq!(&self.kinds.items[cmds.clone()], src_kinds);
                self.kinds.items[cmds.clone()].copy_from_slice(src_kinds);
                for (dst, &abs) in self.starts[cmds.clone()]
                    .iter_mut()
                    .zip(src.starts[src_cmd_range].iter())
                {
                    *dst = abs - src_data.start;
                }
                self.data.items[data.clone()].copy_from_slice(&src.data[src_data_range]);
                bump_rect_min(
                    &self.kinds.items[cmds.clone()],
                    &self.starts[cmds.clone()],
                    &mut self.data.items[data.clone()],
                    neg_origin,
                );
                bump_exit_idx(
                    &self.kinds.items[cmds.clone()],
                    &self.starts[cmds],
                    &mut self.data.items[data],
                    -(src_cmds.start as i64),
                );
                return;
            }
            Some((prev.cmds.len, prev.data.len))
        } else {
            None
        };

        // Different len (or first write): mark old ranges as garbage,
        // append new ones. The trailing `insert` overwrites any prior
        // snapshot at this wid in a single probe.
        if let Some((cmds_len, data_len)) = prev_lens {
            self.kinds.release(cmds_len);
            self.data.release(data_len);
        }
        let cmds_span = Span::new(self.kinds.items.len() as u32, src_cmds.len);
        let data_span = Span::new(self.data.items.len() as u32, src_data.len);

        self.kinds
            .items
            .extend_from_slice(&src.kinds[src_cmd_range.clone()]);
        self.starts.reserve(src_cmds.len as usize);
        for &abs in &src.starts[src_cmd_range.clone()] {
            self.starts.push(abs - src_data.start);
        }
        self.data.items.extend_from_slice(&src.data[src_data_range]);

        bump_rect_min(
            &self.kinds.items[cmds_span.range()],
            &self.starts[cmds_span.range()],
            &mut self.data.items[data_span.range()],
            neg_origin,
        );
        bump_exit_idx(
            &self.kinds.items[cmds_span.range()],
            &self.starts[cmds_span.range()],
            &mut self.data.items[data_span.range()],
            -(src_cmds.start as i64),
        );

        self.kinds.acquire(src_cmds.len);
        self.data.acquire(src_data.len);
        self.snapshots.insert(
            wid,
            EncodeSnapshot {
                subtree_hash,
                available_q,
                cmds: cmds_span,
                data: data_span,
            },
        );

        if self.kinds.needs_compact() || self.data.needs_compact() {
            self.compact();
        }
    }

    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.kinds.release(snap.cmds.len);
                self.data.release(snap.data.len);
            }
        }
    }

    /// Drop every snapshot and free all arena storage. Reachable only
    /// via `internals::clear_encode_cache` (gated to tests + the
    /// `internals` feature) — not part of any production code path.
    #[cfg(any(test, feature = "internals"))]
    pub(crate) fn clear(&mut self) {
        self.kinds.clear();
        self.starts.clear();
        self.data.clear();
        self.snapshots.clear();
    }

    /// Rare path: only fires when an arena exceeds `live × COMPACT_RATIO`
    /// AND lives above `COMPACT_FLOOR`. Allocating fresh `Vec`s sized
    /// to `live` is cheaper than reusing scratch (which would carry
    /// the larger pre-compact capacity until the next allocation
    /// shrink) — revisit if compaction shows up in a profile.
    fn compact(&mut self) {
        let mut new_kinds: Vec<CmdKind> = Vec::with_capacity(self.kinds.live);
        let mut new_starts: Vec<u32> = Vec::with_capacity(self.kinds.live);
        let mut new_data: Vec<u32> = Vec::with_capacity(self.data.live);
        for snap in self.snapshots.values_mut() {
            let cmds = snap.cmds.range();
            let data = snap.data.range();
            // Starts are subtree-relative — copying without rewrite is
            // sufficient. Compaction moves the *range*, not the
            // intra-range offsets.
            snap.cmds.start = new_kinds.len() as u32;
            snap.data.start = new_data.len() as u32;
            new_kinds.extend_from_slice(&self.kinds.items[cmds.clone()]);
            new_starts.extend_from_slice(&self.starts[cmds]);
            new_data.extend_from_slice(&self.data.items[data]);
        }
        self.kinds.items = new_kinds;
        self.starts = new_starts;
        self.data.items = new_data;
    }
}

/// Add `offset` to `rect.min` for every rect-bearing cmd in `kinds`,
/// reading the payload offset from the parallel `starts` slice.
///
/// `rect.min` lives at the first 8 bytes (= 2 u32 words) of every
/// payload that begins with `rect: Rect` — `Rect` is `#[repr(C)]
/// { min: Vec2, size: Size }`. Read/write through `f32::{from,to}_bits`
/// so we don't depend on the arena's u32 alignment lining up with f32
/// (it does, but staying bits-only matches the rest of the buffer's
/// discipline).
#[inline]
fn bump_rect_min(kinds: &[CmdKind], starts: &[u32], data: &mut [u32], offset: Vec2) {
    assert_eq!(kinds.len(), starts.len());
    for (kind, &start) in kinds.iter().zip(starts.iter()) {
        if !kind.has_leading_rect() {
            continue;
        }
        let off = start as usize;
        let x = f32::from_bits(data[off]) + offset.x;
        let y = f32::from_bits(data[off + 1]) + offset.y;
        data[off] = x.to_bits();
        data[off + 1] = y.to_bits();
    }
}

/// Shift every `EnterSubtree`'s `exit_idx` payload field by `delta`.
/// Used when moving cached cmds between buffers: the exit_idx is stored
/// as an absolute kind-index in the source buffer, so it must be
/// rebased on every write/replay so the composer's fast-forward lands
/// on the matching ExitSubtree in the destination buffer.
///
/// Storage convention: snapshots store exit_idx as **snapshot-relative**
/// (subtract `cmd_start_at_write` at write time, add `kinds_start_at_replay`
/// at replay time). Without this, a cached subtree replayed at a different
/// absolute buffer position fast-forwards to the wrong cmd, leaving
/// unmatched `Push/PopClip` pairs and panicking the composer.
fn bump_exit_idx(kinds: &[CmdKind], starts: &[u32], data: &mut [u32], delta: i64) {
    assert_eq!(kinds.len(), starts.len());
    if delta == 0 {
        return;
    }
    const EXIT_IDX_WORD: usize =
        std::mem::offset_of!(EnterSubtreePayload, exit_idx) / size_of::<u32>();
    for (kind, &start) in kinds.iter().zip(starts.iter()) {
        if matches!(kind, CmdKind::EnterSubtree) {
            let off = start as usize + EXIT_IDX_WORD;
            let cur = data[off] as i64;
            data[off] = (cur + delta) as u32;
        }
    }
}

#[cfg(test)]
mod tests;
