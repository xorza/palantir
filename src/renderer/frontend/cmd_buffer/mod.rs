//! `RenderCmdBuffer` — packed command stream.
//!
//! A `u32` descriptor per command packs the kind into its low four bits
//! and the word offset into the payload arena into the upper 28. Consumers
//! walk [`Command`] values through [`RenderCmdBuffer::iter`]; descriptor
//! packing, arena alignment, and payload decoding stay private to this module.
//! All variants are paint ops the composer scales, snaps, and groups
//! into the `RenderBuffer`.
//!
//! Memory: a tagged-enum representation would size to its largest
//! variant (~80 B with padding), so a sequence of
//! `PopClip`/`PopTransform` would pay full-variant storage. Here Pops
//! are one four-byte descriptor with no payload. The 28-bit word offset
//! permits a payload arena just under 1 GiB.
//!
//! Soundness: payload structs are `#[repr(C)]` aggregates of
//! `f32`/`u32` (and one `u64` in `TextCacheKey`) tagged
//! `bytemuck::Pod`, so the compiler proves they have no padding bytes.
//! The arena is `Vec<u32>` (4-byte aligned). Pushes go through
//! `bytemuck::cast_slice` (safe); reads go through
//! `bytemuck::pod_read_unaligned` so payloads with align >4
//! (`DrawTextPayload`) work even when the arena slot starts at a
//! 4-byte-only-aligned offset.
//!
//! ## Noop policy
//!
//! Every `draw_*` early-returns when its inputs would emit no visible
//! pixels (transparent fill color, no-op stroke, no-op shadow tint).
//! **The cmd buffer is the single canonical correctness gate** —
//! callers don't need to pre-check, and the encoder doesn't gate per
//! branch. Upstream filters (`Shape::is_noop` at `Ui::add_shape`,
//! whole-`Background::is_noop` at `Tree::open_node`) are performance
//! optimizations that skip expensive lowering (text shaping, payload
//! staging) or sparse-column writes, not correctness gates.
//!
//! Exception: `draw_polyline` doesn't gate on colour. Its colors live
//! in spans (`PerSegment` can mix one solid stop with N transparent),
//! and an O(n) read on every emit would dominate the per-cmd cost.
//! Colour noops are caught by `Shape::Polyline::is_noop` at the
//! authoring boundary instead; the payload's own `is_noop` still
//! gates degenerate geometry (point count / width).

use crate::primitives::brush::FillAxis;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::{color::ColorF16, corners::Corners, rect::Rect, transform::TranslateScale};
use crate::renderer::gpu_view::GpuPaintRef;
use crate::renderer::texture_id::TextureId;
use crate::scene::shapes::paint::ShapeStroke;
use crate::text::TextCacheKey;

pub(crate) mod payload;

use crate::renderer::frontend::cmd_buffer::payload::{
    BrushSource, CmdKind, DrawArcPayload, DrawCurvePayload, DrawImagePayload, DrawMeshPayload,
    DrawPolylinePayload, DrawRectPayload, DrawShadowPayload, DrawTextPayload, DrawTrianglePayload,
    GpuFillFields, PushClipPayload, PushTransformPayload,
};

const COMMAND_KIND_BITS: u32 = 4;
const COMMAND_KIND_MASK: u32 = (1 << COMMAND_KIND_BITS) - 1;
const MAX_DATA_WORD_OFFSET: usize = (u32::MAX >> COMMAND_KIND_BITS) as usize;
const _: () = assert!(<CmdKind as strum::EnumCount>::COUNT <= 1 << COMMAND_KIND_BITS);

/// Append-only command buffer. See module docs.
#[derive(Debug, Default)]
pub(crate) struct RenderCmdBuffer {
    descriptors: Vec<u32>,
    data: Vec<u32>,
    /// Side channel the Pod `data` stream can't hold: one `GpuPaintRef` per
    /// `GpuView` draw, in emission order. A `DrawImage` with `target = n`
    /// references index `n - 1` here; the composer forwards the callback into
    /// `RenderBuffer.frame_targets`. Cleared per frame with the rest.
    gpu_view_paints: Vec<GpuPaintRef>,
}

#[derive(Clone, Debug)]
pub(crate) enum Command<'a> {
    PushClip(PushClipPayload),
    PopClip,
    PushTransform(TranslateScale),
    PopTransform,
    DrawRect(DrawRectPayload),
    DrawShadow(DrawShadowPayload),
    DrawText(DrawTextPayload),
    DrawMesh(DrawMeshPayload),
    DrawPolyline(DrawPolylinePayload),
    DrawImage {
        payload: DrawImagePayload,
        paint: Option<&'a GpuPaintRef>,
    },
    DrawCurve(DrawCurvePayload),
    DrawArc(DrawArcPayload),
    DrawTriangle(DrawTrianglePayload),
}

#[derive(Debug)]
pub(crate) struct Commands<'a> {
    buffer: &'a RenderCmdBuffer,
    index: usize,
}

impl<'a> Iterator for Commands<'a> {
    type Item = Command<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let command = self.buffer.command(self.index)?;
        self.index += 1;
        Some(command)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.buffer.descriptors.len() - self.index;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Commands<'_> {}

impl RenderCmdBuffer {
    pub(crate) fn iter(&self) -> Commands<'_> {
        Commands {
            buffer: self,
            index: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.descriptors.clear();
        self.data.clear();
        self.gpu_view_paints.clear();
    }

    #[inline]
    pub(crate) fn push_clip(&mut self, rect: Rect) {
        self.record_start(CmdKind::PushClip);
        write_pod(
            &mut self.data,
            PushClipPayload {
                rect,
                corners: Corners::ZERO,
            },
        );
    }

    #[inline]
    pub(crate) fn push_clip_rounded(&mut self, rect: Rect, corners: Corners) {
        self.record_start(CmdKind::PushClip);
        write_pod(&mut self.data, PushClipPayload { rect, corners });
    }

    #[inline]
    pub(crate) fn pop_clip(&mut self) {
        self.record_start(CmdKind::PopClip);
    }

    #[inline]
    pub(crate) fn push_transform(&mut self, t: TranslateScale) {
        self.record_start(CmdKind::PushTransform);
        write_pod(&mut self.data, PushTransformPayload::from(t));
    }

    #[inline]
    pub(crate) fn pop_transform(&mut self) {
        self.record_start(CmdKind::PopTransform);
    }

    #[inline]
    pub(crate) fn draw_rect(
        &mut self,
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) {
        self.draw_rect_impl(rect, corners, fill, stroke, false);
    }

    /// Windowed-rect sibling of [`Self::draw_rect`]: same payload, but
    /// the `FillKind` carries the window bit so the shader inverts the
    /// fill coverage (fill outside the rounded boundary, transparent
    /// window inside the stroke). The bit also keeps the composer's
    /// opaque-cover checks (`fill_kind == FillKind::SOLID`) from
    /// treating the quad as an occluder — its interior is a hole.
    #[inline]
    pub(crate) fn draw_rect_window(
        &mut self,
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
    ) {
        self.draw_rect_impl(rect, corners, fill, stroke, true);
    }

    #[inline]
    fn draw_rect_impl(
        &mut self,
        rect: Rect,
        corners: Corners,
        fill: BrushSource,
        stroke: ShapeStroke,
        window: bool,
    ) {
        if rect.is_paint_empty() || (fill.is_noop() && stroke.is_noop()) {
            return;
        }

        // Stroke stays solid-only — gradient strokes are a non-goal.
        let GpuFillFields {
            color: fill_color,
            kind: fill_kind,
            lut_row: fill_lut_row,
            axis: fill_axis,
        } = fill.to_gpu_fields();
        let fill_kind = if window {
            fill_kind.with_window()
        } else {
            fill_kind
        };

        let (stroke_color, stroke_width) = if stroke.is_noop() {
            (ColorF16::TRANSPARENT, 0.0)
        } else {
            (stroke.color, stroke.width())
        };
        let payload = DrawRectPayload {
            rect,
            corners,
            fill: fill_color,
            stroke_color,
            stroke_width,
            fill_kind,
            fill_lut_row,
            fill_axis,
        };
        self.record_start(CmdKind::DrawRect);
        write_pod(&mut self.data, payload);
    }

    /// Record a shadow paint cmd. For a drop shadow, `rect` is the
    /// offset source inflated by `3σ + max(spread, 0)`; for an inset
    /// shadow it is the source rect. `radius` is the source shape's corner radii.
    /// `color` is the shadow tint. `fill_kind` is
    /// `FillKind::SHADOW_DROP|SHADOW_INSET`. Drop shadows carry
    /// `(0, 0, σ, spread)` in `fill_axis`; inset shadows
    /// carry `(offset.x, offset.y, σ, spread)`. The composer scales the
    /// logical-px lanes to physical px on emit.
    #[inline]
    pub(crate) fn draw_shadow(
        &mut self,
        rect: Rect,
        corners: Corners,
        color: ColorF16,
        fill_kind: FillKind,
        fill_axis: FillAxis,
    ) {
        let payload = DrawShadowPayload {
            rect,
            corners,
            color,
            fill_kind,
            fill_axis,
        };
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawShadow);
        write_pod(&mut self.data, payload);
    }

    #[inline]
    pub(crate) fn draw_text(&mut self, rect: Rect, color: ColorF16, key: TextCacheKey) {
        let payload = DrawTextPayload { rect, color, key };
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawText);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawMesh` cmd against already-staged vertices + indices
    /// in `shape_payloads.meshes`. Caller pushes verts (translated into
    /// the owner's logical-px world coords) and indices directly so the
    /// encoder can apply the owner-rect offset inline without an
    /// intermediate scratch buffer.
    pub(crate) fn draw_mesh(&mut self, payload: DrawMeshPayload) {
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawMesh);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawImage` cmd. Composer transforms `rect` into
    /// physical-px and routes to the backend's image pipeline.
    pub(crate) fn draw_image(&mut self, payload: DrawImagePayload) {
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawImage);
        write_pod(&mut self.data, payload);
    }

    pub(crate) fn draw_gpu_view(&mut self, rect: Rect, handle: TextureId, paint: GpuPaintRef) {
        let paint_index = u32::try_from(self.gpu_view_paints.len())
            .expect("GpuView paint side channel exceeded u32::MAX entries");
        let payload = DrawImagePayload::gpu_view(rect, handle, paint_index);
        if payload.is_noop() {
            return;
        }
        self.gpu_view_paints.push(paint);
        self.record_start(CmdKind::DrawImage);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawCurve` cmd. Composer transforms the control
    /// points to physical-px and pushes per-sub-instance entries onto
    /// `RenderBuffer.curves` — one GPU draw per scissor group covers
    /// every instance the group emitted.
    pub(crate) fn draw_curve(&mut self, payload: DrawCurvePayload) {
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawCurve);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawArc` cmd. Composer transforms center/radius to
    /// physical-px and pushes `kind = ARC` entries onto
    /// `RenderBuffer.curves` — arcs batch with the beziers, one GPU
    /// draw per scissor group.
    pub(crate) fn draw_arc(&mut self, payload: DrawArcPayload) {
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawArc);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawTriangle` cmd. Composer transforms the three corner
    /// `points` (owner-local, offset by `origin`) to physical-px and emits
    /// one `Quad` with `FillKind::TRIANGLE`. Noop strokes normalize to
    /// `(TRANSPARENT, 0.0)` here, exactly like [`Self::draw_rect`].
    pub(crate) fn draw_triangle(
        &mut self,
        origin: glam::Vec2,
        points: [glam::Vec2; 3],
        fill: ColorF16,
        radius: f32,
        stroke: ShapeStroke,
    ) {
        let (stroke_color, stroke_width) = if stroke.is_noop() {
            (ColorF16::TRANSPARENT, 0.0)
        } else {
            (stroke.color, stroke.width())
        };
        let [a, b, c] = points;
        let payload = DrawTrianglePayload {
            origin,
            a,
            b,
            c,
            fill,
            stroke_color,
            radius,
            stroke_width,
            ..bytemuck::Zeroable::zeroed()
        };
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawTriangle);
        write_pod(&mut self.data, payload);
    }

    /// Record a `DrawPolyline` cmd against already-staged points and
    /// colors. Caller pushes onto `polyline_points` / `polyline_colors`
    /// directly (so the encoder can apply the owner-rect offset
    /// inline without an intermediate scratch buffer) and passes the
    /// resulting spans here. The `color_mode`-dictated `colors_len`
    /// is a caller invariant enforced upstream by
    /// `PolylineColors::assert_matches` in `Shapes::add`.
    pub(crate) fn draw_polyline(&mut self, payload: DrawPolylinePayload) {
        if payload.is_noop() {
            return;
        }
        self.record_start(CmdKind::DrawPolyline);
        write_pod(&mut self.data, payload);
    }

    #[inline]
    fn record_start(&mut self, kind: CmdKind) {
        self.descriptors
            .push(pack_command_descriptor(kind, self.data.len()));
    }

    /// Read the payload at `start` (in u32 words) as `T`. Caller picks
    /// `T` based on the descriptor's kind — the symmetric `write_pod` at
    /// push time guarantees the bytes are valid for the expected payload.
    #[inline]
    fn read<T: bytemuck::Pod>(&self, start: u32) -> T {
        // Arena is `Vec<u32>` (4-byte aligned). `pod_read_unaligned`
        // below tolerates align >4, but payload size must be a whole
        // number of u32 words for the slice math to round-trip — a
        // `T` with `size_of % 4 != 0` would compile and silently read
        // garbage from the trailing partial word.
        const { assert!(size_of::<T>().is_multiple_of(4)) };
        let start = start as usize;
        let n_words = size_of::<T>() / 4;
        debug_assert!(start + n_words <= self.data.len());
        let words = &self.data[start..start + n_words];
        // `pod_read_unaligned` so payloads with align >4 (e.g.
        // `DrawTextPayload` via `TextCacheKey: u64`) work even though
        // the arena is `Vec<u32>` (4-byte aligned).
        bytemuck::pod_read_unaligned(bytemuck::cast_slice(words))
    }

    fn command(&self, index: usize) -> Option<Command<'_>> {
        let &descriptor = self.descriptors.get(index)?;
        let kind = CmdKind::from_repr((descriptor & COMMAND_KIND_MASK) as u8)
            .expect("command descriptor contains an unknown kind tag");
        let start = descriptor >> COMMAND_KIND_BITS;
        Some(match kind {
            CmdKind::PushClip => Command::PushClip(self.read(start)),
            CmdKind::PopClip => Command::PopClip,
            CmdKind::PushTransform => {
                let payload: PushTransformPayload = self.read(start);
                Command::PushTransform(TranslateScale::from(payload))
            }
            CmdKind::PopTransform => Command::PopTransform,
            CmdKind::DrawRect => Command::DrawRect(self.read(start)),
            CmdKind::DrawShadow => Command::DrawShadow(self.read(start)),
            CmdKind::DrawText => Command::DrawText(self.read(start)),
            CmdKind::DrawMesh => Command::DrawMesh(self.read(start)),
            CmdKind::DrawPolyline => Command::DrawPolyline(self.read(start)),
            CmdKind::DrawImage => {
                let payload: DrawImagePayload = self.read(start);
                let paint = payload.gpu_view_paint().map(|paint_index| {
                    self.gpu_view_paints
                        .get(paint_index as usize)
                        .expect("DrawImage references a missing GpuView paint")
                });
                Command::DrawImage { payload, paint }
            }
            CmdKind::DrawCurve => Command::DrawCurve(self.read(start)),
            CmdKind::DrawArc => Command::DrawArc(self.read(start)),
            CmdKind::DrawTriangle => Command::DrawTriangle(self.read(start)),
        })
    }
}

#[inline]
fn pack_command_descriptor(kind: CmdKind, start: usize) -> u32 {
    assert!(
        start <= MAX_DATA_WORD_OFFSET,
        "command payload arena exceeds the 28-bit word-offset limit"
    );
    ((start as u32) << COMMAND_KIND_BITS) | kind as u32
}

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

#[cfg(test)]
pub(crate) mod test_support {
    use super::RenderCmdBuffer;

    pub(crate) fn assert_same_stream(left: &RenderCmdBuffer, right: &RenderCmdBuffer) {
        assert_eq!(
            left.descriptors, right.descriptors,
            "cmd descriptors must match"
        );
        assert_eq!(left.data, right.data, "cmd payload bytes must match");
        assert_eq!(
            left.gpu_view_paints.len(),
            right.gpu_view_paints.len(),
            "GpuView paint side-channel lengths must match"
        );
    }
}
