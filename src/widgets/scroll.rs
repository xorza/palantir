use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::result::LayoutResult;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::transform::TranslateScale;
use crate::shape::Shape;
use crate::tree::element::{Configure, Element, LayoutMode, ScrollAxes};
use crate::tree::forest::Forest;
use crate::tree::widget_id::WidgetId;
use crate::tree::{Layer, NodeId};
use crate::ui::Ui;
use crate::ui::state::StateMap;
use crate::widgets::Response;
use crate::widgets::theme::{ScrollbarTheme, Surface};
use glam::Vec2;

/// One scroll widget recorded this frame: the stable `WidgetId` keying
/// its [`ScrollState`] row, the layer it was recorded into, and the
/// per-frame `NodeId` for reading arranged rect / measured content.
/// Pushed during recording, drained in `Ui::end_frame` after arrange.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollNode {
    pub(crate) id: WidgetId,
    pub(crate) layer: Layer,
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

/// Per-frame registry of recorded [`Scroll`] widgets. Pushed during
/// recording, drained in `Ui::end_frame` after arrange to refresh each
/// widget's [`ScrollState`] row. Capacity-retained across frames.
#[derive(Default)]
pub(crate) struct ScrollRegistry {
    nodes: Vec<ScrollNode>,
}

impl ScrollRegistry {
    pub(crate) fn begin_frame(&mut self) {
        self.nodes.clear();
    }

    pub(crate) fn push(&mut self, node: ScrollNode) {
        self.nodes.push(node);
    }

    /// Refresh each registered scroll widget's state row with the
    /// freshly-arranged viewport + measured content extent. Called
    /// post-arrange / pre-cascade so next frame's record clamps with
    /// up-to-date numbers; the current frame's pan already used last
    /// frame's clamp.
    pub(crate) fn refresh(&self, forest: &Forest, results: &LayoutResult, state: &mut StateMap) {
        for s in self.nodes.iter().copied() {
            let tree = forest.tree(s.layer);
            let layout = &results[s.layer];
            assert!(
                s.node.index() < layout.rect.len(),
                "scroll registry entry references node {} past tree length {}",
                s.node.index(),
                layout.rect.len(),
            );
            let outer_rect = layout.rect[s.node.index()];
            let pad = tree.records.layout()[s.node.index()].padding;
            let outer = outer_rect.size;
            let viewport = outer_rect.deflated_by(pad).size;
            let content = layout.scroll_content[s.node.index()];
            let row = state.get_or_insert_with::<ScrollState, _>(s.id, Default::default);
            row.viewport = viewport;
            row.outer = outer;
            row.content = content;
            // End-frame re-clamp: pairs with the record-time clamp in
            // `Scroll::show`, which only had last frame's numbers.
            let max_x = (content.w - viewport.w).max(0.0);
            let max_y = (content.h - viewport.h).max(0.0);
            row.offset.x = row.offset.x.clamp(0.0, max_x);
            row.offset.y = row.offset.y.clamp(0.0, max_y);
        }
    }
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
    surface: Option<Surface>,
}

impl Scroll {
    pub fn vertical() -> Self {
        Self::with_axes(ScrollAxes::Vertical)
    }

    pub fn horizontal() -> Self {
        Self::with_axes(ScrollAxes::Horizontal)
    }

    pub fn both() -> Self {
        Self::with_axes(ScrollAxes::Both)
    }

    fn with_axes(axes: ScrollAxes) -> Self {
        let mut element = Element::new(LayoutMode::Scroll(axes));
        element.sense = Sense::Scroll;
        Self {
            element,
            surface: None,
        }
    }

    /// Install chrome for the scroll viewport. Accepts a bare `Background`
    /// (paint-only — clip stays scissor) or a full `Surface`. Scroll
    /// requires clipping, so `ClipMode::None` on the supplied surface is
    /// upgraded to `Rect`; `Rect` and `Rounded` pass through.
    pub fn background(mut self, s: impl Into<Surface>) -> Self {
        let mut s = s.into();
        if matches!(s.clip, ClipMode::None) {
            s.clip = ClipMode::Rect;
        }
        self.surface = Some(s);
        self
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

        // Default to scissor when no user surface — Scroll is always clipped.
        let surface = self.surface.unwrap_or_else(Surface::clip_rect);
        let node = ui.node(element, Some(surface), |ui| {
            body(ui);
            // Bars push *after* the body → they land in the trailing
            // shape slot (after every child), so the encoder paints
            // them on top of content. They paint owner-relative under
            // the viewport's clip and outside the owner's pan, so
            // they stay anchored in the reserved strips while content
            // scrolls. Chrome paint is emitted by the encoder via
            // `Tree::chrome_for`, so the panel's own background sits
            // behind these bars.
            push_bar(ui, viewport, outer, content, offset, Axis::Y, pan.y, &theme);
            push_bar(ui, viewport, outer, content, offset, Axis::X, pan.x, &theme);
        });
        let layer = ui.forest.recording.current_layer;
        ui.scrolls.push(ScrollNode { id, layer, node });

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
        ui.add_shape(Shape::RoundedRect {
            local_rect: Some(track),
            radius,
            fill: theme.track,
            stroke: None,
        });
    }
    let thumb = axis.compose_rect(geom.thumb_offset, cross_pos, geom.thumb_size, theme.width);
    ui.add_shape(Shape::RoundedRect {
        local_rect: Some(thumb),
        radius,
        fill: theme.thumb,
        stroke: None,
    });
}
