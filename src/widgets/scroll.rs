use crate::forest::element::{Configure, Element, LayoutMode, ScrollAxes};
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

// `ScrollLayoutState` lives on `LayoutEngine::scroll_states` rather
// than `StateMap` — it's a layout-derived concern, refresh writes
// the layout fields after arrange, and the widget reads/mutates the
// row at record time via [`Ui::scroll_state`].
//
// Bar drawing + reservation logic stay here as widget concerns; the
// layout primitive itself is unaware of scrollbars.

// ---------------------------------------------------------------------------
// Bar geometry helpers
// ---------------------------------------------------------------------------

/// Cross-axis space stolen from children when an axis's bar is shown:
/// the bar's `width` plus a `gap` strip so the bar doesn't touch the
/// visible content. Returns 0 when the axis isn't currently
/// overflowing (or isn't panned at all).
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
/// **outer** rect (so they land in the reserved gutter even when the
/// viewport panel has user padding) and run the **viewport**'s
/// main-axis extent (so the V/H bars don't overlap at the corner when
/// both are present).
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

// ---------------------------------------------------------------------------
// Scroll widget
// ---------------------------------------------------------------------------

/// Scroll viewport. Three flavors via constructor:
/// - [`Scroll::vertical`]: pans on Y, lays children out as a `VStack`.
/// - [`Scroll::horizontal`]: pans on X, lays children out as an
///   `HStack`.
/// - [`Scroll::both`]: pans on both axes, lays children out as a
///   `ZStack` measured with both axes unbounded.
///
/// All three measure the panned axes as `INF` so children report
/// their full natural extent; the viewport itself takes whatever its
/// parent gave it. Wheel / touchpad input over the viewport pans
/// children via a `transform` applied at record time using the
/// previous frame's clamp.
///
/// **Reservation layout**: when content overflows on a panned axis,
/// the widget reserves `theme.scrollbar.width + gap` of padding on
/// the cross-axis far edge and paints the bar in the reserved strip.
/// The reservation decision is record-time, sourced from last frame's
/// `ScrollState.overflow`. When `refresh` detects the overflow flag
/// flipped after measure, it returns `true` and `Ui` retries the
/// frame with the corrected reservation — same model as
/// `Ui::request_relayout`. v1 is indicator-only; drag-to-pan and
/// click-on-track come in a follow-up.
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

        // Record-time clamp + reservation-guess: both use *last*
        // frame's `viewport`/`content`/`overflow`. The matching
        // re-clamp in `LayoutEngine::refresh_scrolls` corrects with
        // fresh numbers post-arrange. Off-axis offsets stay at 0 for
        // single-axis scrolls.
        //
        // Cold-mount: state is default (`seen == false`) → the
        // reservation guess defaults to `(false, false)`, wrong, so
        // we request a relayout. After this pass's record + measure
        // + refresh, `seen` is true and pass B records with the
        // measured reservation in place. Subsequent overflow flips
        // mid-life produce a one-frame visual blip — accepted on
        // the same tier as the wheel-pan clamp's staleness.
        let delta = ui.input.scroll_delta_for(id);
        let scroll = {
            let row = ui.scroll_state(id);
            let max_x = (row.content.w - row.viewport.w).max(0.0);
            let max_y = (row.content.h - row.viewport.h).max(0.0);
            if pan.x {
                row.offset.x = (row.offset.x + delta.x).clamp(0.0, max_x);
            }
            if pan.y {
                row.offset.y = (row.offset.y + delta.y).clamp(0.0, max_y);
            }
            *row
        };
        if !scroll.seen {
            ui.request_relayout();
        }
        let viewport = scroll.viewport;
        let outer_size = scroll.outer;
        let content = scroll.content;
        let overflow = scroll.overflow;
        let offset = scroll.offset;
        let theme = ui.theme.scrollbar.clone();

        // Reservation: a panned axis with current-state overflow
        // donates `theme.width + theme.gap` of cross-axis space for
        // the bar to land in. The Y bar steals X (`right` padding);
        // the X bar steals Y (`bottom` padding). When `overflow`
        // flips after measure, `refresh` returns `true` so this same
        // frame re-records with the corrected reservation — no
        // cold-mount overlap, no empty strip when content fits.
        let reserve_y = bar_reservation(pan.y && overflow.1, &theme);
        let reserve_x = bar_reservation(pan.x && overflow.0, &theme);

        // Outer: bare ZStack that holds the inner viewport + bar
        // shapes. Its padding is the reservation gutter — encoder's
        // standard clip-mask deflation picks it up. No clip on outer
        // so bars (its direct shapes) paint unclipped; the inner
        // panel clips its own children.
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

        let outer_node = ui.node(outer, |ui| {
            ui.node(inner, |ui| body(ui));
            // Bars push *after* the viewport panel → siblings under
            // the outer ZStack, painted on top (record order = paint
            // order). Local rects in OUTER frame so they land in the
            // reserved gutter even when the viewport panel has user
            // padding.
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
