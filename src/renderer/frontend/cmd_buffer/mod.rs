//! `RenderCmdBuffer` — SoA command stream.
//!
//! Three columns: a 1-byte kind discriminant per command, a `u32` start
//! offset into a payload arena, and the arena itself. Consumers walk
//! `kinds` / `starts` by index and read each payload with the typed
//! `read::<T>()` helper — no command-enum is ever materialized. Index
//! iteration lets the composer fast-forward past `EnterSubtree` ranges
//! on a compose-cache hit.
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

use crate::layout::cache::AvailableKey;
use crate::primitives::{
    color::Color, corners::Corners, rect::Rect, stroke::Stroke, transform::TranslateScale,
};
use crate::text::TextCacheKey;
use crate::tree::node_hash::NodeHash;
use crate::tree::widget_id::WidgetId;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CmdKind {
    PushClip,
    /// Scissor clip + rounded-corner stencil mask. Carries
    /// `PushClipRoundedPayload` (rect + radius). Composer treats it as a
    /// regular scissor for the purposes of group splitting; the
    /// backend's stencil path reads the radius to write the SDF mask.
    PushClipRounded,
    PopClip,
    PushTransform,
    PopTransform,
    DrawRect,
    DrawRectStroked,
    DrawText,
    /// Brackets a subtree the encoder considered cache-eligible. Carries
    /// the subtree's `WidgetId` plus the kinds-array index of its
    /// matching [`CmdKind::ExitSubtree`] (patched by `push_exit_subtree`)
    /// so a composer-cache hit can fast-forward past the cmd range.
    /// `EnterSubtree` drives `ComposeCache::try_splice`; `ExitSubtree`
    /// drives `ComposeCache::write_subtree` on the miss path.
    EnterSubtree,
    ExitSubtree,
}

impl CmdKind {
    /// `true` for kinds whose payload begins with `rect: Rect` — i.e.
    /// the first 8 bytes (= 2 u32 words) of the payload are
    /// `rect.min.{x, y}`. Used by the encode cache to translate
    /// subtree-relative coordinates without unpacking each payload
    /// variant. Pinned by const asserts on each payload struct's
    /// `offset_of!(rect)` below.
    #[inline]
    pub(crate) fn has_leading_rect(self) -> bool {
        matches!(
            self,
            CmdKind::PushClip
                | CmdKind::PushClipRounded
                | CmdKind::DrawRect
                | CmdKind::DrawRectStroked
                | CmdKind::DrawText,
        )
    }
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

/// `exit_idx` is patched by [`RenderCmdBuffer::push_exit_subtree`] once
/// the matching close is recorded. Carries the subtree's `(WidgetId,
/// subtree_hash, available_q)` triple — the composer cache reads them
/// to key its lookup directly off the cmd stream (self-describing
/// markers). Trailing padding is injected by `padding_struct` so
/// `bytemuck::Pod`'s no-padding-bytes invariant holds.
#[repr(C)]
#[padding_struct::padding_struct]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct EnterSubtreePayload {
    pub(crate) wid: WidgetId,
    pub(crate) subtree_hash: NodeHash,
    pub(crate) avail: AvailableKey,
    pub(crate) exit_idx: u32,
}

/// Pin the `rect: Rect` leading-field invariant for every kind whose
/// [`CmdKind::has_leading_rect`] returns `true`. The encode cache's
/// `bump_rect_min` reads `data[start..start+2]` as `rect.min.{x, y}`
/// without unpacking the variant, relying on this layout.
const _: () = {
    assert!(std::mem::offset_of!(PushClipRoundedPayload, rect) == 0);
    assert!(std::mem::offset_of!(DrawRectPayload, rect) == 0);
    assert!(std::mem::offset_of!(DrawRectStrokedPayload, rect) == 0);
    assert!(std::mem::offset_of!(DrawTextPayload, rect) == 0);
};

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

    /// Open a subtree marker. Returns a patch handle the caller threads
    /// into [`Self::push_exit_subtree`] so the open's `exit_idx` field
    /// is rewritten to point at the matching close once known.
    #[inline]
    pub(crate) fn push_enter_subtree(
        &mut self,
        wid: WidgetId,
        subtree_hash: NodeHash,
        avail: AvailableKey,
    ) -> EnterPatch {
        self.record_start(CmdKind::EnterSubtree);
        let payload_word_offset = self.data.len() as u32;
        write_pod(
            &mut self.data,
            EnterSubtreePayload {
                wid,
                subtree_hash,
                avail,
                exit_idx: 0,
                ..bytemuck::Zeroable::zeroed()
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
        const EXIT_IDX_WORD: usize =
            std::mem::offset_of!(EnterSubtreePayload, exit_idx) / size_of::<u32>();
        self.data[patch.payload_word_offset as usize + EXIT_IDX_WORD] = exit_idx;
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
