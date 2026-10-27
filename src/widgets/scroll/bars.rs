//! The scrollbar overlay: what it reserves from the viewport, the per-axis
//! track/thumb pair, and how bar interaction folds into the scroll offset.

use crate::input::interaction::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::chrome::Chrome;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::theme::scrollbar::ScrollbarTheme;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::ResponseSnapshot;
use crate::widget_core::widget::Widget;
use crate::widgets::scroll::scroll_state::{ScrollState, ThumbTravel};
use crate::widgets::scroll::{Scroll, ScrollGeometry};

/// One scrollbar axis: the two overlay leaves and last frame's interaction on
/// each.
#[derive(Copy, Clone, Debug)]
struct BarAxis {
    track_id: WidgetId,
    thumb_id: WidgetId,
    track: ResponseState,
    thumb: ResponseState,
}

impl BarAxis {
    /// Emit this axis's track leaf (`Sense::CLICK`, one page per click on release)
    /// and thumb leaf (`Sense::DRAG`, painted on top). Neither carries size or
    /// position: the [`Widget::scrollbars`] container's layout assigns both once
    /// measure has the content extent.
    ///
    /// Both are recorded unconditionally (arrange collapses an unused axis to zero),
    /// keeping the child list's shape stable across overflow toggles so the driver
    /// can address children positionally. The track stays a leaf even at zero alpha
    /// so the click-to-page surface remains, as in OS scrollbars.
    fn record(&self, ui: &mut Ui, theme: &ScrollbarTheme) {
        let radius = Corners::all(theme.thickness * 0.5);
        let track = Widget::leaf().id(self.track_id).sense(Sense::CLICK);
        let track_chrome = (!theme.track.is_noop()).then(|| Chrome::rounded(theme.track, radius));
        track.record(ui, track_chrome.as_ref(), |_| {});

        let fill = if self.thumb.left.drag.delta().is_some() || self.thumb.pressed() {
            theme.thumb_active
        } else if self.thumb.hovered() {
            theme.thumb_hovered
        } else {
            theme.thumb
        };
        let thumb = Widget::leaf().id(self.thumb_id).sense(Sense::DRAG);
        let chrome = Chrome::rounded(fill, radius);
        thumb.record(ui, Some(&chrome), |_| {});
    }
}

/// Both scrollbars: ids, last frame's interaction, and the paint theme. Read
/// before the `&mut` state borrow, since reading a response borrows all of `Ui`.
#[derive(Debug)]
pub(super) struct Bars {
    theme: ScrollbarTheme,
    v: BarAxis,
    h: BarAxis,
}

impl Bars {
    /// Last frame's responses of the bars of the scroll `scroll` answers for, with the
    /// scroll's own `disabled` folded in: the cascade reaches the bars with it only a frame
    /// later, so a scroll disabled mid-drag would still pan for one.
    pub(super) fn read(ui: &Ui, scroll: &ResponseSnapshot, theme: &ScrollbarTheme) -> Self {
        let scroll_id = Scroll::viewport_id(scroll.id);
        let disabled = scroll.disabled;
        let axis = |track: &str, thumb: &str| {
            let (track_id, thumb_id) = (scroll_id.with(track), scroll_id.with(thumb));
            let read = |id| {
                let mut response = ui.response_for(id);
                response.merge_disabled(disabled);
                response
            };
            BarAxis {
                track_id,
                thumb_id,
                track: read(track_id),
                thumb: read(thumb_id),
            }
        };
        Self {
            theme: theme.clone(),
            v: axis("vtrack", "vthumb"),
            h: axis("htrack", "hthumb"),
        }
    }

    /// The axes in the order the layout driver addresses their nodes: vertical
    /// track + thumb, then horizontal.
    const fn axes(&self) -> [(Axis, &BarAxis); 2] {
        [(Axis::Y, &self.v), (Axis::X, &self.h)]
    }

    /// Fold this frame's bar interaction into the offset: thumb drags first, then
    /// track pages. Two passes because a page click reads the offset a same-frame
    /// drag on the *other* axis moved, and the drag anchor is one slot shared by
    /// both axes.
    pub(super) fn drive(&self, state: &mut ScrollState, geometry: ScrollGeometry) {
        let axes = geometry.bars.axes;
        for (axis, bar) in self.axes() {
            if !axes.pans(axis) {
                continue;
            }
            let travel = geometry.thumb(axis, state).map(ThumbTravel::of);
            state.apply_thumb_drag(axis, bar.thumb.left.drag, travel);
        }
        for (axis, bar) in self.axes() {
            if !axes.pans(axis) || !bar.track.clicked() {
                continue;
            }
            if let (Some(pointer_local), Some(thumb)) =
                (bar.track.pointer_local, geometry.thumb(axis, state))
            {
                state.apply_track_page(axis, thumb, axis.main_v(pointer_local));
            }
        }
    }

    /// Record the bar overlay as a sibling of the viewport `def` names: a
    /// `scrollbars` container filling the outer rect, holding four leaves in the
    /// order its driver addresses them. Painted after the viewport by record order,
    /// hit-tested above it by cascade order.
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
/// - [`Self::Reserved`] (default): the gutter always takes a strip of the cross
///   axis (`theme.scrollbar.thickness + gap`), with the bar drawn inside only on
///   overflow, so a Hug ancestor doesn't shift when overflow toggles.
/// - [`Self::Overlay`]: no gutter; the bar paints **over** the content's
///   far-edge strip on overflow (macOS-style).
/// - [`Self::Hidden`]: no bar, no gutter; wheel / touchpad / drag still pan, for
///   canvas-style scopes.
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
    /// The gutter the bars take out of the widget's box: a strip of bar thickness
    /// plus gap along the far edge across each panned axis. Only [`Self::Reserved`]
    /// reserves. It doesn't depend on overflow, so a `Hug` ancestor doesn't shift;
    /// the thumb shows only on overflow, decided by the overlay's layout after
    /// measure.
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
