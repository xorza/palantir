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
use std::ops::RangeInclusive;

// Logical pixels per wheel "notch" — matches `input::SCROLL_LINE_PIXELS`.
// Used to convert `frame_scroll_delta` (sign-flipped logical pixels) back
// into discrete notches so we can compose `step.powf(notches)` for zoom.
const SCROLL_LINE_PIXELS: f32 = 40.0;

/// What kind of input triggers a zoom step. See [`ZoomConfig::modifier`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomModifier {
    /// Hold `Ctrl` (or `Cmd` on macOS) and turn the wheel. Default.
    /// Bare wheel pans as today.
    CtrlOrCmd,
    /// Plain wheel always zooms (rare; for image viewers without pan).
    Always,
    /// Wheel always pans; only pinch gestures zoom. Touch-first apps.
    PinchOnly,
}

/// Where the zoom step pivots — the point that stays fixed across the
/// scale change.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomPivot {
    /// Pointer position (in widget-local coords). Default — the point
    /// under the cursor stays put across the zoom step.
    Pointer,
    /// Viewport center.
    Center,
}

/// Per-widget zoom configuration. Attach to a `Scroll::both` via
/// [`Scroll::with_zoom`] / [`Scroll::with_zoom_config`].
#[derive(Clone, Debug)]
pub struct ZoomConfig {
    /// Inclusive `[min, max]` zoom range. Default `0.1..=10.0`.
    pub range: RangeInclusive<f32>,
    /// Multiplicative factor per wheel notch; `step.powf(notches)`.
    /// Default `1.1` (10% per notch).
    pub step: f32,
    /// Wheel-vs-pinch routing. Default [`ZoomModifier::CtrlOrCmd`].
    pub modifier: ZoomModifier,
    /// Where the zoom step pivots. Default [`ZoomPivot::Pointer`].
    pub pivot: ZoomPivot,
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            range: 0.1..=10.0,
            step: 1.03,
            modifier: ZoomModifier::CtrlOrCmd,
            pivot: ZoomPivot::Pointer,
        }
    }
}

// `ScrollLayoutState` lives on `LayoutEngine::scroll_states` rather
// than `StateMap` — it's a layout-derived concern, the scroll driver
// writes the layout fields during measure + arrange, and the widget
// reads/mutates the row at record time via [`Ui::scroll_state`].
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
/// viewport panel has user padding) and run `outer.main -
/// other_reservation` (so the V/H bars don't overlap at the corner
/// when both are present, and the length stays stable across the
/// cold-mount two-pass record — `viewport` from cached state lags
/// reservation by one pass and would shrink by `theme.width +
/// theme.gap` on the second frame).
#[allow(clippy::too_many_arguments)]
fn push_bar(
    ui: &mut Ui,
    bar_viewport: Size,
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
    let main = axis.main(bar_viewport);
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
    zoom: Option<ZoomConfig>,
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
        element.sense = Sense::SCROLL;
        // Scroll requires clipping; default to `Rect` so callers that
        // don't override get the cheap scissor path. Callers can still
        // call `Configure::clip_rounded` to upgrade to a stencil mask.
        element.clip = ClipMode::Rect;
        Self {
            element,
            zoom: None,
        }
    }

    /// Enable pivot-anchored zoom with a default [`ZoomConfig`]. Asserts
    /// at record time that the underlying axes are [`ScrollAxes::Both`]
    /// — uniform scale on a single-axis scroll has no clean answer
    /// (cross-axis content escapes the viewport with no way to reach
    /// it). Caller bug, hard error.
    pub fn with_zoom(mut self) -> Self {
        self.zoom = Some(ZoomConfig::default());
        self
    }

    /// Enable zoom with explicit config. See [`Self::with_zoom`].
    pub fn with_zoom_config(mut self, cfg: ZoomConfig) -> Self {
        self.zoom = Some(cfg);
        self
    }

    pub fn show(&self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;
        let axes = match self.element.mode {
            LayoutMode::Scroll(a) => a,
            _ => unreachable!("Scroll widget must carry LayoutMode::Scroll"),
        };
        let pan = axes.pan_mask();
        if self.zoom.is_some() {
            assert!(
                matches!(axes, ScrollAxes::Both),
                "Scroll::with_zoom requires Scroll::both — single-axis scroll has no clean zoom semantics",
            );
        }

        // Record-time clamp + reservation-guess: both use *last*
        // frame's `viewport`/`content`/`overflow`. The matching
        // re-clamp in `scroll::arrange` corrects with fresh numbers
        // post-arrange. Off-axis offsets stay at 0 for single-axis
        // scrolls.
        //
        // Cold-mount: state is default (`seen == false`) → the
        // reservation guess defaults to `(false, false)`, wrong, so
        // we request a relayout. After this pass's record + measure
        // + arrange, `seen` is true and pass B records with the
        // measured reservation in place. Subsequent overflow flips
        // mid-life produce a one-frame visual blip — accepted on
        // the same tier as the wheel-pan clamp's staleness.
        // Input routes by `Sense::SCROLL`, which sits on the outer
        // ZStack (so wheel events over the bar gutter still pan the
        // viewport). Layout state, however, is keyed by the inner
        // viewport node's id — that's the `LayoutMode::Scroll` node
        // the driver writes to.
        let scroll_id = id.with("__viewport");
        let pan_delta_raw = ui.input.scroll_delta_for(id);
        let pinch_delta = ui.input.zoom_delta_for(id);
        let mods = ui.input.modifiers;
        // `mods.ctrl || mods.meta` rather than `Modifiers::any_command`,
        // which folds `alt` in too — alt-wheel shouldn't zoom.
        let wheel_zoom_gate = self.zoom.as_ref().is_some_and(|cfg| match cfg.modifier {
            ZoomModifier::CtrlOrCmd => mods.ctrl || mods.meta,
            ZoomModifier::Always => true,
            ZoomModifier::PinchOnly => false,
        });
        // Route the wheel: when the gate matches, the wheel notches
        // become a multiplicative zoom factor; pan is suppressed for
        // the same frame. Convert sign-flipped logical pixels back into
        // notches; positive scroll_delta.y means scroll-down which by
        // convention zooms *out* (factor < 1).
        let (pan_delta, wheel_zoom_factor) = if wheel_zoom_gate {
            let cfg = self.zoom.as_ref().unwrap();
            let notches_y = pan_delta_raw.y / SCROLL_LINE_PIXELS;
            (Vec2::ZERO, cfg.step.powf(-notches_y))
        } else {
            (pan_delta_raw, 1.0_f32)
        };
        let zoom_delta = pinch_delta * wheel_zoom_factor;
        // Pivot in widget-local coords (outer rect origin). On the
        // first frame the response rect is None — fall back to viewport
        // center, which makes the zoom *feel* anchored even before
        // pointer-tracked anchoring kicks in.
        let resp_rect = ui.response_for(id).rect;
        let widget_origin = resp_rect.map(|r| r.min);
        let widget_size = resp_rect.map(|r| r.size);
        let pivot_local = if (zoom_delta - 1.0).abs() > f32::EPSILON {
            let pointer_local = ui.input.pointer_pos.zip(widget_origin).map(|(p, o)| p - o);
            let cfg_pivot = self
                .zoom
                .as_ref()
                .map(|c| c.pivot)
                .unwrap_or(ZoomPivot::Pointer);
            match (cfg_pivot, pointer_local, widget_size) {
                (ZoomPivot::Pointer, Some(p), _) => Some(p),
                (_, _, Some(sz)) => Some(Vec2::new(sz.w * 0.5, sz.h * 0.5)),
                _ => None,
            }
        } else {
            None
        };
        let scroll = {
            let row = ui.layout.scroll_states.entry(scroll_id).or_default();
            // 1) Zoom step (pivot-anchored). Clamp `new_zoom` to
            //    `cfg.range`, derive the effective `dz_eff`, then
            //    update `offset` so the pivot point in widget-local
            //    coords stays fixed across the scale change.
            if let (Some(cfg), Some(p)) = (self.zoom.as_ref(), pivot_local) {
                let new_zoom = (row.zoom * zoom_delta).clamp(*cfg.range.start(), *cfg.range.end());
                let dz_eff = if row.zoom > 0.0 {
                    new_zoom / row.zoom
                } else {
                    1.0
                };
                if (dz_eff - 1.0).abs() > f32::EPSILON {
                    row.offset = (row.offset + p) * dz_eff - p;
                    row.zoom = new_zoom;
                }
            }
            // 2) Pan from the wheel delta. Only clamp when pan_delta is
            //    actually nonzero — pure-zoom frames must leave the
            //    pivot-anchored offset alone (otherwise repeated tiny
            //    pinches near a content edge would each snap offset
            //    back into the natural range and drift the world point
            //    under the cursor).
            //
            //    Natural range is `[min(0, slack), max(0, slack)]`:
            //    `[0, slack]` for overflow, `[slack, 0]` for underflow.
            //    Pivot-anchored zoom can legitimately leave `offset`
            //    outside that range — e.g. user zooms out below slack=0
            //    (offset goes negative to anchor the cursor), then
            //    zooms back in so slack flips positive while offset is
            //    still negative. A wheel-pan that frame must NOT yank
            //    `offset` back to `[0, slack]` (that's the visible
            //    "snap to top" when the bar reappears). Extend the
            //    clamp range to include the current offset so pan
            //    further out-of-range is blocked but pan toward the
            //    natural range works — the user scrolls back gradually,
            //    never with a one-frame yank.
            let slack_x = row.content.w * row.zoom - row.viewport.w;
            let slack_y = row.content.h * row.zoom - row.viewport.h;
            if pan.x && pan_delta.x != 0.0 {
                let lo = row.offset.x.min(slack_x.min(0.0));
                let hi = row.offset.x.max(slack_x.max(0.0));
                row.offset.x = (row.offset.x + pan_delta.x).clamp(lo, hi);
            }
            if pan.y && pan_delta.y != 0.0 {
                let lo = row.offset.y.min(slack_y.min(0.0));
                let hi = row.offset.y.max(slack_y.max(0.0));
                row.offset.y = (row.offset.y + pan_delta.y).clamp(lo, hi);
            }
            *row
        };
        //todo
        if !scroll.seen {
            ui.request_relayout();
        }
        let outer_size = scroll.outer;
        let content = scroll.content;
        let zoom = scroll.zoom;
        // Bars + reservation reason in *post-zoom* (user-perceived)
        // content extent. `content` is the unscaled measure; multiply
        // here so a zoomed-in subtree forces a bar even when its
        // unscaled extent fit the viewport.
        let scaled_content = Size::new(content.w * zoom, content.h * zoom);
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

        // Bar geometry is derived from outer - reservation - user
        // padding rather than the cached `viewport`. The cached
        // viewport lags by one arrange pass during cold-mount: pass A
        // arranges without reservation (overflow not yet seen) and
        // writes `viewport = outer - user_padding`; pass B records bars
        // off that stale value, and frame 2's pass A finally writes
        // `viewport = outer - reserve - user_padding`, shrinking the
        // bar by ~12px visibly. `outer_size`, the reservations, and
        // the inner padding are all known at record time and stable,
        // so this expression matches the steady-state viewport every
        // frame.
        let user_pad = self.element.padding;
        let bar_viewport = Size::new(
            (outer_size.w - reserve_y - user_pad.horiz()).max(0.0),
            (outer_size.h - reserve_x - user_pad.vert()).max(0.0),
        );

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
        inner.id = scroll_id;
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
        // Children's layout rects are in *absolute* screen coords
        // (e.g. a cell at inner-local (x,y) has `child.rect.min =
        // inner.rect.min + (x,y)`). A bare `TranslateScale(-offset,
        // zoom)` would scale around (0,0), shifting the entire
        // content by `inner.rect.min * (zoom - 1)` — visible drift
        // from the cursor anchor. Compensate by translating so the
        // scale anchors at `inner.rect.min`:
        //   screen = child.abs * zoom + (origin*(1-zoom) - offset)
        // which expands to `inner_local * zoom + origin - offset` —
        // top-left fixed at zoom=any, offset=0; offset translates
        // the scaled content. Origin is sourced from the previous
        // frame's response rect (one-frame stale, fine for stable
        // layouts; the first frame has zoom=1 + offset=0 so the
        // compensation is 0 either way).
        if offset != Vec2::ZERO || (zoom - 1.0).abs() > f32::EPSILON {
            let origin = widget_origin.unwrap_or(Vec2::ZERO);
            inner.transform = Some(TranslateScale::new(origin * (1.0 - zoom) - offset, zoom));
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
                bar_viewport,
                outer_size,
                scaled_content,
                offset,
                Axis::Y,
                pan.y,
                &theme,
            );
            push_bar(
                ui,
                bar_viewport,
                outer_size,
                scaled_content,
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
