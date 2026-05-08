//! `RenderCmdBuffer` — SoA command stream.
//!
//! Three columns: a 1-byte kind discriminant per command, a `u32`
//! start offset into a payload arena, and the arena itself. Consumers
//! walk `kinds` / `starts` by index and read each payload with the
//! typed `read::<T>()` helper — no command-enum is ever materialized.
//! All variants are paint ops the composer scales, snaps, and groups
//! into the `RenderBuffer`.
//!
//! Memory: a tagged-enum representation would size to its largest
//! variant (~80 B with padding), so a sequence of
//! `PopClip`/`PopTransform` would pay full-variant storage. Here Pops
//! are 1 + 4 = 5 bytes (kind byte + start offset, no payload).
//! `DrawRect` splits into stroked / unstroked kinds so the no-stroke
//! variant skips the 5×u32 stroke payload entirely.
//!
//! Soundness: payload structs are `#[repr(C)]` aggregates of
//! `f32`/`u32` (and one `u64` in `TextCacheKey`) tagged
//! `bytemuck::Pod`, so the compiler proves they have no padding bytes.
//! The arena is `Vec<u32>` (4-byte aligned). Pushes go through
//! `bytemuck::cast_slice` (safe); reads go through
//! `bytemuck::pod_read_unaligned` so payloads with align >4
//! (`DrawTextPayload`) work even when the arena slot starts at a
//! 4-byte-only-aligned offset.

use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::text::TextCacheKey;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdKind {
    PushClip,
    /// Scissor clip + rounded-corner stencil mask. Carries
    /// `PushClipRoundedPayload` (rect + radius). Composer treats it as
    /// a regular scissor for the purposes of group splitting; the
    /// backend's stencil path reads the radius to write the SDF mask.
    PushClipRounded,
    PopClip,
    PushTransform,
    PopTransform,
    DrawRect,
    DrawRectStroked,
    DrawText,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PushClipRoundedPayload {
    pub(crate) rect: Rect,
    pub(crate) radius: Corners,
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
    pub(crate) fn push_clip_rounded(&mut self, rect: Rect, radius: Corners) {
        self.record_start(CmdKind::PushClipRounded);
        write_pod(&mut self.data, PushClipRoundedPayload { rect, radius });
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

    #[inline]
    fn record_start(&mut self, kind: CmdKind) {
        self.starts.push(self.data.len() as u32);
        self.kinds.push(kind);
    }

    /// Read the payload at `start` (in u32 words) as `T`. Caller picks
    /// `T` based on `kinds[i]` — the symmetric `write_pod` at push time
    /// guarantees the bytes are valid for the kind's expected payload.
    #[inline]
    pub(crate) fn read<T: bytemuck::Pod>(&self, start: u32) -> T {
        let start = start as usize;
        let n_words = std::mem::size_of::<T>() / 4;
        assert!(start + n_words <= self.data.len());
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
