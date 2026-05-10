use crate::forest::Forest;
use crate::forest::element::{Configure, Element, LayoutMode, ScrollAxes};
use crate::forest::tree::{Layer, NodeId};
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::result::LayoutResult;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::ui::state::StateMap;
use crate::widgets::Response;
use crate::widgets::theme::ScrollbarTheme;
use glam::Vec2;

/// One scroll widget recorded this frame: the stable `WidgetId` keying
/// its [`ScrollState`] row, the layer it was recorded into, and the
/// per-frame `NodeId`s for the outer container and the inner viewport
/// panel. Pushed during recording, drained in `Ui::end_frame` after
/// arrange. The viewport panel owns the clip + pan transform; the
/// outer node holds the scrollbar gutter reservation. Two ids so
/// `refresh` can read the outer rect (for bar positioning) and the
/// inner rect (for the user-visible viewport size and content extent)
/// from a single frame's layout result.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollNode {
    pub(crate) id: WidgetId,
    pub(crate) layer: Layer,
    pub(crate) outer: NodeId,
    pub(crate) inner: NodeId,
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
    pub(crate) nodes: Vec<ScrollNode>,
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
                s.outer.index() < layout.rect.len() && s.inner.index() < layout.rect.len(),
                "scroll registry entry references nodes ({}, {}) past tree length {}",
                s.outer.index(),
                s.inner.index(),
                layout.rect.len(),
            );
            let outer = layout.rect[s.outer.index()].size;
            let inner_rect = layout.rect[s.inner.index()];
            let inner_pad = tree.records.layout()[s.inner.index()].padding;
            let viewport = inner_rect.deflated_by(inner_pad).size;
            let content = layout.scroll_content[s.inner.index()];
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
        // Scroll requires clipping; default to `Rect` so callers that
        // don't override get the cheap scissor path. Callers can still
        // call `Configure::clip_rounded` to upgrade to a stencil mask.
        element.clip = ClipMode::Rect;
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
        let outer_size = row.outer;
        let content = row.content;
        let theme = ui.theme.scrollbar.clone();

        // Reservation: each panned axis with overflow donates
        // `theme.width` of cross-axis space for the bar to land in.
        // The split layout (outer ZStack hosting the viewport panel
        // + bar shapes) keeps the reservation on the OUTER node so
        // the encoder's padding-deflated clip on the inner panel
        // doesn't swallow the bars.
        let reserve_y = bar_reservation(pan.y, content.h, viewport.h, &theme);
        let reserve_x = bar_reservation(pan.x, content.w, viewport.w, &theme);

        // Outer: bare ZStack that holds the viewport panel + bar
        // shapes. Carries spatial fields (size, margin, align,
        // min/max, sense for wheel input, visibility, disabled,
        // chrome). No clip, no transform, no Scroll layout — just a
        // container with reservation padding so its inner rect
        // matches the viewport rect.
        let mut outer = Element::new(LayoutMode::ZStack);
        outer.id = id;
        outer.auto_id = self.element.auto_id;
        outer.size = self.element.size;
        outer.min_size = self.element.min_size;
        outer.max_size = self.element.max_size;
        outer.margin = self.element.margin;
        outer.align = self.element.align;
        outer.position = self.element.position;
        outer.grid = self.element.grid;
        outer.sense = self.element.sense;
        outer.disabled = self.element.disabled;
        outer.focusable = self.element.focusable;
        outer.visibility = self.element.visibility;
        outer.padding = Spacing {
            right: reserve_y,
            bottom: reserve_x,
            ..Spacing::ZERO
        };

        // Inner viewport: owns the clip, the pan transform, the
        // user-set padding (which the encoder uses to deflate the
        // clip mask), and the actual `Scroll` layout mode that runs
        // children with INF on panned axes.
        let mut inner = Element::new(self.element.mode);
        inner.id = id.with("__viewport");
        inner.size = (Sizing::FILL, Sizing::FILL).into();
        inner.padding = self.element.padding;
        inner.gap = self.element.gap;
        inner.line_gap = self.element.line_gap;
        inner.justify = self.element.justify;
        inner.child_align = self.element.child_align;
        inner.chrome = self.element.chrome;
        // Scroll is always clipped — `with_axes` set `ClipMode::Rect`
        // by default; if the caller upgraded to `Rounded` via
        // `Configure::clip_rounded`, that wins.
        inner.clip = if matches!(self.element.clip, ClipMode::None) {
            ClipMode::Rect
        } else {
            self.element.clip
        };
        if offset != Vec2::ZERO {
            inner.transform = Some(TranslateScale::from_translation(-offset));
        }

        let mut inner_node = NodeId(0);
        let outer_node = ui.node(outer, |ui| {
            inner_node = ui.node(inner, |ui| body(ui));
            // Bars push *after* the viewport panel → they're siblings
            // of the inner panel under the outer ZStack, painted on
            // top of it (record order = paint order). They use
            // `local_rect` in the OUTER's frame so the cross-axis
            // edge sits in the reserved gutter even when the
            // viewport panel has user padding.
            push_bar(
                ui,
                viewport,
                outer_size,
                content,
                offset,
                Axis::Y,
                pan.y,
                &theme,
            );
            push_bar(
                ui,
                viewport,
                outer_size,
                content,
                offset,
                Axis::X,
                pan.x,
                &theme,
            );
        });
        let layer = ui.forest.current_layer();
        ui.scrolls.push(ScrollNode {
            id,
            layer,
            outer: outer_node,
            inner: inner_node,
        });

        let resp_state = ui.response_for(id);
        Response {
            node: outer_node,
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
            stroke: Stroke::ZERO,
        });
    }
    let thumb = axis.compose_rect(geom.thumb_offset, cross_pos, geom.thumb_size, theme.width);
    ui.add_shape(Shape::RoundedRect {
        local_rect: Some(thumb),
        radius,
        fill: theme.thumb,
        stroke: Stroke::ZERO,
    });
}
