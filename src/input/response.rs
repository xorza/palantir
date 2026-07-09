//! Widget-facing input results: [`ResponseState`] (one widget's
//! interaction snapshot for the frame), [`DragState`] (its active drag,
//! if any), and [`InputDelta`] (the repaint hint `Ui::on_input` returns).
//! These are pure outputs — they never reference the [`InputState`]
//! machine (`super`) that produces them.

use glam::Vec2;

use crate::input::pointer::PointerButton;
use crate::primitives::rect::Rect;
use crate::primitives::transform::TranslateScale;

/// Repaint hint returned by `Ui::on_input`: `true` when the event
/// changed something the next frame must reflect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputDelta {
    pub requests_repaint: bool,
}

/// One widget's active drag. Carried on
/// [`ResponseState::drag`]. Only one drag exists per widget at a
/// time — when multiple buttons are simultaneously latched on the
/// same widget, the priority-first in
/// [`crate::input::pointer::PointerButton::all`] wins.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragState {
    /// Which pointer button is doing the dragging.
    pub button: PointerButton,
    /// Cumulative pointer travel since press. Rect-independent; the
    /// pointer may leave the widget's rect mid-drag and the delta
    /// keeps tracking.
    pub delta: Vec2,
    /// One-frame edge: `true` on exactly the frame the drag latched
    /// (the threshold-crossing pointer move). Snapshot anchors here.
    pub started: bool,
}

/// Snapshot of one widget's interaction state for the current frame.
/// `rect` is the widget's last-frame logical-pixel rect (`None` on first frame).
///
/// `disabled` is the **cascaded** disabled flag (the widget OR any ancestor),
/// read from the previous frame's cascade — one-frame stale, like
/// hover/press. Widgets that need lag-free self-disabled visuals also
/// merge their own `Element::disabled` (`state.disabled |= element.disabled`)
/// before reading the field.
///
/// `focused` is `true` when this widget currently holds keyboard focus
/// (`Ui::focused_id() == Some(id)`). Updated synchronously with focus
/// changes, so unlike `hovered`/`pressed` it isn't one-frame stale —
/// a widget that just called `ui.request_focus(id)` reads `true` on
/// the same frame.
#[derive(Clone, Copy, Debug)]
pub struct ResponseState {
    pub rect: Option<Rect>,
    /// Pre-transform, unclipped layout rect in world coords — the
    /// widget's arranged position before any ancestor `transform`
    /// (scroll pan/zoom) or `clip` is applied. Use when you need a
    /// widget's true position regardless of how its parent scrolls
    /// or clips it; subtract two such rects to get one widget's
    /// owner-local offset under another.
    pub layout_rect: Option<Rect>,
    /// Cumulative ancestor transform mapping this widget's `layout_rect`
    /// into `rect` (screen space): `rect == transform.apply_rect(layout_rect)`.
    /// [`TranslateScale::IDENTITY`] when the widget sits under no transform.
    /// Use it to convert a surface-space pointer into the widget's own
    /// logical coordinates — e.g. `(ptr - rect.min) / transform.scale` for
    /// hit-testing content laid out in logical px under a zoomed canvas.
    pub transform: TranslateScale,
    pub hovered: bool,
    pub pressed: bool,
    /// The primary (left) button's press is currently latched on this
    /// widget — `true` from the press that captured it until release,
    /// **regardless of where the pointer has since moved**. Unlike
    /// [`Self::pressed`] (which also requires the pointer to stay over
    /// the widget), this keeps reporting while the pointer drags outside
    /// the widget's rect or off the surface entirely, and — unlike
    /// [`Self::drag`] — it has no travel threshold, so it's live from the
    /// very first press frame. Drag-tracking widgets (text selection)
    /// use it to keep following the pointer past their own bounds.
    pub held: bool,
    pub clicked: bool,
    /// One-frame edge: right-button click landed and released on this
    /// widget without a drag. Independent of `clicked` (left-button).
    pub secondary_clicked: bool,
    pub disabled: bool,
    pub focused: bool,
    /// Active drag on this widget — `None` outside drag and for
    /// sub-threshold wiggle. Only one button can drag a given widget
    /// at a time; when more than one button is captured + latched,
    /// the priority-first in `PointerButton::all()` wins. Callers go
    /// through [`Self::drag_delta`] / [`Self::drag_started`] /
    /// [`Self::dragged_by`] etc. rather than reading this field.
    pub drag: Option<DragState>,
    /// One-frame edge: the button whose latched drag on this widget
    /// ended this frame (release). The drag itself is already gone —
    /// [`Self::drag`] is `None` and [`Self::drag_delta`] can't report
    /// the final travel — so commit-on-release gestures stash their
    /// running value and key the commit off this edge. `None` outside
    /// that frame. Callers use [`Self::drag_stopped`] /
    /// [`Self::drag_stopped_by`] rather than reading this field.
    pub drag_stopped: Option<PointerButton>,
    /// One-frame edge: the button that just fired a double-click on
    /// this widget (two clicks on the same id within
    /// [`crate::input::sense::DOUBLE_CLICK_WINDOW`]). `None` outside
    /// that frame. Callers use [`Self::double_clicked`] /
    /// [`Self::double_clicked_by`] rather than reading this field.
    pub double_click: Option<PointerButton>,
    /// Pixel-precise scroll delta this frame, in logical pixels — the
    /// touchpad / precision-wheel source (winit
    /// `MouseScrollDelta::PixelDelta`). Already negated at ingest so
    /// `+y` means "advance the scroll offset forward." Only non-zero
    /// when the widget has [`Sense::SCROLL`](crate::input::sense::Sense::SCROLL)
    /// AND was the topmost scroll target under the pointer this frame.
    /// Pair with [`Self::scroll_lines`] to form a combined pan delta:
    /// `scroll_pixels + scroll_lines * line_px`.
    pub scroll_pixels: Vec2,
    /// Notched / line-discrete scroll delta this frame, in raw line
    /// units (NOT pixels) — the classic-wheel source (winit
    /// `MouseScrollDelta::LineDelta`). Sign matches `scroll_pixels`
    /// (`+y` = advance offset). Use for "mouse wheel" intent (e.g.
    /// zoom-by-notches in a graph viewport that pans on touchpad).
    /// Same routing as `scroll_pixels`.
    pub scroll_lines: Vec2,
    /// Multiplicative pinch zoom factor this frame (`1.0` = no
    /// pinch). Same routing as `scroll_pixels`/`scroll_lines` (widget
    /// must have [`Sense::SCROLL`](crate::input::sense::Sense::SCROLL)
    /// and be the topmost target). Pinch always reports — no modifier
    /// gating, unlike wheel zoom which the caller derives manually from
    /// `scroll_lines` + modifiers.
    pub zoom_factor: f32,
    /// Cursor position relative to this widget's `rect.min`. `None`
    /// when the pointer is off-surface or the widget didn't arrange
    /// (no `rect`). Useful as a pivot for zoom-about-cursor or any
    /// custom local-space hit math without recomputing the rect
    /// origin at every call site.
    pub pointer_local: Option<Vec2>,
}

/// Hand-rolled because `zoom_factor`'s identity is `1.0`, not the
/// `0.0` that `#[derive(Default)]` would produce — `(factor - 1.0)
/// .abs() > eps` is a safe presence check for routed pinch on a
/// `Default`-constructed instance.
impl Default for ResponseState {
    fn default() -> Self {
        Self {
            rect: None,
            layout_rect: None,
            transform: TranslateScale::IDENTITY,
            hovered: false,
            pressed: false,
            held: false,
            clicked: false,
            secondary_clicked: false,
            disabled: false,
            focused: false,
            drag: None,
            drag_stopped: None,
            double_click: None,
            scroll_pixels: Vec2::ZERO,
            scroll_lines: Vec2::ZERO,
            zoom_factor: 1.0,
            pointer_local: None,
        }
    }
}

impl ResponseState {
    /// Any pointer button currently dragging this widget. Sugar for
    /// `self.drag.is_some()`.
    #[inline]
    pub fn dragged(&self) -> bool {
        self.drag.is_some()
    }

    /// `true` when `button` is the one dragging this widget. For "is
    /// anything dragging?" use [`Self::dragged`].
    #[inline]
    pub fn dragged_by(&self, button: PointerButton) -> bool {
        self.drag.is_some_and(|d| d.button == button)
    }

    /// One-frame edge: `true` on the frame the active drag latches.
    /// Snapshot anchors here.
    #[inline]
    pub fn drag_started(&self) -> bool {
        self.drag.is_some_and(|d| d.started)
    }

    /// One-frame edge filtered by button. `drag_started_by(Middle)`
    /// is `true` only on the frame a middle-button drag latches.
    #[inline]
    pub fn drag_started_by(&self, button: PointerButton) -> bool {
        self.drag.is_some_and(|d| d.button == button && d.started)
    }

    /// Cumulative pointer travel of the active drag, regardless of
    /// which button is dragging.
    #[inline]
    pub fn drag_delta(&self) -> Option<Vec2> {
        self.drag.map(|d| d.delta)
    }

    /// Cumulative pointer travel, but only if the dragging button is
    /// `button`. `None` outside drag or when a different button is
    /// dragging.
    #[inline]
    pub fn drag_delta_by(&self, button: PointerButton) -> Option<Vec2> {
        self.drag.filter(|d| d.button == button).map(|d| d.delta)
    }

    /// One-frame edge: a latched drag on this widget ended this frame
    /// (any button). The drag state is already gone by now — stash the
    /// running value during the drag and commit it on this edge.
    #[inline]
    pub fn drag_stopped(&self) -> bool {
        self.drag_stopped.is_some()
    }

    /// One-frame edge filtered by button. `drag_stopped_by(Left)` is
    /// the standard "scrub finished, commit" gesture.
    #[inline]
    pub fn drag_stopped_by(&self, button: PointerButton) -> bool {
        self.drag_stopped == Some(button)
    }

    /// One-frame edge: any pointer button double-clicked this widget
    /// this frame. Mirror of [`Self::dragged`] for two-click gestures.
    #[inline]
    pub fn double_clicked(&self) -> bool {
        self.double_click.is_some()
    }

    /// One-frame edge filtered by button. `double_clicked_by(Left)`
    /// is the standard "open / activate" gesture; right- or middle-
    /// double-clicks are rarer but available without extra plumbing.
    #[inline]
    pub fn double_clicked_by(&self, button: PointerButton) -> bool {
        self.double_click == Some(button)
    }
}
