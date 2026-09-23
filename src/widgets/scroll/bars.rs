//! The scrollbar overlay: what it reserves out of the viewport, the
//! per-axis track/thumb pair, and how a frame's bar interaction folds
//! back into the scroll offset.

use crate::input::response::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::scrollbars::scrollbars_def::ScrollbarsDef;
use crate::layout::types::scroll_axes::ScrollAxes;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::scroll::ScrollGeometry;
use crate::widgets::scroll::state::{ScrollState, ThumbTravel, TrackPage};
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use crate::widgets::widget::Widget;

/// One scrollbar axis: the two leaves the overlay records for it, and
/// last frame's interaction on each.
#[derive(Copy, Clone, Debug)]
struct BarAxis {
    track_id: WidgetId,
    thumb_id: WidgetId,
    track: ResponseState,
    thumb: ResponseState,
}

impl BarAxis {
    /// Emit this axis's two nodes onto the overlay: a track leaf with
    /// `Sense::CLICK` (paging on press) and a thumb leaf with
    /// `Sense::DRAG` painted on top. Neither carries a size or a
    /// position — the overlay is a [`Widget::scrollbars`] container,
    /// and its layout assigns both rects once measure has
    /// produced the content extent they are a ratio of.
    ///
    /// Both are recorded unconditionally, even on an axis showing no
    /// bar: arrange collapses those to zero extent. Recording them
    /// either way is what keeps the child list the same shape across an
    /// overflow toggle, which is what lets the driver address children
    /// positionally.
    ///
    /// Track stays a leaf even when `theme.track` alpha is 0 so the
    /// click-to-page surface remains — the gutter is reserved either
    /// way, matching OS scrollbar conventions.
    fn record(&self, ui: &mut Ui, theme: &ScrollbarTheme) {
        let radius = Corners::all(theme.thickness * 0.5);
        let track = Widget::leaf().id(self.track_id).sense(Sense::CLICK);
        let track_chrome =
            (!theme.track.is_noop()).then(|| Background::rounded(theme.track, radius));
        track.record(ui, track_chrome.as_ref(), |_| {});

        let fill = if self.thumb.left.drag.delta().is_some() || self.thumb.pressed() {
            theme.thumb_active
        } else if self.thumb.hovered() {
            theme.thumb_hovered
        } else {
            theme.thumb
        };
        let thumb = Widget::leaf().id(self.thumb_id).sense(Sense::DRAG);
        let chrome = Background::rounded(fill, radius);
        thumb.record(ui, Some(&chrome), |_| {});
    }
}

/// Both scrollbars: their ids, last frame's interaction on each, and the
/// theme they paint with. Read in full *before* the `&mut` state borrow
/// that acts on them, because reading a response borrows all of `Ui`.
#[derive(Debug)]
pub(super) struct Bars {
    theme: ScrollbarTheme,
    v: BarAxis,
    h: BarAxis,
}

impl Bars {
    pub(super) fn read(ui: &Ui, scroll_id: WidgetId, theme: &ScrollbarTheme) -> Self {
        let axis = |track: &str, thumb: &str| {
            let (track_id, thumb_id) = (scroll_id.with(track), scroll_id.with(thumb));
            BarAxis {
                track_id,
                thumb_id,
                track: ui.response_for(track_id),
                thumb: ui.response_for(thumb_id),
            }
        };
        Self {
            theme: theme.clone(),
            v: axis("vtrack", "vthumb"),
            h: axis("htrack", "hthumb"),
        }
    }

    /// The axes in the order the layout driver addresses their nodes:
    /// vertical track + thumb, then horizontal.
    fn axes(&self) -> [(Axis, &BarAxis); 2] {
        [(Axis::Y, &self.v), (Axis::X, &self.h)]
    }

    /// Fold this frame's bar interaction into the offset: thumb drags
    /// first, then track pages.
    ///
    /// Two passes, not one per axis: a page click reads the offset a
    /// same-frame drag on the *other* axis already moved, and the drag
    /// anchor is a single slot shared by both axes. Resolving each bar
    /// immediately before it is applied is what keeps the thumb tracking
    /// the cursor within the frame.
    pub(super) fn drive(&self, state: &mut ScrollState, geom: ScrollGeometry) {
        let axes = geom.bars.axes;
        for (axis, bar) in self.axes() {
            if !axes.pans(axis) {
                continue;
            }
            let travel = geom.thumb(axis, state).map(ThumbTravel::of);
            state.apply_thumb_drag(
                axis,
                bar.thumb.left.drag.started(),
                bar.thumb.left.drag.delta(),
                travel,
            );
        }
        for (axis, bar) in self.axes() {
            if !axes.pans(axis) || !bar.track.clicked() {
                continue;
            }
            let Some(pointer_local) = bar.track.pointer_local else {
                continue;
            };
            let page = geom
                .thumb(axis, state)
                .map(|thumb| TrackPage::at(thumb, axis.main_v(pointer_local)));
            state.apply_track_page(axis, page);
        }
    }

    /// Record the bar overlay as a sibling of the viewport `def` names: a
    /// `scrollbars` container filling the outer rect, holding the four
    /// leaves in the fixed order its driver addresses them by. Painted
    /// after the viewport via record order, hit-tested above it via
    /// cascade order.
    pub(super) fn record(&self, ui: &mut Ui, def: ScrollbarsDef) {
        let mut overlay = Widget::scrollbars()
            .id(def.content.with("bars"))
            .size((Sizing::FILL, Sizing::FILL));
        overlay.scrollbar_def(ui, def);
        overlay.record(ui, None, |ui| {
            for (_, bar) in self.axes() {
                bar.record(ui, &self.theme);
            }
        });
    }
}

/// How the scrollbars relate to the content area on the pan axes.
///
/// - [`Self::Reserved`] (default): the gutter always takes a strip of
///   the cross axis (`theme.scrollbar.thickness + gap`), and the bar is
///   drawn inside that gutter only when content overflows. The
///   reserved width is constant whether or not anything currently
///   overflows — so a Hug ancestor of the scroll doesn't shift when
///   overflow toggles.
/// - [`Self::Overlay`]: no gutter is reserved. The content gets the
///   full inner width, and the bar paints **over** the content's
///   far-edge strip when overflow happens. Modern macOS-style scroll
///   indicator behaviour.
/// - [`Self::Hidden`]: no bar, no gutter. Wheel / touchpad / drag
///   input still pans. Useful for canvas-style scopes (node graphs,
///   infinite boards) where indicators would be noise.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum BarMode {
    /// A gutter always reserved, with the bar drawn in it on overflow.
    /// The default.
    #[default]
    Reserved,
    /// No gutter. The bar paints over the content on overflow.
    Overlay,
    /// No bar and no gutter. Input still pans.
    Hidden,
}

impl BarMode {
    /// The gutter the bars take out of the widget's box: a strip of the
    /// bar's thickness plus its gap along the far edge across each panned
    /// axis, so the bar does not touch the visible content.
    ///
    /// Only [`Self::Reserved`] reserves. [`Self::Overlay`] paints the bar
    /// over the content, and [`Self::Hidden`] has no bar at all. The strip
    /// does not depend on overflow, so a `Hug` ancestor does not shift
    /// when the content starts or stops fitting; the thumb itself still
    /// shows only when the content overflows, which the overlay's layout
    /// decides after measure.
    pub(super) fn gutter(self, axes: ScrollAxes, theme: &ScrollbarTheme) -> Spacing {
        if self != Self::Reserved {
            return Spacing::ZERO;
        }
        let strip = |axis| {
            if axes.pans(axis) {
                theme.thickness + theme.gap
            } else {
                0.0
            }
        };
        Spacing::new(0.0, 0.0, strip(Axis::Y), strip(Axis::X))
    }
}
