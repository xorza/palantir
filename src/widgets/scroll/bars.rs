//! The scrollbar overlay: what it reserves out of the viewport, the
//! per-axis track/thumb pair, and how a frame's bar interaction folds
//! back into the scroll offset.

use crate::input::response::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::scrollbars::{self, BarDomain, ScrollbarsDef};
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::ui::Ui;
use crate::widgets::scroll::ScrollGeometry;
use crate::widgets::scroll::state::{ScrollState, ThumbTravel, TrackPage};
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use glam::BVec2;

/// the bar's `width` plus a `gap` strip so the bar doesn't touch the
/// visible content. Returns 0 when the axis isn't panned.
#[inline]
fn bar_reservation(panned: bool, theme: &ScrollbarTheme) -> f32 {
    if panned { theme.width + theme.gap } else { 0.0 }
}

/// Cross-axis space the bars take out of the widget's box: the gutter
/// reserved on each panned axis, and the viewport left over for content.
#[derive(Copy, Clone, Debug)]
pub(super) struct BarSpace {
    pub(super) bar_viewport: Size,
    pub(super) reserve_y: f32,
    pub(super) reserve_x: f32,
}

pub(super) fn bar_space(
    outer: Size,
    pan: BVec2,
    user_padding: Spacing,
    theme: &ScrollbarTheme,
    bar_mode: BarMode,
) -> BarSpace {
    // Only `Reserved` reserves the gutter on the pan axes. `Overlay`
    // paints the bar over content without reservation; `Hidden` has
    // no bar at all. Reservation is constant for `Reserved` (not
    // toggled by overflow) so a Hug ancestor doesn't shift between
    // frames; the bar thumb itself still appears conditionally on
    // `content > viewport`, decided by `layout::scrollbars` after
    // measure rather than here.
    let reserve = matches!(bar_mode, BarMode::Reserved);
    let reserve_y = bar_reservation(pan.y && reserve, theme);
    let reserve_x = bar_reservation(pan.x && reserve, theme);
    let bar_viewport = scrollbars::viewport(outer, reserve_y, reserve_x, user_padding);
    BarSpace {
        bar_viewport,
        reserve_y,
        reserve_x,
    }
}

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
    /// position — the overlay is a [`crate::layout::scrollbars`]
    /// container, and its arrange assigns both rects once measure has
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
        let radius = Corners::all(theme.radius);
        let track = Node::leaf().id(self.track_id).sense(Sense::CLICK);
        if !theme.track.is_noop() {
            let chrome = Background::rounded(theme.track, radius);
            ui.widget(track).record(ui, Some(&chrome), |_| {});
        } else {
            ui.widget(track).record(ui, None, |_| {});
        }

        let fill = if self.thumb.left.drag.delta().is_some() || self.thumb.pressed() {
            theme.thumb_active
        } else if self.thumb.hovered {
            theme.thumb_hover
        } else {
            theme.thumb
        };
        let thumb = Node::leaf().id(self.thumb_id).sense(Sense::DRAG);
        let chrome = Background::rounded(fill, radius);
        ui.widget(thumb).record(ui, Some(&chrome), |_| {});
    }
}

/// One axis's bar resolved against the offset at the moment it was
/// taken. Absent (`Bars::resolve` returning `None`) means the content
/// fits that axis and no thumb shows.
#[derive(Copy, Clone, Debug)]
struct ResolvedBar {
    /// Main-axis length of the track — also the page step, since a
    /// click past the thumb pages by one viewport.
    track_main: f32,
    /// Post-zoom content extent on the main axis.
    content_main: f32,
    thumb_offset: f32,
    thumb_size: f32,
}

impl ResolvedBar {
    /// The offset range this bar can express.
    fn domain(&self) -> BarDomain {
        BarDomain::new(self.content_main, self.track_main)
    }

    fn travel(&self) -> ThumbTravel {
        let domain = self.domain();
        ThumbTravel {
            factor: domain.max_off() / (self.track_main - self.thumb_size).max(f32::EPSILON),
            domain,
        }
    }

    fn page_at(&self, click_main: f32) -> TrackPage {
        TrackPage {
            click_main,
            thumb_offset: self.thumb_offset,
            thumb_size: self.thumb_size,
            page_step: self.track_main,
            domain: self.domain(),
        }
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

    /// This axis's thumb against `offset`, or `None` when the content
    /// fits and no thumb shows.
    fn resolve(
        &self,
        axis: Axis,
        geom: ScrollGeometry,
        scaled: Size,
        offset: f32,
    ) -> Option<ResolvedBar> {
        let track_main = axis.main(geom.space.bar_viewport);
        let content_main = axis.main(scaled);
        let g =
            scrollbars::bar_geometry(track_main, content_main, offset, self.theme.min_thumb_px)?;
        Some(ResolvedBar {
            track_main,
            content_main,
            thumb_offset: g.thumb_offset,
            thumb_size: g.thumb_size,
        })
    }

    /// Fold this frame's bar interaction into the offset: thumb drags
    /// first, then track pages.
    ///
    /// Two passes, not one per axis: a page click reads the offset a
    /// same-frame drag on the *other* axis already moved, and the drag
    /// anchor is a single slot shared by both axes. Resolving each bar
    /// immediately before it is applied is what keeps the thumb tracking
    /// the cursor within the frame.
    pub(super) fn drive(&self, state: &mut ScrollState, geom: ScrollGeometry, pan: BVec2) {
        let scaled = geom.scaled_content(state.zoom);
        for (axis, bar) in self.axes() {
            if !axis.main_b(pan) {
                continue;
            }
            let travel = self
                .resolve(axis, geom, scaled, axis.main_v(state.offset))
                .map(|resolved| resolved.travel());
            state.apply_thumb_drag(
                axis,
                bar.thumb.left.drag.started(),
                bar.thumb.left.drag.delta(),
                travel,
            );
        }
        for (axis, bar) in self.axes() {
            if !axis.main_b(pan) || !bar.track.left.clicked() {
                continue;
            }
            let Some(pointer_local) = bar.track.pointer_local else {
                continue;
            };
            let page = self
                .resolve(axis, geom, scaled, axis.main_v(state.offset))
                .map(|resolved| resolved.page_at(axis.main_v(pointer_local)));
            state.apply_track_page(axis, page);
        }
    }

    /// Record the bar overlay as a sibling of the viewport: a
    /// `scrollbars` container filling the outer rect, holding the four
    /// leaves in the fixed order its driver addresses them by. Painted
    /// after the viewport via record order, hit-tested above it via
    /// cascade order.
    pub(super) fn record(
        &self,
        ui: &mut Ui,
        scroll_id: WidgetId,
        state: ScrollState,
        geom: ScrollGeometry,
        pan: BVec2,
    ) {
        // The viewport was opened on the line above, so this pass's id
        // map already holds its node — the handle the driver needs to
        // reach `scroll_content`.
        let content = ui.current_node(scroll_id);
        let def_id = ui.push_scrollbars_def(ScrollbarsDef {
            content,
            offset: state.offset,
            zoom: state.zoom,
            pan,
            reserve_y: geom.space.reserve_y,
            reserve_x: geom.space.reserve_x,
            padding: geom.padding,
            bar_width: self.theme.width,
            min_thumb: self.theme.min_thumb_px,
        });
        let overlay = Node::scrollbars(def_id)
            .id(scroll_id.with("bars"))
            .size((Sizing::FILL, Sizing::FILL));
        ui.widget(overlay).record(ui, None, |ui| {
            for (_, bar) in self.axes() {
                bar.record(ui, &self.theme);
            }
        });
    }
}

/// How the scrollbars relate to the content area on the pan axes.
///
/// - [`Self::Reserved`] (default): the gutter always takes a strip of
///   the cross axis (`theme.scrollbar.width + gap`), and the bar is
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
    #[default]
    Reserved,
    Overlay,
    Hidden,
}
