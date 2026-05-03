//! `RenderCmdBuffer` — SoA replacement for `Vec<RenderCmd>`.
//!
//! Three columns: a 1-byte kind discriminant per command, a `u32` start
//! offset into a payload arena, and the arena itself. Decodes back to
//! `RenderCmd` on demand for tests; the composer dispatches directly on
//! `CmdKind` without materializing the enum.
//!
//! Memory: `RenderCmd` enum is sized to its largest variant (~80 B with
//! padding), so a sequence of `PopClip`/`PopTransform` paid full-variant
//! storage in the old `Vec<RenderCmd>`. Here Pops are 1 + 4 = 5 bytes
//! (kind byte + start offset, no payload). DrawRect splits into stroked
//! / unstroked kinds so the no-stroke variant skips the 5×u32 stroke
//! payload entirely.
//!
//! Soundness: payload structs are `#[repr(C)]` aggregates of `f32`/`u32`
//! (and one `u64` in `TextCacheKey`) tagged `bytemuck::Pod`, so the
//! compiler proves they have no padding bytes. The arena is `Vec<u32>`
//! (4-byte aligned). Pushes go through `bytemuck::cast_slice` (safe);
//! reads go through `bytemuck::pod_read_unaligned` so payloads with
//! align >4 (`DrawTextPayload`) work even when the arena slot starts at
//! a 4-byte-only-aligned offset.

use crate::primitives::{Color, Corners, Rect, Stroke, TranslateScale};
use crate::text::TextCacheKey;
use glam::Vec2;
use std::ops::Range;

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

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawRectPayload {
    pub rect: Rect,
    pub radius: Corners,
    pub fill: Color,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawRectStrokedPayload {
    pub rect: Rect,
    pub radius: Corners,
    pub fill: Color,
    pub stroke: Stroke,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawTextPayload {
    pub rect: Rect,
    pub color: Color,
    pub key: TextCacheKey,
}

/// Decoded view of one command. The buffer never stores `RenderCmd` —
/// it's reconstructed by `get()` / `iter()` for tests and debugging.
/// Production code reads payloads directly via the typed helpers and
/// dispatches on `CmdKind`.
///
/// `DrawRect` and `DrawRectStroked` are separate variants because the
/// buffer's storage splits them: stroked rects pay 5 extra u32s for the
/// stroke, unstroked rects don't. The split is part of the contract.
#[derive(Clone, Debug)]
pub enum RenderCmd {
    /// Push a logical-px clip rect; the backend intersects it with the
    /// parent at process time. Pairs with `PopClip`.
    PushClip(Rect),
    PopClip,
    /// Push a transform applied to subsequent draws and clips, composed
    /// onto any ancestor transform. Pairs with `PopTransform`.
    PushTransform(TranslateScale),
    PopTransform,
    /// Filled rounded rect, no stroke.
    DrawRect(DrawRectPayload),
    /// Filled rounded rect with stroke.
    DrawRectStroked(DrawRectStrokedPayload),
    /// Place a shaped text run at `payload.rect` (logical px). The
    /// shaped buffer is resolved at submit time via
    /// [`crate::text::TextCacheKey`] against the `TextMeasure` that did
    /// the shaping. Runs whose key is invalid are dropped by the backend.
    DrawText(DrawTextPayload),
}

/// Append-only command buffer. See module docs.
#[derive(Default)]
pub struct RenderCmdBuffer {
    pub(crate) kinds: Vec<CmdKind>,
    pub(crate) starts: Vec<u32>,
    pub(crate) data: Vec<u32>,
}

impl RenderCmdBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.kinds.clear();
        self.starts.clear();
        self.data.clear();
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }

    #[inline]
    pub fn push_clip(&mut self, r: Rect) {
        self.record_start(CmdKind::PushClip);
        write_pod(&mut self.data, r);
    }

    #[inline]
    pub fn pop_clip(&mut self) {
        self.record_start(CmdKind::PopClip);
    }

    #[inline]
    pub fn push_transform(&mut self, t: TranslateScale) {
        self.record_start(CmdKind::PushTransform);
        write_pod(&mut self.data, t);
    }

    #[inline]
    pub fn pop_transform(&mut self) {
        self.record_start(CmdKind::PopTransform);
    }

    #[inline]
    pub fn draw_rect(&mut self, rect: Rect, radius: Corners, fill: Color, stroke: Option<Stroke>) {
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
    pub fn draw_text(&mut self, rect: Rect, color: Color, key: TextCacheKey) {
        self.record_start(CmdKind::DrawText);
        write_pod(&mut self.data, DrawTextPayload { rect, color, key });
    }

    /// Decode the i-th command back into a `RenderCmd`. Used by tests
    /// and `iter()`; production code dispatches on `kinds[i]` directly.
    #[inline]
    pub fn get(&self, i: usize) -> RenderCmd {
        let start = self.starts[i];
        match self.kinds[i] {
            CmdKind::PushClip => RenderCmd::PushClip(self.read(start)),
            CmdKind::PopClip => RenderCmd::PopClip,
            CmdKind::PushTransform => RenderCmd::PushTransform(self.read(start)),
            CmdKind::PopTransform => RenderCmd::PopTransform,
            CmdKind::DrawRect => RenderCmd::DrawRect(self.read(start)),
            CmdKind::DrawRectStroked => RenderCmd::DrawRectStroked(self.read(start)),
            CmdKind::DrawText => RenderCmd::DrawText(self.read(start)),
        }
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter { buf: self, i: 0 }
    }

    /// Append a slice of cmds + their payload bytes from `src`, shifting
    /// `rect.min` by `offset` on every payload that begins with a `Rect`
    /// (PushClip, DrawRect, DrawRectStroked, DrawText). Pops carry no
    /// payload; PushTransform carries a `TranslateScale` that is
    /// subtree-local — both pass through untouched and compose with the
    /// parent at composer-time.
    ///
    /// `cmd_range` indexes into `src.kinds`/`src.starts`. `data_range`
    /// indexes into `src.data` and must cover every payload referenced
    /// by `cmd_range` (in normal use it's captured as
    /// `src.data.len()` before/after the subtree's encode).
    // Wired into the encode cache in a follow-up; until then only the
    // tests in this module exercise it.
    #[allow(dead_code)]
    pub(crate) fn extend_translated(
        &mut self,
        src: &Self,
        cmd_range: Range<u32>,
        data_range: Range<u32>,
        offset: Vec2,
    ) {
        let cmd_lo = cmd_range.start as usize;
        let cmd_hi = cmd_range.end as usize;
        let data_lo = data_range.start as usize;
        let data_hi = data_range.end as usize;

        let dest_data_base = self.data.len() as u32;
        self.data.extend_from_slice(&src.data[data_lo..data_hi]);

        let n_cmds = cmd_hi - cmd_lo;
        self.kinds.reserve(n_cmds);
        self.starts.reserve(n_cmds);

        for i in cmd_lo..cmd_hi {
            let kind = src.kinds[i];
            let src_start = src.starts[i];
            debug_assert!(src_start >= data_range.start && src_start <= data_range.end);
            let new_start = src_start - data_range.start + dest_data_base;
            self.kinds.push(kind);
            self.starts.push(new_start);

            // `rect.min` lives at the first 8 bytes (= 2 u32 words) of
            // every payload that starts with `rect: Rect` — `Rect` is
            // `#[repr(C)] { min: Vec2, size: Size }`. Read/write through
            // `f32::{from,to}_bits` so we don't depend on the arena's
            // u32 alignment lining up with f32 (it does, but staying
            // bits-only matches the rest of the buffer's discipline).
            match kind {
                CmdKind::PushClip
                | CmdKind::DrawRect
                | CmdKind::DrawRectStroked
                | CmdKind::DrawText => {
                    let off = new_start as usize;
                    let x = f32::from_bits(self.data[off]) + offset.x;
                    let y = f32::from_bits(self.data[off + 1]) + offset.y;
                    self.data[off] = x.to_bits();
                    self.data[off + 1] = y.to_bits();
                }
                CmdKind::PopClip | CmdKind::PushTransform | CmdKind::PopTransform => {}
            }
        }
    }

    /// Raw iterator over `(kind, payload-start)` pairs, in order. Used by
    /// the composer hot path to dispatch on `CmdKind` and call typed
    /// `read_*` helpers — avoids materializing `RenderCmd` per command.
    #[inline]
    pub(crate) fn raw_iter(&self) -> impl Iterator<Item = (CmdKind, u32)> + '_ {
        self.kinds.iter().copied().zip(self.starts.iter().copied())
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

pub struct Iter<'a> {
    buf: &'a RenderCmdBuffer,
    i: usize,
}

impl Iterator for Iter<'_> {
    type Item = RenderCmd;
    fn next(&mut self) -> Option<RenderCmd> {
        if self.i >= self.buf.len() {
            return None;
        }
        let cmd = self.buf.get(self.i);
        self.i += 1;
        Some(cmd)
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

#[cfg(test)]
mod tests;
