use crate::forest::element::{Configure, Element, LayoutMode, ScrollAxes};
use crate::forest::tree::{Layer, NodeId};
use crate::forest::widget_id::WidgetId;
use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::shape::Shape;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::ScrollbarTheme;
use glam::Vec2;

/// Per-frame record-side metadata for one [`Scroll`] widget. Pushed
/// onto `Ui::scroll_widgets` in [`Scroll::show`], drained by
/// `Ui::end_frame_record_phase` after `layout.run` to refresh the
/// widget's [`ScrollState`] row. NodeIds are this frame's pre-order
/// indices (the tree is rebuilt every frame); `inner_padding` is the
/// user-supplied padding the encoder uses to deflate the clip mask.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollWidget {
    pub(crate) layer: Layer,
    pub(crate) id: WidgetId,
    pub(crate) outer: NodeId,
    pub(crate) inner: NodeId,
    pub(crate) inner_padding: Spacing,
}

/// Per-frame collection of [`ScrollWidget`] entries pushed during
/// recording. Drained by [`Self::refresh`] after `layout.run`.
/// `entries` is `pub(crate)` so callers push/clear directly — the
/// only method here is the one that does real work.
#[derive(Default)]
pub(crate) struct ScrollWidgets {
    pub(crate) entries: Vec<ScrollWidget>,
}

impl ScrollWidgets {
    /// Update each widget's [`ScrollState`] row from this frame's
    /// arranged rects + measured content extent. Returns `true` if
    /// any widget's `overflow` flag flipped — signals that the
    /// record-time reservation decision used stale data and the frame
    /// should be re-recorded with corrected state.
    ///
    /// Content-extent lookup: `LayoutEngine::scroll_measures` carries
    /// `(layer, inner, content)` for every scroll widget whose
    /// measure arm fired this frame. On a measure-cache hit at any
    /// ancestor (or at the scroll itself) the arm doesn't fire and no
    /// entry is pushed — but the cache hit is keyed on
    /// `subtree_hash`, so the cached subtree's measure output is
    /// byte-identical to last frame's. Last frame's
    /// `ScrollState.content` is therefore still correct, and we just
    /// leave the row's `content` untouched.
    pub(crate) fn refresh(
        &self,
        layout: &crate::layout::LayoutEngine,
        state: &mut crate::ui::state::StateMap,
    ) -> bool {
        let mut overflow_flipped = false;
        for w in &self.entries {
            let rects = &layout.result[w.layer].rect;
            assert!(
                w.outer.index() < rects.len() && w.inner.index() < rects.len(),
                "scroll widget references nodes ({}, {}) past tree length {}",
                w.outer.index(),
                w.inner.index(),
                rects.len(),
            );
            let outer = rects[w.outer.index()].size;
            let viewport = rects[w.inner.index()].deflated_by(w.inner_padding).size;
            let row = state.get_or_insert_with::<ScrollState, _>(w.id, Default::default);
            let content = layout
                .scroll_measures
                .iter()
                .find(|m| m.layer == w.layer && m.inner == w.inner)
                .map(|m| m.content)
                .unwrap_or(row.content);
            let new_overflow = (content.w > viewport.w, content.h > viewport.h);
            overflow_flipped |= row.overflow != new_overflow;
            row.viewport = viewport;
            row.outer = outer;
            row.content = content;
            row.overflow = new_overflow;
            // End-frame re-clamp: pairs with the record-time clamp in
            // `Scroll::show`, which only had last frame's numbers.
            let max_x = (content.w - viewport.w).max(0.0);
            let max_y = (content.h - viewport.h).max(0.0);
            row.offset.x = row.offset.x.clamp(0.0, max_x);
            row.offset.y = row.offset.y.clamp(0.0, max_y);
        }
        overflow_flipped
    }
}

/// Cross-frame state row for one [`Scroll`] widget. Persisted via
/// `Ui::state_mut` keyed by the widget's `WidgetId` and refreshed in
/// `Ui::end_frame` after arrange — `viewport`/`content`/`overflow`
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
/// - `overflow` — `(x, y)` per-axis: did this axis's content overflow
///   the viewport on the most recent measure? Read at *record time*
///   to decide whether to reserve a scrollbar gutter on the cross
///   axis. When `refresh` flips this away from the value record-time
///   used, it requests a relayout so the same frame re-records with
///   the corrected reservation — no cold-mount overlap.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub(crate) offset: Vec2,
    pub(crate) viewport: Size,
    pub(crate) outer: Size,
    pub(crate) content: Size,
    pub(crate) overflow: (bool, bool),
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
        // the wheel-pan clamp). `overflow` is read here for the
        // reservation decision below; if it disagrees with this
        // frame's measure, the post-arrange `ScrollHook` requests a
        // relayout and the same frame re-records with the corrected
        // value.
        let viewport = row.viewport;
        let outer_size = row.outer;
        let content = row.content;
        let overflow = row.overflow;
        let theme = ui.theme.scrollbar.clone();

        // Reservation: a panned axis with current-state overflow
        // donates `theme.width + theme.gap` of cross-axis space for
        // the bar to land in. When `overflow` flips after measure,
        // refresh requests a relayout so this same frame re-records
        // with the right reservation — no cold-mount overlap, and no
        // empty strip when content fits.
        let reserve_y = bar_reservation(pan.y && overflow.1, &theme);
        let reserve_x = bar_reservation(pan.x && overflow.0, &theme);

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
        ui.scroll_widgets.entries.push(ScrollWidget {
            layer,
            id,
            outer: outer_node,
            inner: inner_node,
            inner_padding: self.element.padding,
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
/// the bar doesn't touch the visible content. Returns 0 when the
/// axis isn't currently overflowing (or isn't panned at all). The
/// caller passes the post-relayout `overflow.{x,y}` flag from
/// `ScrollState`, so this stays a pure function of "is the bar
/// drawn this frame."
#[inline]
fn bar_reservation(visible: bool, theme: &ScrollbarTheme) -> f32 {
    if visible {
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
