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
            CmdKind::PopClip | CmdKind::PushTransform | CmdKind::PopTransform => {}
        }
    }
}

#[cfg(test)]
mod tests;
