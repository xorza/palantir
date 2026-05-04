//! `RenderCmdBuffer` — SoA command stream.
//!
//! Three columns: a 1-byte kind discriminant per command, a `u32` start
//! offset into a payload arena, and the arena itself. Consumers (the
//! composer, the encode cache, tests) dispatch on `CmdKind` via
//! `iter()` and read each payload with the typed `read::<T>()`
//! helper — no command-enum is ever materialized.
//!
//! Memory: a tagged-enum representation would size to its largest
//! variant (~80 B with padding), so a sequence of
//! `PopClip`/`PopTransform` would pay full-variant storage. Here Pops
//! are 1 + 4 = 5 bytes (kind byte + start offset, no payload). DrawRect
//! splits into stroked / unstroked kinds so the no-stroke variant skips
//! the 5×u32 stroke payload entirely.
//!
//! Soundness: payload structs are `#[repr(C)]` aggregates of `f32`/`u32`
//! (and one `u64` in `TextCacheKey`) tagged `bytemuck::Pod`, so the
//! compiler proves they have no padding bytes. The arena is `Vec<u32>`
//! (4-byte aligned). Pushes go through `bytemuck::cast_slice` (safe);
//! reads go through `bytemuck::pod_read_unaligned` so payloads with
//! align >4 (`DrawTextPayload`) work even when the arena slot starts at
//! a 4-byte-only-aligned offset.

use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::text::TextCacheKey;
use crate::tree::widget_id::WidgetId;
use glam::Vec2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdKind {
    PushClip,
    PopClip,
    PushTransform,
    PopTransform,
    DrawRect,
    DrawRectStroked,
    DrawText,
    /// Brackets a subtree the encoder considered cache-eligible. Carries
    /// the subtree's `WidgetId` plus the kinds-array index of its
    /// matching [`CmdKind::ExitSubtree`] (patched by `push_exit_subtree`)
    /// so a future composer-cache hit can fast-forward past the cmd
    /// range. Composer treats both markers as no-ops today — they exist
    /// only to anchor the upcoming cache.
    EnterSubtree,
    ExitSubtree,
}

/// One command yielded by [`RenderCmdBuffer::iter`]: the kind tag plus
/// the offset (in `u32` words) at which its payload begins in the
/// arena. Read with [`RenderCmdBuffer::read::<T>(start)`] using the
/// payload type matching `kind`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cmd {
    pub(crate) kind: CmdKind,
    pub(crate) start: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawRectPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
    pub(crate) fill: Color,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawRectStrokedPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
    pub(crate) fill: Color,
    pub(crate) stroke: Stroke,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct DrawTextPayload {
    pub(crate) rect: Rect,
    pub(crate) color: Color,
    pub(crate) key: TextCacheKey,
}

/// 12 bytes, align 4. WidgetId is split into two u32s so the struct
/// has no padding (a `u64` field would force struct align 8 + 4 bytes
/// trailing pad, which `bytemuck::Pod` forbids). `exit_idx` is patched
/// by [`RenderCmdBuffer::push_exit_subtree`] once the matching close
/// is recorded.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EnterSubtreePayload {
    pub(crate) wid_lo: u32,
    pub(crate) wid_hi: u32,
    pub(crate) exit_idx: u32,
}

impl EnterSubtreePayload {
    /// Reconstruct the `WidgetId` from the split halves. Unused by the
    /// spike (composer treats EnterSubtree as a no-op); the upcoming
    /// composer cache reads this to key its lookup.
    #[allow(dead_code)]
    #[inline]
    pub(crate) fn wid(self) -> WidgetId {
        WidgetId(((self.wid_hi as u64) << 32) | self.wid_lo as u64)
    }
}

/// Returned by [`RenderCmdBuffer::push_enter_subtree`]; threaded into
/// [`RenderCmdBuffer::push_exit_subtree`] so the close cmd can patch
/// the matching open cmd's `exit_idx` field in-place.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EnterPatch {
    payload_word_offset: u32,
}

/// Append-only command buffer. See module docs.
#[derive(Default)]
pub(crate) struct RenderCmdBuffer {
    pub(crate) kinds: Vec<CmdKind>,
    pub(crate) starts: Vec<u32>,
    pub(crate) data: Vec<u32>,
}

impl RenderCmdBuffer {
    pub(crate) fn clear(&mut self) {
        self.kinds.clear();
        self.starts.clear();
        self.data.clear();
    }

    #[inline]
    pub(crate) fn push_clip(&mut self, r: Rect) {
        self.record_start(CmdKind::PushClip);
        write_pod(&mut self.data, r);
    }

    #[inline]
    pub(crate) fn pop_clip(&mut self) {
        self.record_start(CmdKind::PopClip);
    }

    #[inline]
    pub(crate) fn push_transform(&mut self, t: TranslateScale) {
        self.record_start(CmdKind::PushTransform);
        write_pod(&mut self.data, t);
    }

    #[inline]
    pub(crate) fn pop_transform(&mut self) {
        self.record_start(CmdKind::PopTransform);
    }

    #[inline]
    pub(crate) fn draw_rect(
        &mut self,
        rect: Rect,
        radius: Corners,
        fill: Color,
        stroke: Option<Stroke>,
    ) {
        match stroke {
            None => {
                self.record_start(CmdKind::DrawRect);
                write_pod(&mut self.data, DrawRectPayload { rect, radius, fill });
            }
            Some(stroke) => {
                self.record_start(CmdKind::DrawRectStroked);
                write_pod(
                    &mut self.data,
                    DrawRectStrokedPayload {
                        rect,
                        radius,
                        fill,
                        stroke,
                    },
                );
            }
        }
    }

    #[inline]
    pub(crate) fn draw_text(&mut self, rect: Rect, color: Color, key: TextCacheKey) {
        self.record_start(CmdKind::DrawText);
        write_pod(&mut self.data, DrawTextPayload { rect, color, key });
    }

    /// Open a subtree marker. Returns a patch handle the caller threads
    /// into [`Self::push_exit_subtree`] so the open's `exit_idx` field
    /// is rewritten to point at the matching close once known.
    #[inline]
    pub(crate) fn push_enter_subtree(&mut self, wid: WidgetId) -> EnterPatch {
        self.record_start(CmdKind::EnterSubtree);
        let payload_word_offset = self.data.len() as u32;
        write_pod(
            &mut self.data,
            EnterSubtreePayload {
                wid_lo: wid.0 as u32,
                wid_hi: (wid.0 >> 32) as u32,
                exit_idx: 0,
            },
        );
        EnterPatch {
            payload_word_offset,
        }
    }

    /// Close a subtree marker, patching `patch.exit_idx` of the matching
    /// open to the kinds-array index of this close.
    #[inline]
    pub(crate) fn push_exit_subtree(&mut self, patch: EnterPatch) {
        self.record_start(CmdKind::ExitSubtree);
        let exit_idx = (self.kinds.len() - 1) as u32;
        // exit_idx is the 3rd u32 word in EnterSubtreePayload after
        // wid_lo (word 0) and wid_hi (word 1).
        self.data[patch.payload_word_offset as usize + 2] = exit_idx;
    }

    /// Append a cached subtree's cmd slice into this buffer, shifting
    /// `rect.min` by `offset` on every payload that begins with a `Rect`
    /// (PushClip, DrawRect, DrawRectStroked, DrawText). Pops carry no
    /// payload; PushTransform carries a `TranslateScale` that is
    /// subtree-local — both pass through untouched and compose with the
    /// parent at composer-time.
    ///
    /// `starts` are subtree-relative offsets (0-based into `data`); they
    /// get rebased onto this buffer's data arena during append. Used by
    /// [`crate::renderer::frontend::encoder::cache::EncodeCache`] to
    /// replay a cached subtree under the current frame's root origin.
    pub(crate) fn extend_from_cached(
        &mut self,
        kinds: &[CmdKind],
        starts: &[u32],
        data: &[u32],
        offset: Vec2,
    ) {
        let dest_data_base = self.data.len() as u32;
        self.data.extend_from_slice(data);

        self.kinds.extend_from_slice(kinds);
        self.starts.reserve(starts.len());
        for &s in starts {
            debug_assert!((s as usize) < data.len() || s as usize == data.len());
            self.starts.push(s + dest_data_base);
        }

        let n = kinds.len();
        let appended_starts = &self.starts[self.starts.len() - n..];
        bump_rect_min(kinds, appended_starts, &mut self.data, offset);
    }

    /// Iterator over [`Cmd`]s in record order. Used by the composer hot
    /// path (and tests) to dispatch on `CmdKind` and read payloads with
    /// the typed `read::<T>()` helper — avoids materializing a
    /// per-command enum.
    #[inline]
    pub(crate) fn iter(&self) -> impl Iterator<Item = Cmd> + '_ {
        self.kinds
            .iter()
            .copied()
            .zip(self.starts.iter().copied())
            .map(|(kind, start)| Cmd { kind, start })
    }

    #[inline]
    fn record_start(&mut self, kind: CmdKind) {
        self.starts.push(self.data.len() as u32);
        self.kinds.push(kind);
    }

    /// Read the payload at `start` (in u32 words) as `T`. Caller picks
    /// `T` based on `kinds[i]` — the symmetric `write_pod` at push time
    /// guarantees the bytes are valid for the kind's expected payload.
    /// Used by `get()` and the composer hot path.
    #[inline]
    pub(crate) fn read<T: bytemuck::Pod>(&self, start: u32) -> T {
        let start = start as usize;
        let n_words = std::mem::size_of::<T>() / 4;
        debug_assert!(start + n_words <= self.data.len());
        let words = &self.data[start..start + n_words];
        // `pod_read_unaligned` so payloads with align >4 (e.g.
        // `DrawTextPayload` via `TextCacheKey: u64`) work even though
        // the arena is `Vec<u32>` (4-byte aligned).
        bytemuck::pod_read_unaligned(bytemuck::cast_slice(words))
    }
}

// --- raw POD r/w on the u32 arena ----------------------------------

/// Append a `T` to the arena as `size_of::<T>() / 4` u32 words. `Pod`
/// guarantees no padding bytes — the reinterpretation as `&[u32]` is
/// sound because `align_of::<T>() % 4 == 0` for every payload we use
/// (all field alignments are multiples of 4).
#[inline]
fn write_pod<T: bytemuck::Pod>(data: &mut Vec<u32>, v: T) {
    data.extend_from_slice(bytemuck::cast_slice(std::slice::from_ref(&v)));
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
pub(crate) fn bump_rect_min(kinds: &[CmdKind], starts: &[u32], data: &mut [u32], offset: Vec2) {
    debug_assert_eq!(kinds.len(), starts.len());
    for (kind, &start) in kinds.iter().zip(starts.iter()) {
        match kind {
            CmdKind::PushClip
            | CmdKind::DrawRect
            | CmdKind::DrawRectStroked
            | CmdKind::DrawText => {
                let off = start as usize;
                let x = f32::from_bits(data[off]) + offset.x;
                let y = f32::from_bits(data[off + 1]) + offset.y;
                data[off] = x.to_bits();
                data[off + 1] = y.to_bits();
            }
            CmdKind::PopClip
            | CmdKind::PushTransform
            | CmdKind::PopTransform
            | CmdKind::EnterSubtree
            | CmdKind::ExitSubtree => {}
        }
    }
}

#[cfg(test)]
mod tests;
