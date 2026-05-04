//! Cross-frame encode cache (Phase 3 of the cross-frame cache series).
//! Subtree-skip on the encoder, mirroring [`MeasureCache`]: same
//! arena+snapshot shape, same in-place-on-match / append-on-mismatch
//! write path, same `live × COMPACT_RATIO` compaction trigger. See
//! `src/renderer/frontend/encoder/encode-cache.md` and
//! `src/layout/measure-cache.md`.
//!
//! Storage layout: three SoA arenas — `kinds_arena`, `starts_arena`
//! (parallel, length = total cached cmds across all snapshots) and
//! `data_arena` (length = total cached payload words). Per-`WidgetId`
//! [`EncodeSnapshot`] picks two contiguous ranges out of those.
//!
//! **Subtree-relative storage**: `data_arena` stores `rect.min` with
//! the snapshot root's `origin` already subtracted. On replay the
//! caller (encoder) translates back by the *current* frame's root
//! origin, so a cached subtree survives parent origin shifts (scroll,
//! resize, reflowed siblings) without invalidating. Net offset over an
//! unchanged frame is zero — replay is byte-identical to a cold encode.
//!
//! `starts_arena` stores **subtree-relative** payload offsets — i.e.
//! offsets into `data_arena[snap.data.range()]` rather than into the
//! whole arena. Compaction can therefore move a snapshot's range
//! without touching the starts.
//!
//! [`MeasureCache`]: crate::layout::cache::MeasureCache
//! [`EncodeSnapshot`]: EncodeSnapshot

use crate::layout::cache::AvailableKey;
use crate::layout::types::span::Span;
use crate::renderer::frontend::cmd_buffer::{CmdKind, RenderCmdBuffer, bump_rect_min};
use crate::tree::hash::NodeHash;
use crate::tree::widget_id::WidgetId;
use glam::Vec2;
use rustc_hash::FxHashMap;

/// 32-byte snapshot. `cmds` indexes the parallel
/// (`kinds_arena`, `starts_arena`); `data` indexes `data_arena`. Both
/// `subtree_hash` and `available_q` are required equal at lookup time.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EncodeSnapshot {
    pub(crate) subtree_hash: NodeHash,
    pub(crate) available_q: AvailableKey,
    pub(crate) cmds: Span,
    pub(crate) data: Span,
}

/// What [`EncodeCache::try_lookup`] returns on a hit. Slices borrow
/// directly into the cache arenas — caller threads them into
/// [`RenderCmdBuffer::extend_from_cached`] with the current root's
/// origin to replay.
pub(crate) struct CachedEncode<'a> {
    pub(crate) kinds: &'a [CmdKind],
    pub(crate) starts: &'a [u32],
    pub(crate) data: &'a [u32],
}

const COMPACT_RATIO: usize = 2;
const COMPACT_FLOOR: usize = 64;

#[derive(Default)]
pub(crate) struct EncodeCache {
    kinds_arena: Vec<CmdKind>,
    starts_arena: Vec<u32>,
    data_arena: Vec<u32>,
    snapshots: FxHashMap<WidgetId, EncodeSnapshot>,
    live_cmds: usize,
    live_data: usize,
}

impl EncodeCache {
    #[inline]
    pub(crate) fn try_lookup(
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
            kinds: &self.kinds_arena[snap.cmds.range()],
            starts: &self.starts_arena[snap.cmds.range()],
            data: &self.data_arena[snap.data.range()],
        })
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

        if let Some(prev) = self.snapshots.get_mut(&wid)
            && prev.cmds.len == src_cmds.len
            && prev.data.len == src_data.len
        {
            // In-place: hot path. Same subtree_hash → identical layout.
            let cmds = prev.cmds.range();
            let data = prev.data.range();
            prev.subtree_hash = subtree_hash;
            prev.available_q = available_q;
            let Self {
                kinds_arena,
                starts_arena,
                data_arena,
                ..
            } = self;
            let src_kinds = &src.kinds[src_cmd_range.clone()];
            debug_assert_eq!(&kinds_arena[cmds.clone()], src_kinds);
            kinds_arena[cmds.clone()].copy_from_slice(src_kinds);
            for (dst, &abs) in starts_arena[cmds]
                .iter_mut()
                .zip(src.starts[src_cmd_range].iter())
            {
                *dst = abs - src_data.start;
            }
            data_arena[data.clone()].copy_from_slice(&src.data[src_data_range]);
            bump_rect_min(
                &kinds_arena[prev.cmds.range()],
                &starts_arena[prev.cmds.range()],
                &mut data_arena[data],
                neg_origin,
            );
            return;
        }

        // Different len (or first write): mark old ranges as garbage,
        // append new ones.
        if let Some(prev) = self.snapshots.get(&wid) {
            debug_assert!(self.live_cmds >= prev.cmds.len as usize);
            debug_assert!(self.live_data >= prev.data.len as usize);
            self.live_cmds -= prev.cmds.len as usize;
            self.live_data -= prev.data.len as usize;
        }
        let cmds_span = Span::new(self.kinds_arena.len() as u32, src_cmds.len);
        let data_span = Span::new(self.data_arena.len() as u32, src_data.len);

        self.kinds_arena
            .extend_from_slice(&src.kinds[src_cmd_range.clone()]);
        self.starts_arena.reserve(src_cmds.len as usize);
        for &abs in &src.starts[src_cmd_range.clone()] {
            self.starts_arena.push(abs - src_data.start);
        }
        self.data_arena.extend_from_slice(&src.data[src_data_range]);

        let appended_kinds = &self.kinds_arena[cmds_span.range()];
        let appended_starts = &self.starts_arena[cmds_span.range()];
        bump_rect_min(
            appended_kinds,
            appended_starts,
            &mut self.data_arena[data_span.range()],
            neg_origin,
        );

        self.live_cmds += src_cmds.len as usize;
        self.live_data += src_data.len as usize;
        self.snapshots.insert(
            wid,
            EncodeSnapshot {
                subtree_hash,
                available_q,
                cmds: cmds_span,
                data: data_span,
            },
        );

        let cmds_overgrown = self.kinds_arena.len() > self.live_cmds.saturating_mul(COMPACT_RATIO);
        let data_overgrown = self.data_arena.len() > self.live_data.saturating_mul(COMPACT_RATIO);
        if (cmds_overgrown || data_overgrown)
            && (self.live_cmds > COMPACT_FLOOR || self.live_data > COMPACT_FLOOR)
        {
            self.compact();
        }
    }

    pub(crate) fn sweep_removed(&mut self, removed: &[WidgetId]) {
        for wid in removed {
            if let Some(snap) = self.snapshots.remove(wid) {
                self.live_cmds -= snap.cmds.len as usize;
                self.live_data -= snap.data.len as usize;
            }
        }
    }

    /// Drop every snapshot and free all arena storage. Used by
    /// `Ui::__clear_encode_cache` for benches.
    pub(crate) fn clear(&mut self) {
        self.kinds_arena.clear();
        self.starts_arena.clear();
        self.data_arena.clear();
        self.snapshots.clear();
        self.live_cmds = 0;
        self.live_data = 0;
    }

    fn compact(&mut self) {
        let Self {
            kinds_arena,
            starts_arena,
            data_arena,
            snapshots,
            live_cmds,
            live_data,
        } = self;
        let mut new_kinds: Vec<CmdKind> = Vec::with_capacity(*live_cmds);
        let mut new_starts: Vec<u32> = Vec::with_capacity(*live_cmds);
        let mut new_data: Vec<u32> = Vec::with_capacity(*live_data);
        for snap in snapshots.values_mut() {
            let cmds = snap.cmds.range();
            let data = snap.data.range();
            // Starts are subtree-relative — copying without rewrite is
            // sufficient. Compaction moves the *range*, not the
            // intra-range offsets.
            snap.cmds.start = new_kinds.len() as u32;
            snap.data.start = new_data.len() as u32;
            new_kinds.extend_from_slice(&kinds_arena[cmds.clone()]);
            new_starts.extend_from_slice(&starts_arena[cmds]);
            new_data.extend_from_slice(&data_arena[data]);
        }
        *kinds_arena = new_kinds;
        *starts_arena = new_starts;
        *data_arena = new_data;
    }
}

#[cfg(test)]
mod tests;
