use crate::layout::axis::Axis;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sense::Sense;
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::transform::TranslateScale;
use crate::shape::Shape;
use crate::tree::NodeId;
use crate::tree::element::{Configure, Element, LayoutMode, ScrollAxes};
use crate::tree::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::ScrollbarTheme;
use glam::Vec2;

/// One scroll widget recorded this frame: the stable `WidgetId` keying
/// its [`ScrollState`] row plus the per-frame `NodeId` for reading
/// arranged rect / measured content. Pushed during recording, drained
/// in `Ui::end_frame` after arrange.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollNode {
    pub(crate) id: WidgetId,
    pub(crate) node: NodeId,
}

/// Cross-frame state row for one [`Scroll`] widget. Persisted via
/// `Ui::state_mut` keyed by the widget's `WidgetId` and refreshed in
/// `Ui::end_frame` after arrange — `viewport`/`content`/`outer`
/// reflect the just-finished frame, while `offset` is the *next*
/// frame's starting pan position. Clamping uses the previous frame's
/// numbers, so a single frame after a resize may render with a stale
/// clamp; the next frame settles. Single-axis scrolls leave the
/// un-panned axis at 0.
///
/// - `viewport` — INNER (padding-deflated) size: what children see.
///   Drives `content > viewport` overflow checks.
/// - `outer` — full arranged rect size including any reserved
///   scrollbar strips. Drives bar positioning so the bar sits flush
///   with the OUTER far edge (otherwise it'd land inside any
///   user-set padding).
/// - `content` — measured content extent on the panned axes.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub(crate) offset: Vec2,
    pub(crate) viewport: Size,
    pub(crate) outer: Size,
    pub(crate) content: Size,
}

/// Scroll viewport. Three flavors via constructor:
/// - [`Scroll::vertical`]: pans on Y, lays children out as a `VStack`.
/// - [`Scroll::horizontal`]: pans on X, lays children out as an
///   `HStack`.
/// - [`Scroll::both`]: pans on both axes, lays children out as a
///   `ZStack` measured with both axes unbounded.
///
/// All three measure the panned axes as `INF` so children report their
/// full natural extent; the viewport itself takes whatever its parent
/// gave it. Wheel / touchpad input over the viewport pans children via
/// a `transform` applied at record time using the previous frame's
/// clamp.
///
/// **Reservation layout**: when content overflows on a panned axis, the
/// widget reserves `Theme::scrollbar.width` of padding on that axis's
/// far edge and paints the bar in the reserved strip — children
/// measure/arrange against the deflated inner area, never under the
/// bars. When content fits, the reservation collapses and the
/// viewport gets the full size. One-frame stale (uses last-frame
/// overflow state for the decision; same model as the wheel-pan clamp).
/// v1 is indicator-only — drag-to-pan and click-on-track come in a
/// follow-up.
pub struct Scroll {
    element: Element,
}

impl Scroll {
    #[track_caller]
    pub fn vertical() -> Self {
        Self::with_axes(ScrollAxes::Vertical)
    }

    #[track_caller]
    pub fn horizontal() -> Self {
        Self::with_axes(ScrollAxes::Horizontal)
    }

    #[track_caller]
    pub fn both() -> Self {
        Self::with_axes(ScrollAxes::Both)
    }

    #[track_caller]
    fn with_axes(axes: ScrollAxes) -> Self {
        let mut element = Element::new_auto(LayoutMode::Scroll(axes));
        element.clip = ClipMode::Rect;
        element.sense = Sense::Scroll;
        Self { element }
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;
        let pan = match self.element.mode {
            LayoutMode::Scroll(a) => a.pan_mask(),
            _ => unreachable!("Scroll widget must carry LayoutMode::Scroll"),
        };

        // Record-time clamp: uses *last* frame's `viewport`/`content`
        // because this frame's measure hasn't run yet. The matching
        // re-clamp in `Ui::end_frame` corrects with fresh numbers so
        // next frame's record starts in-bounds. Off-axis offsets stay
        // at 0 for single-axis scrolls.
        let delta = ui.input.scroll_delta_for(id);
        let row = ui.state_mut::<ScrollState>(id);
        let max_x = (row.content.w - row.viewport.w).max(0.0);
        let max_y = (row.content.h - row.viewport.h).max(0.0);
        let mut offset = row.offset;
        if pan.x {
            offset.x = (offset.x + delta.x).clamp(0.0, max_x);
        }
        if pan.y {
            offset.y = (offset.y + delta.y).clamp(0.0, max_y);
        }
        row.offset = offset;
        // Snapshot inputs before the body borrows `ui`. Bar geometry
        // uses last frame's measurements (same one-frame staleness as
        // the wheel-pan clamp).
        let viewport = row.viewport;
        let outer = row.outer;
        let content = row.content;
        let theme = ui.theme.scrollbar.clone();

        let mut element = self.element;
        // Reservation: each panned axis with overflow donates
        // `theme.width` to the far-edge padding. Adds to any user-set
        // padding rather than replacing it. Reservation collapses when
        // overflow goes away; one frame later the inner area expands.
        element.padding.right += bar_reservation(pan.y, content.h, viewport.h, &theme);
        element.padding.bottom += bar_reservation(pan.x, content.w, viewport.w, &theme);
        if offset != Vec2::ZERO {
            element.transform = Some(TranslateScale::from_translation(-offset));
        }

        let node = ui.node(element, |ui| {
            // Bar shapes must precede any child node so `Tree::add_shape`'s
            // contiguity invariant holds. They paint owner-relative under
            // the viewport's clip, before the pan transform — so they
            // stay anchored in the reserved strips while content scrolls.
            push_bar(ui, viewport, outer, content, offset, Axis::Y, pan.y, &theme);
            push_bar(ui, viewport, outer, content, offset, Axis::X, pan.x, &theme);
            body(ui);
        });
        ui.scroll_nodes.push(ScrollNode { id, node });

        let resp_state = ui.response_for(id);
        Response {
            node,
            state: resp_state,
        }
    }
}

impl Configure for Scroll {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// Cross-axis space stolen from children when this axis's bar is
/// shown: the bar's `width` plus a `gap` strip of empty padding so
/// the bar doesn't touch the visible content. Returns 0 when no bar
/// is needed (axis not panned, or content fits).
#[inline]
fn bar_reservation(panned: bool, content: f32, viewport: f32, theme: &ScrollbarTheme) -> f32 {
    if panned && content > viewport {
        theme.width + theme.gap
    } else {
        0.0
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct BarGeometry {
    pub(crate) thumb_size: f32,
    pub(crate) thumb_offset: f32,
}

/// Compute thumb size/offset along the bar's main axis. Returns `None`
/// when the bar can't be drawn meaningfully (zero/negative track or no
/// scrollable range).
pub(crate) fn bar_geometry(
    viewport: f32,
    content: f32,
    offset: f32,
    track_len: f32,
    theme: &ScrollbarTheme,
) -> Option<BarGeometry> {
    if track_len <= 0.0 || content <= viewport {
        return None;
    }
    let raw = viewport / content * track_len;
    let thumb_size = raw.max(theme.min_thumb_px).min(track_len);
    let max_off = (content - viewport).max(f32::EPSILON);
    let travel = (track_len - thumb_size).max(0.0);
    let thumb_offset = (offset / max_off).clamp(0.0, 1.0) * travel;
    Some(BarGeometry {
        thumb_size,
        thumb_offset,
    })
}

/// Emit one bar (track + thumb) along `axis` if `panned` and content
/// overflows. Track + thumb sit at the cross-axis far edge of the
/// **outer** rect (so they land in the reserved padding strip even
/// when the user added their own padding) and run the **viewport**'s
/// main-axis extent (so the V/H bars don't overlap at the corner
/// when both are present).
#[allow(clippy::too_many_arguments)]
fn push_bar(
    ui: &mut Ui,
    viewport: Size,
    outer: Size,
    content: Size,
    offset: Vec2,
    axis: Axis,
    panned: bool,
    theme: &ScrollbarTheme,
) {
    if !panned {
        return;
    }
    let main = axis.main(viewport);
    let cross_outer = axis.cross(outer);
    let main_content = axis.main(content);
    let main_offset = axis.main_v(offset);
    let Some(geom) = bar_geometry(main, main_content, main_offset, main, theme) else {
        return;
    };
    let radius = Corners::all(theme.radius);
    let cross_pos = cross_outer - theme.width;
    let track = axis.compose_rect(0.0, cross_pos, main, theme.width);
    if theme.track.a > 0.0 {
        ui.add_shape(Shape::Overlay {
            rect: track,
            radius,
            fill: theme.track,
        });
    }
    let thumb = axis.compose_rect(geom.thumb_offset, cross_pos, geom.thumb_size, theme.width);
    ui.add_shape(Shape::Overlay {
        rect: thumb,
        radius,
        fill: theme.thumb,
    });
}
