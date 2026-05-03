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
//! only, so they have no padding bytes and trivial Copy. The arena is
//! `Vec<u32>` (4-byte aligned). Each push appends `size_of::<T>() / 4`
//! words at the current `data.len()`; reads cast `data.as_ptr().add(start)`
//! back to `*const T`. Encode/decode are symmetric per kind, both bounded
//! to this module.

use crate::primitives::{Color, Corners, Rect, Stroke, TranslateScale};
use crate::text::TextCacheKey;

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
#[derive(Clone, Copy, Debug)]
pub struct DrawRectPayload {
    pub rect: Rect,
    pub radius: Corners,
    pub fill: Color,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DrawRectStrokedPayload {
    pub rect: Rect,
    pub radius: Corners,
    pub fill: Color,
    pub stroke: Stroke,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
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
    pub fn get(&self, i: usize) -> RenderCmd {
        let start = self.starts[i];
        match self.kinds[i] {
            CmdKind::PushClip => RenderCmd::PushClip(self.read_clip(start)),
            CmdKind::PopClip => RenderCmd::PopClip,
            CmdKind::PushTransform => RenderCmd::PushTransform(self.read_transform(start)),
            CmdKind::PopTransform => RenderCmd::PopTransform,
            CmdKind::DrawRect => RenderCmd::DrawRect(self.read_draw_rect(start)),
            CmdKind::DrawRectStroked => {
                RenderCmd::DrawRectStroked(self.read_draw_rect_stroked(start))
            }
            CmdKind::DrawText => RenderCmd::DrawText(self.read_draw_text(start)),
        }
    }

    pub fn iter(&self) -> Iter<'_> {
        Iter { buf: self, i: 0 }
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

    // --- typed reads, used by composer hot path and `get()` ----------

    #[inline]
    pub(crate) fn read_clip(&self, start: u32) -> Rect {
        unsafe { read_pod(&self.data, start) }
    }

    #[inline]
    pub(crate) fn read_transform(&self, start: u32) -> TranslateScale {
        unsafe { read_pod(&self.data, start) }
    }

    #[inline]
    pub(crate) fn read_draw_rect(&self, start: u32) -> DrawRectPayload {
        unsafe { read_pod(&self.data, start) }
    }

    #[inline]
    pub(crate) fn read_draw_rect_stroked(&self, start: u32) -> DrawRectStrokedPayload {
        unsafe { read_pod(&self.data, start) }
    }

    #[inline]
    pub(crate) fn read_draw_text(&self, start: u32) -> DrawTextPayload {
        unsafe { read_pod(&self.data, start) }
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

#[inline]
fn write_pod<T: Copy>(data: &mut Vec<u32>, v: T) {
    const {
        assert!(
            std::mem::size_of::<T>().is_multiple_of(std::mem::align_of::<u32>()),
            "payload must be a whole number of u32 words",
        );
    }
    let n_words = std::mem::size_of::<T>() / std::mem::size_of::<u32>();
    let ptr = (&v as *const T).cast::<u32>();
    let slice = unsafe { std::slice::from_raw_parts(ptr, n_words) };
    data.extend_from_slice(slice);
}

/// Read a POD payload at `start` (in u32 words). Caller must ensure
/// the encoder wrote a value of type `T` at this offset.
#[inline]
unsafe fn read_pod<T: Copy>(data: &[u32], start: u32) -> T {
    let start = start as usize;
    debug_assert!(start + std::mem::size_of::<T>() / 4 <= data.len());
    let ptr = unsafe { data.as_ptr().add(start).cast::<T>() };
    // The arena is u32-aligned and our payloads are 4-byte-aligned
    // f32/u32 aggregates, so a plain aligned read is sound.
    unsafe { std::ptr::read(ptr) }
}
