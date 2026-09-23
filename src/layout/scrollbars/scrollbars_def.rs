//! A bar overlay's definition, and the thumb arithmetic both the layout
//! driver that places the bars and the widget that maps pointer input
//! onto them read.

use crate::layout::axis::Axis;
use crate::layout::types::scroll_axes::ScrollAxes;
use crate::primitives::approx;
use crate::primitives::approx::FloatHash;
use crate::primitives::num::F32Px;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::tree::node_id::NodeId;
use glam::Vec2;
use std::hash::{Hash, Hasher};

/// What a [`Widget::scrollbars`](crate::widget::Widget::scrollbars) overlay
/// places its bars from.
///
/// Everything here is known while recording except the viewport's content
/// extent, which exists only once measure has run. So the overlay names the
/// viewport, and its layout reads the extent then. That is what lets a
/// scroll widget record its bars on its first frame, with no second pass.
///
/// Install it with
/// [`Widget::scrollbar_def`](crate::widget::Widget::scrollbar_def). A
/// widget reads the same numbers through [`Self::thumb`] to map a thumb
/// drag or a track click onto an offset, so the bar the user grabs is the
/// bar that was drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollbarsDef {
    /// The [`Widget::scroll`](crate::widget::Widget::scroll) viewport whose
    /// content the bars show. It must be recorded earlier in the same
    /// frame, on the overlay's layer.
    pub(crate) content: WidgetId,
    /// How far the content is scrolled, in pixels of the zoomed content:
    /// the translation the viewport's transform carries, negated.
    pub(crate) offset: Vec2,
    /// The content's scale. The bars show the content at this size.
    pub(crate) zoom: f32,
    /// The viewport's axes. An axis that does not pan shows no bar.
    pub(crate) axes: ScrollAxes,
    /// The gutter the bars take out of the overlay's box, per side. Zero
    /// for bars that paint over the content.
    ///
    /// Keep it independent of whether the content overflows: a gutter
    /// that opens with the bar makes a `Hug` ancestor jump when it does.
    pub(crate) reserve: Spacing,
    /// The viewport's own padding, inside the gutter.
    pub(crate) padding: Spacing,
    /// A bar's breadth across its axis. The vertical bar runs along the
    /// overlay's right edge, and the horizontal bar along its bottom edge.
    pub(crate) bar_thickness: f32,
    /// The shortest a thumb gets, however long the content.
    pub(crate) min_thumb: f32,
}

impl ScrollbarsDef {
    /// The strip the content occupies in an overlay of size `outer`:
    /// `outer` less the gutter and the padding, floored at zero.
    ///
    /// The layout driver passes this frame's arranged size, to place the
    /// bars. A widget passes the size it arranged at last frame, to solve
    /// its offset in.
    pub(crate) fn viewport(&self, outer: Size) -> Size {
        Size::new(
            (outer.w - self.reserve.horizontal_sum() - self.padding.horizontal_sum()).max(0.0),
            (outer.h - self.reserve.vertical_sum() - self.padding.vertical_sum()).max(0.0),
        )
    }

    /// The bar on `axis` in an overlay of size `outer`, over content that
    /// measured `content` before zoom — the extent
    /// [`Ui::scroll_content`](crate::Ui::scroll_content) reports.
    ///
    /// `None` when the axis does not pan, when the viewport is empty, or
    /// when the content fits and no thumb shows.
    pub(crate) fn thumb(&self, axis: Axis, outer: Size, content: Size) -> Option<BarGeometry> {
        if !self.axes.pans(axis) {
            return None;
        }
        BarGeometry::along(
            axis.main(self.viewport(outer)),
            axis.main(content) * self.zoom,
            axis.main_v(self.offset),
            self.min_thumb,
        )
    }

    /// Visual hash for the authoring rollup. The fit flags stay out: they
    /// size the viewport, and the bars read only which axes pan.
    pub(crate) fn hash_visual<H: Hasher>(&self, h: &mut H) {
        self.content.hash(h);
        self.offset.hash_visual(h);
        self.zoom.hash_visual(h);
        h.write_u16(self.axes.pan_bits());
        self.reserve.hash(h);
        self.padding.hash(h);
        self.bar_thickness.hash_visual(h);
        self.min_thumb.hash_visual(h);
    }
}

/// A [`ScrollbarsDef`] as the tree stores it, with the viewport it names
/// resolved to the node this pass recorded it as.
///
/// Valid for exactly the pass that resolved it: the tree is rebuilt every
/// pass, and every pass records the def again.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResolvedScrollbarsDef {
    pub(crate) def: ScrollbarsDef,
    pub(crate) content: NodeId,
}

/// One bar along its axis, in logical pixels from the track's start:
/// what [`ScrollbarsDef::thumb`] answers.
///
/// The thumb's size and offset are whole pixels already. The driver paints
/// them as they are, and a widget maps pointer input against the same
/// numbers, so the two cannot disagree by a rounding.
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct BarGeometry {
    /// The track's length: the viewport's extent on this axis, and the
    /// distance one page of a track click moves the content.
    pub(crate) track: f32,
    /// The thumb's length.
    pub(crate) thumb_size: f32,
    /// Where the thumb starts along the track.
    pub(crate) thumb_offset: f32,
    /// How far the thumb can slide: the track floored to whole pixels,
    /// less the thumb.
    ///
    /// Carried rather than left to the reader, because the reader has
    /// only the unfloored [`Self::track`] to subtract from. A fractional
    /// viewport put that denominator up to a pixel away from the distance
    /// the thumb moves, so a drag scrubbed the content at slightly the
    /// wrong rate.
    pub(crate) travel: f32,
    /// The largest offset the bar shows, where the content's far edge
    /// meets the track's. The thumb covers `0.0..=max_offset`. A wheel can
    /// go past either end into a
    /// [`Scroll::content_margin`](crate::Scroll::content_margin) band,
    /// which the thumb does not show.
    pub(crate) max_offset: f32,
}

impl BarGeometry {
    /// `track` is both the ratio the thumb expresses and, floored to
    /// whole logical pixels, the length it slides along.
    fn along(track: f32, content: f32, offset: f32, min_thumb: f32) -> Option<Self> {
        if track <= 0.0 || content <= track {
            return None;
        }
        // Quantized here rather than at the paint site: the widget reads
        // these back to map a drag or a track click onto an offset, so the
        // bar the user grabs has to be the bar that was drawn. Rounding on
        // one side only put drag scaling up to a pixel out and made a click
        // in the rounding sliver page away from the thumb.
        //
        // Whole logical pixels because physical snapping rounds a rect's
        // min and max *independently* (`Rect::scaled_by`), which keeps
        // adjacent rects flush but makes a rect's snapped *length* depend on
        // where it sits — so a thumb on fractional coordinates visibly grows
        // and shrinks by a pixel as it travels. Integer logical edges scale
        // to integer physical ones at integer DPR, which pins the length; at
        // fractional DPR it only narrows the wobble, since the real cause is
        // in the snap.
        //
        // **One length, not two.** `travel` comes off the same floored
        // length the thumb clamped into, so a 0..1 fraction of it lands in
        // `0..=travel` and needs no clamp of its own. Flooring the track
        // separately for each cap let a sub-pixel track floor to zero, take
        // the one-pixel minimum thumb, and place it at -1.
        let floored = track.floor().max(1.0);
        let thumb_size = (track / content * track)
            .max(min_thumb)
            .fast_round()
            .clamp(1.0, floored);
        let travel = floored - thumb_size;
        let max_offset = content - track;
        let fraction = approx::share_of(offset, max_offset).clamp(0.0, 1.0);
        Some(Self {
            track,
            thumb_size,
            thumb_offset: (fraction * travel).fast_round(),
            travel,
            max_offset,
        })
    }
}
