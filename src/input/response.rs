//! Widget-facing input results: [`ResponseState`] (one widget's
//! interaction snapshot for the frame), [`ButtonState`] (its per-button
//! slice), [`ButtonPhase`] / [`Drag`] (its press + drag lifecycles), [`ScrollDelta`]
//! (routed wheel/touchpad/pinch deltas), and [`InputDelta`] (the
//! repaint hint `Ui::on_input` returns). These are pure outputs — they
//! never reference the [`InputState`] machine (`super`) that produces
//! them.

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

/// One button's drag lifecycle, carried on [`ButtonState::drag`] — the
/// owning button is the slot's position in [`ResponseState`]. The four
/// phases are mutually exclusive per button, which is why this is an
/// enum rather than an `Option` + edge flags:
///
/// `None` → `Started` (the threshold-crossing frame) → `Active` (every
/// following held frame) → `Stopped` (the release frame) → `None`.
///
/// `delta` is the cumulative pointer travel since press in pre-transform
/// widget-local logical coordinates. It is rect-independent; the pointer
/// may leave the widget's rect mid-drag and the delta keeps tracking.
/// `Stopped` carries no delta: the capture is already gone, so
/// commit-on-release gestures stash the running value while
/// `Started`/`Active` and commit it on `Stopped`.
///
/// A same-frame stop-and-relatch (release + press + threshold-crossing
/// move all in one event batch) reports the fresh `Started` — the new
/// gesture supersedes the stale stop edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Drag {
    #[default]
    None,
    /// One-frame edge: the drag latched this frame. Snapshot anchors
    /// here.
    Started { delta: Vec2 },
    /// Latched on an earlier frame, still held.
    Active { delta: Vec2 },
    /// One-frame edge: the latched drag ended this frame (release).
    Stopped,
}

impl Drag {
    /// Cumulative travel of a live drag (`Started` / `Active`).
    #[inline]
    pub fn delta(self) -> Option<Vec2> {
        match self {
            Drag::Started { delta } | Drag::Active { delta } => Some(delta),
            Drag::None | Drag::Stopped => None,
        }
    }

    /// A drag is live (`Started` / `Active`).
    #[inline]
    pub fn dragging(self) -> bool {
        matches!(self, Drag::Started { .. } | Drag::Active { .. })
    }

    /// One-frame edge: the latch frame.
    #[inline]
    pub fn started(self) -> bool {
        matches!(self, Drag::Started { .. })
    }

    /// One-frame edge: the release frame of a latched drag.
    #[inline]
    pub fn stopped(self) -> bool {
        matches!(self, Drag::Stopped)
    }
}

/// One pointer button's press lifecycle on a widget. The phases are
/// mutually exclusive per frame, walked in order:
///
/// `Idle` → `Down` (press-edge frame) → `Held` (every following held
/// frame) → `Up` (release-edge frame) → `Idle`.
///
/// `Down`/`Held` are capture-based and rect-independent: they keep
/// reporting while the pointer drags outside the widget's rect or off
/// the surface entirely (no travel threshold — live from the first
/// press frame). Drag-tracking widgets (text selection) ride that to
/// keep following the pointer past their own bounds.
///
/// Multi-press runs ride the phases: presses chain when they land on
/// the same widget within the configured double-click time window and
/// pointer radius; any break resets the run. `Down.press` is the press's
/// position in its run (1 = single,
/// 2 = double-press, 3+ = triple…), and a completing click carries the
/// same number in `Up.click` — so `Up { click: Some(2) }` *is* the
/// double-click, and the second click of a double still reads as a
/// click (`clicked()` and `double_clicked()` both fire on it).
///
/// Collapsed edge cases (one event batch, no frame between): a
/// press+release collapses to `Up` (the completed click outranks the
/// lost press edge); a release+re-press collapses to `Down` (the live
/// capture outranks the stale release).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ButtonPhase {
    #[default]
    Idle,
    /// One-frame edge: the press landed this frame. `press` = its
    /// position in the multi-press run. Rises on the press — clicks
    /// fire on the release — so press-driven gestures (caret
    /// placement, press-select-drag) react while the button is still
    /// down.
    Down { press: u8 },
    /// The press is latched on the widget (level, frames after the
    /// press edge).
    Held,
    /// One-frame edge: released this frame. `click` is `Some(n)` when
    /// the release completed a click (press + release on the widget,
    /// no drag latched), with `n` the click's position in its
    /// multi-press run; `None` when a drag suppressed the click or
    /// the release landed off the widget.
    Up { click: Option<u8> },
}

/// One pointer button's slice of a widget's interaction snapshot.
/// [`ResponseState`] carries one per [`PointerButton`] — every button
/// gets the same uniform surface (middle-click is as queryable as
/// left).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonState {
    /// Press lifecycle (see [`ButtonPhase`]).
    pub phase: ButtonPhase,
    /// Drag lifecycle (see [`Drag`]). At most one button's drag is
    /// live per widget: when several buttons are simultaneously
    /// latched, the priority-first in [`PointerButton::all`] wins.
    pub drag: Drag,
}

impl ButtonState {
    /// The press is latched on the widget (`Down` or `Held`) —
    /// rect-independent, no travel threshold.
    #[inline]
    pub fn held(self) -> bool {
        matches!(self.phase, ButtonPhase::Down { .. } | ButtonPhase::Held)
    }

    /// One-frame edge: a press+release landed on the widget without
    /// latching a drag. Fires on the release. For double/triple
    /// dispatch read [`Self::click_count`] (`== 2` is the
    /// double-click).
    #[inline]
    pub fn clicked(self) -> bool {
        matches!(self.phase, ButtonPhase::Up { click: Some(_) })
    }

    /// This frame's press-run position: `0` off the press edge,
    /// 1/2/3+ on it (`press_count() > 0` is the press-rising edge).
    #[inline]
    pub fn press_count(self) -> u8 {
        match self.phase {
            ButtonPhase::Down { press } => press,
            _ => 0,
        }
    }

    /// This frame's click-run position: `0` off the click edge,
    /// 1/2/3+ on it (`2` = double-click, `3` = triple-click).
    #[inline]
    pub fn click_count(self) -> u8 {
        match self.phase {
            ButtonPhase::Up { click } => click.unwrap_or(0),
            _ => 0,
        }
    }

    /// One-frame edge: this click completed a double (its press was
    /// the second in its run). Sugar for `click_count() == 2` — read
    /// [`Self::click_count`] for triple and beyond.
    #[inline]
    pub fn double_clicked(self) -> bool {
        self.click_count() == 2
    }
}

/// Wheel / touchpad / pinch deltas routed to the widget this frame.
/// Only non-identity when the widget has
/// [`Sense::SCROLL`](crate::input::sense::Sense::SCROLL) /
/// [`Sense::PINCH`](crate::input::sense::Sense::PINCH) AND was the
/// topmost routed target when an event arrived. Later pointer movement
/// does not reassign an accumulated delta.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollDelta {
    /// Pixel-precise scroll delta in logical pixels — the touchpad /
    /// precision-wheel source (winit `MouseScrollDelta::PixelDelta`).
    /// Already negated at ingest so `+y` means "advance the scroll
    /// offset forward." Pair with [`Self::lines`] to form a combined
    /// pan delta: `pixels + lines * line_px`.
    pub pixels: Vec2,
    /// Notched / line-discrete scroll delta in raw line units (NOT
    /// pixels) — the classic-wheel source (winit
    /// `MouseScrollDelta::LineDelta`). Sign matches [`Self::pixels`].
    /// Use for "mouse wheel" intent (e.g. zoom-by-notches in a graph
    /// viewport that pans on touchpad).
    pub lines: Vec2,
    /// Multiplicative pinch zoom factor (`1.0` = no pinch). Pinch
    /// always reports — no modifier gating, unlike wheel zoom which
    /// the caller derives manually from [`Self::lines`] + modifiers.
    pub zoom: f32,
}

/// Hand-rolled because `zoom`'s identity is `1.0`, not the `0.0` that
/// `#[derive(Default)]` would produce — `(zoom - 1.0).abs() > eps` is
/// a safe presence check on a `Default`-constructed instance.
impl Default for ScrollDelta {
    fn default() -> Self {
        Self {
            pixels: Vec2::ZERO,
            lines: Vec2::ZERO,
            zoom: 1.0,
        }
    }
}

/// Snapshot of one widget's interaction state for the current frame.
/// `rect` is the widget's last-frame visible surface-space rect (`None`
/// on first frame), after ancestor transforms and clipping.
///
/// `disabled` is the **cascaded** disabled flag (the widget OR any ancestor),
/// read from the previous frame's cascade — one-frame stale, like
/// hover/press. Widgets that need lag-free self-disabled visuals also
/// merge their own `Element::disabled` (`state.disabled |= element.disabled`)
/// before reading the field.
///
/// `focused` is `true` when this widget currently holds keyboard focus
/// (`Ui::focused_id() == Some(id)`). Updated synchronously with focus
/// changes, so unlike `hovered`/`left.held` it isn't one-frame stale —
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
    /// into unclipped surface space. The visible [`Self::rect`] may be
    /// smaller when an ancestor clips the widget.
    /// [`TranslateScale::IDENTITY`] when the widget sits under no transform.
    pub transform: TranslateScale,
    /// Cursor position in pre-transform widget-local logical coordinates,
    /// relative to [`Self::layout_rect`]'s origin. `None` when the pointer
    /// is off-surface or the widget didn't arrange. This remains relative
    /// to the full widget when ancestor clipping trims [`Self::rect`].
    pub pointer_local: Option<Vec2>,
    pub hovered: bool,
    pub disabled: bool,
    pub focused: bool,
    /// Primary-button state. The classic single-pointer surface
    /// (`clicked`, `held`, press runs, drags) lives here.
    pub left: ButtonState,
    /// Secondary-button state — `right.clicked` is the context-menu
    /// trigger.
    pub right: ButtonState,
    pub middle: ButtonState,
    /// Wheel / touchpad / pinch deltas routed to this widget.
    pub scroll: ScrollDelta,
}

impl Default for ResponseState {
    fn default() -> Self {
        Self {
            rect: None,
            layout_rect: None,
            transform: TranslateScale::IDENTITY,
            pointer_local: None,
            hovered: false,
            disabled: false,
            focused: false,
            left: ButtonState::default(),
            right: ButtonState::default(),
            middle: ButtonState::default(),
            scroll: ScrollDelta::default(),
        }
    }
}

impl ResponseState {
    /// The per-button slice for a **runtime** `button` value — the one
    /// thing the public fields can't express. For a compile-time-known
    /// button read the field directly (`state.left`, not
    /// `state.button(PointerButton::Left)`); reach for this only when
    /// the button is a variable (configurable gesture bindings, loops
    /// over [`PointerButton::all`]).
    #[inline]
    pub fn button(&self, button: PointerButton) -> &ButtonState {
        match button {
            PointerButton::Left => &self.left,
            PointerButton::Right => &self.right,
            PointerButton::Middle => &self.middle,
        }
    }

    /// Left-button press with the pointer still over the widget — the
    /// "shows pressed visuals" predicate. Derived: `left.held &&
    /// hovered` (a held press whose pointer wandered off reports
    /// `left.held` but not `pressed`). The only cross-field
    /// derivation on this type — everything per-button reads its
    /// slot: `state.left.clicked()`, `state.left.drag.delta()`,
    /// `state.left.double_clicked()`.
    #[inline]
    pub fn pressed(&self) -> bool {
        self.left.held() && self.hovered
    }
}
