#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod key_class;
pub(crate) mod keyboard;
pub(crate) mod pointer;
pub(crate) mod policy;
pub(crate) mod response;
pub(crate) mod scope;
pub(crate) mod sense;
pub(crate) mod shortcut;
pub(crate) mod watch;
pub(crate) mod zoom;

use crate::input::key_class::KeyClass;
use crate::input::keyboard::{Key, KeyPress, KeyboardEvent, Modifiers, TextChunk};
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::policy::FocusPolicy;
use crate::input::policy::InputSignal;
use crate::input::response::{
    ButtonPhase, ButtonState, Drag, InputDelta, ResponseState, ScrollDelta,
};
use crate::input::response::{PointerAction, PointerEdge};
use crate::input::scope::Scopes;
use crate::input::sense::{DOUBLE_CLICK_RADIUS, DOUBLE_CLICK_WINDOW, DRAG_THRESHOLD, Sense};
use crate::input::shortcut::Shortcut;
use crate::input::watch::{KeyboardWake, PointerWake, Watches};
use crate::layout::Layout;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use crate::scene::layer::Layer;
use glam::Vec2;
use std::time::Duration;
use strum::EnumCount as _;

fn pointer_in_widget_space(pointer: Vec2, layout_origin: Vec2, transform: TranslateScale) -> Vec2 {
    let surface_origin = transform.apply_point(layout_origin);
    transform.inverse_vector(pointer - surface_origin)
}

/// Per-button capture. One slot per [`PointerButton`]; three
/// all-or-nothing pieces rather than twelve loose fields, so the
/// invariants (a capture always has a press origin, a drag latch always
/// has a capture, click and drag-stop never coexist, the run tracker
/// never half-exists) are unrepresentable rather than maintained by
/// convention.
#[derive(Default, Clone, Copy, Debug)]
struct Capture {
    /// The in-flight press, created on the press event and destroyed
    /// by release / cascade-eviction. `Some` == "this button's
    /// capture is latched".
    press: Option<Press>,
    /// One-frame edge: how a capture ended this frame. Cleared by
    /// `end_frame`.
    release: Option<Release>,
    /// Multi-press run tracker. Persists *across* presses (that's the
    /// chaining) — never cleared, only replaced by the next press.
    run: Option<PressRun>,
}

impl Capture {
    /// Latch a press on `target` at `pos`, chaining the multi-press
    /// run when it lands on the same target within
    /// [`DOUBLE_CLICK_WINDOW`] of the previous press and
    /// [`DOUBLE_CLICK_RADIUS`] of its position; any break restarts the
    /// run at 1. `seq` saturates so a caffeinated 255-click run can't
    /// wrap back to "single".
    fn begin_press(&mut self, target: WidgetId, pos: Vec2, now: Duration) {
        let seq = match &self.run {
            Some(run)
                if run.target == target
                    && now.saturating_sub(run.at) <= DOUBLE_CLICK_WINDOW
                    && pos.distance(run.pos) <= DOUBLE_CLICK_RADIUS =>
            {
                run.seq.saturating_add(1)
            }
            _ => 1,
        };
        self.run = Some(PressRun {
            at: now,
            target,
            pos,
            seq,
        });
        self.press = Some(Press {
            target,
            origin: pos,
            seq,
            fresh: true,
            drag: PressDrag::None,
        });
    }
}

/// One in-flight press: the capture target, the drag anchor, and this
/// press's run position, bundled so none can exist without the others.
#[derive(Clone, Copy, Debug)]
struct Press {
    /// Widget the press latched onto.
    target: WidgetId,
    /// Pointer position at the press. Subtracted from the current
    /// pointer position for rect-independent drag deltas.
    origin: Vec2,
    /// This press's position in its multi-press run (1 = single,
    /// 2 = double-press, 3+ = triple…), stamped from [`PressRun::seq`]
    /// at press time so the release can carry the click count without
    /// depending on the run tracker's later state.
    seq: u8,
    /// One-frame edge: the press landed this frame (drives
    /// `ButtonPhase::Down`). Lowered by `drain_per_frame_queues`.
    fresh: bool,
    /// Drag latch. Sticky non-`None` for the press lifetime; doubles
    /// as "suppress click on release".
    drag: PressDrag,
}

/// Drag latch of an in-flight [`Press`]: `None` until the pointer has
/// travelled [`DRAG_THRESHOLD`] from `origin`, `Started` on exactly
/// the threshold-crossing frame (the drag-start edge),
/// `Active` after — `drain_per_frame_queues` lowers the edge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PressDrag {
    #[default]
    None,
    Started,
    Active,
}

/// One-frame edge: how this button's capture ended this frame. One
/// value instead of three parallel edge fields — a click and a
/// drag-stop are mutually exclusive by construction, and either can
/// only target the widget that was released.
#[derive(Clone, Copy, Debug)]
struct Release {
    /// The widget whose capture ended.
    target: WidgetId,
    kind: ReleaseKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReleaseKind {
    /// The release landed back on the captured widget with no drag
    /// latched — a click. `count` is the press run's number
    /// (2 = double-click, 3 = triple…), stamped from [`Press::seq`].
    Click { count: u8 },
    /// A latched drag ended — the commit edge for drag gestures.
    DragStopped,
    /// Released off the widget with no drag latched — the capture
    /// just dissolves (drives the click-less `ButtonPhase::Up`).
    Miss,
}

/// Multi-press run state: where/when/on-what the last press landed and
/// its position in the run. The next press chains (`seq + 1`) when it
/// lands on the same `target` within [`DOUBLE_CLICK_WINDOW`] of `at`
/// and [`DOUBLE_CLICK_RADIUS`] of `pos`; any break restarts at 1.
#[derive(Clone, Copy, Debug)]
struct PressRun {
    at: Duration,
    target: WidgetId,
    pos: Vec2,
    seq: u8,
}

/// Palantir-native input event. Independent of any windowing toolkit.
/// All coordinates are in **logical pixels** (DIPs). Backends are responsible
/// for any physical→logical conversion before dispatching.
#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    /// Pointer position in logical pixels, relative to the surface origin.
    PointerMoved(Vec2),
    /// Pointer left the surface; clears `hovered`.
    PointerLeft,
    PointerPressed(PointerButton),
    PointerReleased(PointerButton),
    /// Pixel-precise scroll delta — touchpad / precision wheel /
    /// `MouseScrollDelta::PixelDelta`. Logical pixels. Positive `y`
    /// means the user wants content to scroll *down* (a scroll widget
    /// should add to its vertical offset). Multiple events in one frame
    /// accumulate on the scroll target active when each event arrived.
    ScrollPixels(Vec2),
    /// Notched scroll delta — classic wheel /
    /// `MouseScrollDelta::LineDelta`. Carries the raw line count
    /// (sign-flipped to match `ScrollPixels`); the consuming widget
    /// multiplies by its own font-derived line step at record time
    /// rather than this layer baking in a constant. Multiple events
    /// in one frame accumulate on their event-time scroll targets.
    ScrollLines(Vec2),
    /// Multiplicative zoom factor from a touch / touchpad pinch gesture.
    /// `1.0` is identity; `1.05` zooms in 5%, `0.95` zooms out 5%.
    /// Multiple events in one frame multiply into their event-time
    /// pinch targets' zoom totals. Wheel-based zoom is *not*
    /// translated into `Zoom` — the active scroll widget decides at
    /// record time whether wheel ticks count as pan or zoom. Non-positive
    /// and non-finite factors are discarded at ingress.
    Zoom(f32),
    /// Logical key was pressed. `repeat` reflects OS-level key repeat
    /// (held keys re-emit). Modifier state isn't carried on the event;
    /// consumers read the latest [`Modifiers`] from `InputState`. We
    /// don't carry releases — no consumer needs them yet.
    KeyDown {
        key: Key,
        repeat: bool,
        /// Layout-independent physical key — see
        /// [`KeyPress::physical`](crate::input::keyboard::KeyPress::physical).
        physical: Key,
    },
    /// Committed text — a typed character or an IME composition that
    /// just finalized. Distinct from `KeyDown` because IME / dead-key
    /// composition produces text without a physical keypress, and
    /// because keys like `Enter` produce a logical key but no text we
    /// want to insert. Editors should consume `Text` for character
    /// input and `KeyDown` for navigation/control keys.
    Text(TextChunk),
    /// Modifier-key set changed. The carried snapshot is the new state
    /// (not a delta). Consumers track the latest snapshot to disambiguate
    /// e.g. ctrl+'a' (shortcut) from 'a' (text).
    ModifiersChanged(Modifiers),
}

/// What one event asks of the frame, as decided by the [`on_input`
/// arm](InputState::on_input) that handled it. Both answers travel
/// together because every arm has to give both, and a side-effect
/// assignment for one of them made it easy to give only the other.
#[derive(Clone, Copy, Debug, Default)]
struct EventOutcome {
    /// The event could change what is on screen, so the next frame
    /// cannot stay on the paint-anim-only path. Surfaced to the host as
    /// [`InputDelta::requests_repaint`].
    repaint: bool,
    /// The event wrote state that a widget recorded *earlier in the same
    /// pass* may already have read, so the pass has to run again.
    ///
    /// Set by: a `Click` or `DragStopped` release, a `KeyDown` or `Text`
    /// (both land in the keyboard queue), a drag latch crossing its
    /// threshold during a move, and any event a `PointerWake::BUTTONS`
    /// subscriber saw.
    ///
    /// Deliberately clear — though each still repaints: a **press**,
    /// because a capture reaches only its own target and `focused` is
    /// read live off `InputState`, committed before the frame, so pass A
    /// already sees it; a **`ReleaseKind::Miss`**, which fires no click
    /// and only tears down that same single-reader capture; and scroll,
    /// pinch, `PointerLeft` or modifier changes, whose state reaches
    /// exactly one routed target that applies it in the pass that
    /// receives it. An unrouted event leaves this clear because no
    /// widget or watcher can observe it.
    ///
    /// The press and `Miss` exclusions are what let a click-driven UI
    /// settle once per gesture instead of three times. The cost is
    /// narrow: an app that reacts to a press-driven focus change by
    /// writing state a *prefix* widget shows gains a one-frame lag, and
    /// should handle that edge in [`crate::App::update`] instead.
    settles: bool,
}

impl EventOutcome {
    /// Repaints, but does not force a second record pass — the common
    /// case, and the one whose reasoning is on [`Self::settles`].
    #[inline]
    fn repaint(repaint: bool) -> Self {
        Self {
            repaint,
            settles: false,
        }
    }

    /// Repaints and settles together. Every arm that settles also
    /// repaints, so no arm needs to state the two separately.
    #[inline]
    fn settle(both: bool) -> Self {
        Self {
            repaint: both,
            settles: both,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TargetScrollDelta {
    target: WidgetId,
    delta: ScrollDelta,
}

impl TargetScrollDelta {
    fn new(target: WidgetId) -> Self {
        Self {
            target,
            delta: ScrollDelta::default(),
        }
    }
}

/// Live input state machine: the things that survive across input events
/// independently of whether the tree was rebuilt. Per-frame rebuilt data
/// (last-frame rects, cascade scratch) lives in [`crate::scene::cascade::Cascade`].
#[derive(Debug)]
pub(crate) struct InputState {
    /// Pointer position in logical pixels, `None` when off-surface.
    pub(crate) pointer_pos: Option<Vec2>,
    pub(crate) hovered: Option<WidgetId>,
    /// Topmost `Sense::SCROLL` widget under the pointer, recomputed
    /// whenever the pointer moves and at `end_frame`. New scroll events
    /// are attributed to this id when they arrive.
    pub(crate) scroll_target: Option<WidgetId>,
    /// Topmost `Sense::PINCH` widget under the pointer, recomputed
    /// alongside `scroll_target`. Pinch zoom factors route to this id
    /// instead of `scroll_target` so a widget can opt into pan-via-
    /// scroll *without* committing to pinch zoom (and vice versa).
    pub(crate) pinch_target: Option<WidgetId>,
    /// Pixel, line, and pinch deltas accumulated by their event-time
    /// target. One row per touched [`WidgetId`]; capacity is retained
    /// when the rows are cleared in [`Self::drain_per_frame_queues`].
    frame_target_deltas: Vec<TargetScrollDelta>,
    /// Per-button press capture (active widget, press pos, drag latch,
    /// frame edges for `drag_started` and `clicked`). Indexed by
    /// [`PointerButton`] via [`PointerButton::idx`]. Independent per
    /// button — a left-drag in progress doesn't block a right-click.
    captures: [Capture; PointerButton::COUNT],
    /// Frame-snapshot of "no widget can hold any non-default interaction
    /// state this frame" — no pointer on the surface, no routed
    /// scroll/pinch target or pending target delta, no live button
    /// capture or click/double-click edge. Filled once per record pass via
    /// [`Self::snapshot_frame_quiescent`];
    /// read in [`Self::response_for`] to default the whole interaction
    /// half out for every widget instead of re-deriving it per call.
    /// `focused` is excluded on purpose (see `snapshot_frame_quiescent`),
    /// so the fast path still reads it live.
    frame_quiescent: bool,
    /// Unified keyboard event stream this frame:
    /// [`KeyboardEvent::Down`] from `KeyDown` events and
    /// [`KeyboardEvent::Text`] from `Text` events, in arrival order.
    /// Capacity-retained; cleared in [`Self::drain_per_frame_queues`].
    /// Focused/global readers see this only without popup capture; the
    /// active popup reads it through its scoped capture id.
    frame_keyboard_events: Vec<KeyboardEvent>,
    /// Latest modifier-key snapshot. Persists across `end_frame` —
    /// modifier *state* is not a per-frame thing the way keystrokes
    /// are. Updated only on `ModifiersChanged` events.
    pub(crate) modifiers: Modifiers,
    /// Currently focused widget, or `None`. Set on `PointerPressed(Left)`
    /// when the press lands on a focusable widget. Evicted in
    /// [`Self::end_frame`] when the focused widget vanishes from the
    /// tree (matches the per-id state map's eviction model). Read by
    /// keyboard consumers to decide whether to drain
    /// `frame_keyboard_events`.
    pub(crate) focused: Option<WidgetId>,
    /// This pass's scope routing — who owns which key class, and which
    /// layers are cut off. Resolved once per record pass; see
    /// [`Scopes`].
    scopes: Scopes,
    /// Press-on-non-focusable-widget behavior. See [`FocusPolicy`].
    pub(crate) focus_policy: FocusPolicy,
    /// Whether any event this record pass wrote state that an
    /// earlier-recorded widget could already have read — see
    /// [`EventOutcome::settles`], which is where each arm decides.
    /// Folded from the arms once per `on_input`; taken by
    /// [`Self::finish_record`] so `Ui::frame` can re-record the pass.
    frame_had_action: bool,
    /// Strongest input seen since the last frame, thresholded by
    /// [`InputPolicy`](crate::input::policy::InputPolicy) in
    /// `FrameRuntime::take_frame_plan`. Cleared with the per-frame event
    /// queues.
    pub(crate) signal_since_last_frame: InputSignal,
    /// Wake-gate watches ([`PointerWake`] / [`KeyboardWake`]
    /// flag masks + specific-chord list). Cleared pre-record (in
    /// `FrameCycle::record_pass`); widgets re-assert each active frame. The
    /// masks **persist across silent frames** — that's the wake
    /// signal a dormant popup needs to be paged in by the next click.
    /// `on_input` short-circuits on the masks before touching event
    /// buffers, so idle frames pay nothing.
    subs: Watches,
    /// Unified pointer event stream this frame: moves, presses,
    /// releases, scrolls, zooms, leave. Pushes are gated per-category
    /// on [`Watches::pointer_mask`] (`MOVE` for `Move`,
    /// `BUTTONS` for `Down`/`Up`, `SCROLL` for `Scroll`/`Zoom`, any
    /// pointer flag for `Leave`) — idle frames pay nothing. Cleared
    /// in [`Self::drain_per_frame_queues`]. Read through
    /// [`Self::pointer_events`], which layer-gates it against
    /// [`Self::silenced`].
    pub(crate) frame_pointer_events: Vec<PointerEvent>,
    /// Frame-runtime clock as of the last `Ui::frame`, refreshed
    /// once per frame so input handlers running *between* frames stamp
    /// events on the same deterministic clock the rest of the crate uses
    /// (vs wall-clock `Instant`). Drives double-click timing.
    pub(crate) frame_time: Duration,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            pointer_pos: None,
            hovered: None,
            scroll_target: None,
            pinch_target: None,
            frame_target_deltas: Vec::new(),
            captures: [Capture::default(); PointerButton::COUNT],
            // Recomputed each record pass before any `response_for`
            // call; `false` is the safe pre-frame default (forces the
            // full path).
            frame_quiescent: false,
            frame_keyboard_events: Vec::new(),
            modifiers: Modifiers::NONE,
            focused: None,
            scopes: Scopes::default(),
            focus_policy: FocusPolicy::default(),
            frame_had_action: false,
            signal_since_last_frame: InputSignal::None,
            subs: Watches::default(),
            frame_pointer_events: Vec::new(),
            frame_time: Duration::ZERO,
        }
    }
}

impl InputState {
    /// Start a record pass: drop last pass's watches and resolve this
    /// pass's scope path.
    ///
    /// Resolution happens **here**, once, rather than live per read.
    /// Focus is already committed by this point, and a path fixed for
    /// the whole pass is what keeps grants independent of where in the
    /// pass anything recorded — the same argument the claim resolution
    /// it replaced made for deferring to end-of-pass.
    pub(crate) fn begin_record(&mut self, cascade: &Cascade) {
        self.subs.clear();
        self.scopes.resolve(self.focused, cascade);
        self.snapshot_frame_quiescent();
    }

    pub(crate) fn watch_pointer(&mut self, flags: PointerWake) {
        self.subs.pointer_mask |= flags;
    }

    pub(crate) fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.subs.keyboard_mask |= flags;
    }

    pub(crate) fn watch_key(&mut self, shortcut: Shortcut) {
        self.subs.watch_key(shortcut);
    }

    /// The raw keyboard stream as seen from `reader`'s layer.
    ///
    /// **Layer-gated only, never class-filtered.** A scope's filter
    /// decides who a *chord* is granted to ([`Self::key_pressed`]); this
    /// is the wholesale drain a focused editor reads, and it returns a
    /// borrowed slice of the frame buffer — partitioning it by class
    /// would break the arrival order that drain depends on. The only
    /// wholesale drainer is the scope holder itself.
    pub(crate) fn keyboard_events(&self, reader: Layer) -> &[KeyboardEvent] {
        if self.silenced(reader) {
            return &[];
        }
        &self.frame_keyboard_events
    }

    /// Whether an overlay's scope cuts `reader`'s layer off both
    /// streams. Strictly-below, so the scope's own body keeps reading —
    /// a `TextEdit` inside a `Popup` drains this stream and would
    /// otherwise get nothing.
    fn silenced(&self, reader: Layer) -> bool {
        self.scopes.silences(reader)
    }

    /// The pointer watch stream as seen from `reader`'s layer, gated the
    /// same way [`Self::keyboard_events`] gates keys: an overlay's scope
    /// silences only readers *strictly below* its own layer, so the
    /// scope's own body keeps watching while everything beneath it is
    /// cut off.
    ///
    /// Watches bypass hit-testing by design — that is what makes them
    /// useful for gestures with no widget under the pointer — so the
    /// scrim that stops routed input at the hit index does nothing here.
    /// Without this gate a `Main`-layer `SCROLL` watcher kept receiving
    /// every event under an open modal, which is exactly the graph
    /// canvas that pans and zooms that `Sense::ABSORB_POINTER` is named
    /// for.
    pub(crate) fn pointer_events(&self, reader: Layer) -> &[PointerEvent] {
        if self.silenced(reader) {
            return &[];
        }
        &self.frame_pointer_events
    }

    /// Whether `shortcut` was pressed **and granted to the scope**
    /// `parent` sits in. `parent` is the record position asking — the
    /// most recently opened node — so a read inside a focused editor and
    /// a read at the app root get different answers for the same chord.
    pub(crate) fn key_pressed(
        &mut self,
        reader: Layer,
        parent: Option<WidgetId>,
        cascade: &Cascade,
        shortcut: Shortcut,
    ) -> bool {
        self.subs.watch_key(shortcut);
        // Before resolving anything: on a frame with no keys — nearly
        // every frame — an app polling its whole chord table pays one
        // subscription push and this check, and never touches the
        // cascade.
        if self.frame_keyboard_events.is_empty() || self.silenced(reader) {
            return false;
        }
        // `None` on both sides is the no-scopes-anywhere case: an app that
        // declares none reads every chord, exactly as before scopes existed.
        let scope = self.scopes.reader(parent, cascade);
        self.frame_keyboard_events.iter().any(|event| {
            matches!(event, KeyboardEvent::Down(press)
                if shortcut.matches(*press) && self.scopes.grant(KeyClass::of(*press)) == scope)
        })
    }

    /// Withdraw `owner`'s scope from the next resolution — see
    /// [`Scopes::close`] for the span that covers.
    pub(crate) fn close_scope(&mut self, owner: WidgetId) {
        self.scopes.close(owner);
    }

    /// Close out the pass. Ownership resolution moved to
    /// [`Self::begin_record`] when claims became scopes — a scope path
    /// derives from focus and the cascade, so there is nothing left to
    /// commit here.
    pub(crate) fn finish_record(&mut self) -> bool {
        self.take_action_flag()
    }

    fn target_scroll_delta(&self, target: WidgetId) -> Option<&ScrollDelta> {
        self.frame_target_deltas
            .iter()
            .find(|deltas| deltas.target == target)
            .map(|deltas| &deltas.delta)
    }

    fn target_scroll_delta_mut(&mut self, target: WidgetId) -> &mut ScrollDelta {
        if let Some(index) = self
            .frame_target_deltas
            .iter()
            .position(|deltas| deltas.target == target)
        {
            return &mut self.frame_target_deltas[index].delta;
        }
        self.frame_target_deltas
            .push(TargetScrollDelta::new(target));
        &mut self.frame_target_deltas.last_mut().unwrap().delta
    }

    #[inline]
    /// Every edge the pointer produced this frame, widget by widget.
    ///
    /// Reads the same capture state [`Self::response_for`] does, so the two
    /// cannot disagree about what happened — this walks the buttons and reports
    /// what each one's capture says, where `response_for` asks one widget
    /// whether any of it was about them.
    ///
    /// Three slots per button and at most two filled: a press frame can also
    /// cross the drag threshold, and a release frame has no press left to
    /// report. Nothing is allocated — the slots are an array.
    pub(crate) fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_ {
        PointerButton::all().flat_map(move |button| {
            let cap = self.capture(button);
            // Each edge is built where its target is already in hand, rather
            // than recovered afterwards from which variant it turned out to be:
            // a new variant would have had to be remembered in that lookup or
            // silently lose its widget.
            let of = move |id, edge| PointerAction { id, button, edge };
            let press = cap.press.as_ref();
            let pressed = press
                .filter(|press| press.fresh)
                .map(|press| of(press.target, PointerEdge::Pressed { count: press.seq }));
            let dragging = press
                .filter(|press| press.drag == PressDrag::Started)
                .map(|press| of(press.target, PointerEdge::DragStarted));
            // A release destroys the press, so this is the other frame: never
            // both, which is why one array covers either.
            let ended = cap.release.as_ref().and_then(|release| {
                let edge = match release.kind {
                    ReleaseKind::Click { count } => PointerEdge::Clicked { count },
                    ReleaseKind::DragStopped => PointerEdge::DragStopped,
                    // A release that landed off its widget ended nothing anyone
                    // asked about — the capture simply dissolves.
                    ReleaseKind::Miss => return None,
                };
                Some(of(release.target, edge))
            });
            [pressed, dragging, ended].into_iter().flatten()
        })
    }

    fn capture(&self, b: PointerButton) -> &Capture {
        &self.captures[b.idx()]
    }

    #[inline]
    fn capture_mut(&mut self, b: PointerButton) -> &mut Capture {
        &mut self.captures[b.idx()]
    }

    /// Push a pointer event to [`Self::frame_pointer_events`] and
    /// answer "should this event wake the next frame?" Wake fires
    /// when any watcher holds `sense` — single bitwise AND on the
    /// cached `pointer_mask`. Returns `true` even when `pos` is `None`
    /// so an off-surface press still wakes; the `PointerEvent` itself
    /// is only pushed if there's a position (no consumer can do
    /// anything useful without one).
    fn push_pointer_event(
        &mut self,
        sense: PointerWake,
        pos: Option<Vec2>,
        make: impl FnOnce(Vec2) -> PointerEvent,
    ) -> bool {
        if !self.subs.pointer_mask.contains(sense) {
            return false;
        }
        if let Some(pos) = pos {
            self.frame_pointer_events.push(make(pos));
        }
        true
    }

    /// Push for the events that route *by pointer position* — scroll and
    /// pinch. Their wake additionally requires a pointer on the surface:
    /// with none, they route nowhere, so waking would be pointless.
    fn push_positioned(
        &mut self,
        wake: PointerWake,
        make: impl FnOnce(Vec2) -> PointerEvent,
    ) -> bool {
        self.pointer_pos.is_some() && self.push_pointer_event(wake, self.pointer_pos, make)
    }

    /// Feed an palantir-native input event. Hit-tests against the
    /// frozen `Cascade` from this frame's most recent run. Returns an
    /// [`InputDelta`] hosts use to decide whether to request a redraw —
    /// a `PointerMoved` over a non-hover-reactive surface (no active
    /// capture, no hover/scroll target change) leaves
    /// `requests_repaint` false so the frame can be skipped entirely.
    pub(crate) fn on_input(&mut self, event: InputEvent, cascade: &Cascade) -> InputDelta {
        if let InputEvent::Zoom(factor) = event
            && !zoom::is_valid(factor)
        {
            return InputDelta::default();
        }
        // Any host-pushed event that survived the guard above
        // disqualifies the next frame from the paint-anim-only
        // short-circuit — the recording closure might observe even a
        // pointer move (hover styling) or modifier change (shortcut
        // hint). A rejected zoom returned before this on purpose: it
        // mutates nothing, so there is nothing for the closure to
        // observe. Cleared at the top of `frame` after the gate has read
        // it.
        // Any host-pushed event that survived the guard is at least
        // `Inert`; the arms below raise it to `Repaint` by returning
        // `repaint: true`.
        self.signal_since_last_frame.raise(InputSignal::Inert);
        let outcome = match event {
            InputEvent::PointerMoved(p) => {
                let prev_hover = self.hovered;
                let prev_scroll = self.scroll_target;
                let prev_pinch = self.pinch_target;
                self.pointer_pos = Some(p);
                // Drag-latch check per button. Every captured button
                // independently latches once travel crosses
                // `DRAG_THRESHOLD`. Right-drag latching just suppresses
                // the click (same as left), so a slow right-press that
                // wiggles no longer pops a context menu — consistent
                // with click-suppression semantics.
                let mut latched = false;
                for cap in &mut self.captures {
                    if let Some(press) = &mut cap.press
                        && press.drag == PressDrag::None
                        && p.distance_squared(press.origin) >= DRAG_THRESHOLD * DRAG_THRESHOLD
                    {
                        press.drag = PressDrag::Started;
                        latched = true;
                    }
                }
                self.refresh_pointer_targets(cascade);
                let move_subbed =
                    self.push_pointer_event(PointerWake::MOVE, Some(p), PointerEvent::Move);
                EventOutcome {
                    repaint: self.hovered != prev_hover
                        || self.scroll_target != prev_scroll
                        || self.pinch_target != prev_pinch
                        || self.captures.iter().any(|c| c.press.is_some())
                        || move_subbed,
                    // Only the threshold crossing settles: the latch is
                    // what a widget reads, and it flips exactly once.
                    settles: latched,
                }
            }
            InputEvent::PointerLeft => {
                let observable = self.hovered.is_some()
                    || self.scroll_target.is_some()
                    || self.pinch_target.is_some()
                    || self.captures.iter().any(|c| c.press.is_some());
                self.pointer_pos = None;
                self.refresh_pointer_targets(cascade);
                // `Leave` is rare; emit whenever any pointer-class
                // watch is active so watchers can clean up
                // (clear crosshair, dismiss hover preview).
                let pointer_subbed = !self.subs.pointer_mask.is_empty();
                if pointer_subbed {
                    self.frame_pointer_events.push(PointerEvent::Leave);
                }
                EventOutcome::repaint(observable || pointer_subbed)
            }
            InputEvent::PointerPressed(btn) => {
                // Hit-test for the press target (the topmost *clickable*
                // widget under the pointer). Hover-only widgets are
                // transparent to presses even though they show as hovered.
                let pointer_pos = self.pointer_pos;
                // One walk for both answers: the press target and the focus
                // target are independent filters over the same hit table.
                let targets = pointer_pos.map(|p| cascade.hit_test_press(p));
                let hit = targets.and_then(|t| t.click);
                let buttons_subbed =
                    self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
                        PointerEvent::Down { pos, button: btn }
                    });
                // Frame clock for multi-press timing — read before the
                // `capture_mut` borrow.
                let now = self.frame_time;
                let cap = self.capture_mut(btn);
                match hit.zip(pointer_pos) {
                    Some((target, pos)) => cap.begin_press(target, pos, now),
                    // A missed press clears any stale capture and
                    // leaves the run alone.
                    None => cap.press = None,
                }
                // Focus updates on a separate hit-test on the *left*
                // button only — right/middle clicks shouldn't steal
                // focus from a TextEdit. Focusability is orthogonal to
                // clickability (clicking a Button shouldn't steal focus
                // from a TextEdit either, hence the separate test).
                let prev_focus = self.focused;
                if btn == PointerButton::Left {
                    match (targets.and_then(|t| t.focus), self.focus_policy) {
                        (Some(id), _) => self.focused = Some(id),
                        (None, FocusPolicy::ClearOnMiss) => self.focused = None,
                        (None, FocusPolicy::PreserveOnMiss) => {}
                    }
                }
                // Press on inert surface (no click target, no focus
                // change, no `BUTTONS` watcher) is observably
                // a no-op — under `OnDelta` the frame stays on the
                // paint-anim path. Focus-clearing clicks (outside a
                // focused TextEdit) and any sense hit still record;
                // popup-dismiss watchers wake themselves.
                EventOutcome {
                    repaint: hit.is_some() || self.focused != prev_focus || buttons_subbed,
                    // Narrower than `repaint`: a press records whenever
                    // it lands, but a `BUTTONS` subscriber is the only
                    // channel it writes that an earlier widget could
                    // have read.
                    settles: buttons_subbed,
                }
            }
            InputEvent::PointerReleased(btn) => {
                let pointer_pos = self.pointer_pos;
                let cap = self.capture_mut(btn);
                // A captureless release (the press missed every widget)
                // has no press to take and touches nothing — an earlier
                // same-batch gesture's release edge survives it.
                let released = cap.press.take();
                // A `Miss` only tears down a capture that exactly one
                // widget reads, which is precisely what this module's
                // settle rule excludes. A `Click` or `DragStopped` is the
                // edge apps act on — dropping a graph node rewires things
                // a prefix widget draws — so those keep their settle.
                let mut settles = false;
                if let Some(press) = released {
                    // A latched drag ending is its own edge (the release
                    // just destroyed the drag, so widgets can't infer it);
                    // otherwise a release back on the widget is a click
                    // carrying its press's run number — double-click is
                    // simply "the click whose press was #2 in the run".
                    let kind = if press.drag != PressDrag::None {
                        ReleaseKind::DragStopped
                    } else {
                        let hit = pointer_pos.and_then(|p| cascade.hit_test(p, Sense::clicks));
                        if hit == Some(press.target) {
                            ReleaseKind::Click { count: press.seq }
                        } else {
                            ReleaseKind::Miss
                        }
                    };
                    settles = !matches!(kind, ReleaseKind::Miss);
                    cap.release = Some(Release {
                        target: press.target,
                        kind,
                    });
                }
                let buttons_subbed =
                    self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
                        PointerEvent::Up { pos, button: btn }
                    });
                EventOutcome {
                    // Capture was live ⇒ owning widget needs a record;
                    // otherwise only `BUTTONS` watchers wake.
                    repaint: released.is_some() || buttons_subbed,
                    settles: settles || buttons_subbed,
                }
            }
            InputEvent::ScrollPixels(d) => {
                let target = self.scroll_target;
                if let Some(target) = target {
                    self.target_scroll_delta_mut(target).pixels += d;
                }
                let subbed =
                    self.push_positioned(PointerWake::SCROLL, |pos| PointerEvent::Scroll {
                        pos,
                        pixels: d,
                        lines: Vec2::ZERO,
                    });
                EventOutcome::repaint(target.is_some() || subbed)
            }
            InputEvent::ScrollLines(d) => {
                let target = self.scroll_target;
                if let Some(target) = target {
                    self.target_scroll_delta_mut(target).lines += d;
                }
                let subbed =
                    self.push_positioned(PointerWake::SCROLL, |pos| PointerEvent::Scroll {
                        pos,
                        pixels: Vec2::ZERO,
                        lines: d,
                    });
                EventOutcome::repaint(target.is_some() || subbed)
            }
            InputEvent::Zoom(f) => {
                let target = self.pinch_target;
                if let Some(target) = target {
                    let delta = self.target_scroll_delta_mut(target);
                    delta.zoom = zoom::combine(delta.zoom, f);
                }
                let subbed = self.push_positioned(PointerWake::PINCH, |pos| PointerEvent::Zoom {
                    pos,
                    factor: f,
                });
                EventOutcome::repaint(target.is_some() || subbed)
            }
            InputEvent::KeyDown {
                key,
                repeat,
                physical,
            } => {
                let kp = KeyPress {
                    key,
                    mods: self.modifiers,
                    repeat,
                    physical,
                };
                // Wake when a focused widget would consume the key
                // OR a specific-chord watcher asked for it
                // OR a `KeyboardWake::KEY` watcher is recording
                // raw key events. Idle keys with none of those
                // (typing into empty surface) skip the frame. The
                // chord check takes the whole `KeyPress` so the
                // non-Latin layout fallback applies — an off-focus
                // Cmd+Z still wakes on a Russian layout.
                let observable = self.focused.is_some()
                    || self.subs.matches_press(kp)
                    || self.subs.keyboard_mask.contains(KeyboardWake::KEY);
                if observable {
                    self.frame_keyboard_events.push(KeyboardEvent::Down(kp));
                }
                EventOutcome::settle(observable)
            }
            InputEvent::Text(chunk) => {
                // Text is rare (only fires on IME commit / dead-key
                // resolution on most platforms). Wake when a focused
                // widget would consume it OR a TEXT watcher wants
                // it.
                let observable =
                    self.focused.is_some() || self.subs.keyboard_mask.contains(KeyboardWake::TEXT);
                if observable {
                    self.frame_keyboard_events.push(KeyboardEvent::Text(chunk));
                }
                EventOutcome::settle(observable)
            }
            InputEvent::ModifiersChanged(m) => {
                self.modifiers = m;
                // Only wake if a watcher asked. Accel-underline
                // UIs / modifier debug overlays must watch to
                // `MODIFIER`; nothing else cares.
                EventOutcome::repaint(self.subs.keyboard_mask.contains(KeyboardWake::MODIFIER))
            }
        };
        if outcome.repaint {
            self.signal_since_last_frame.raise(InputSignal::Repaint);
        }
        self.frame_had_action |= outcome.settles;
        InputDelta {
            requests_repaint: outcome.repaint,
        }
    }

    /// Read and reset [`Self::frame_had_action`]. Called by
    /// [`crate::Ui::frame`] to decide whether to run a discarded
    /// pre-pass for state-mutation settling.
    fn take_action_flag(&mut self) -> bool {
        std::mem::take(&mut self.frame_had_action)
    }

    /// Drain the per-frame input queues without touching cascade-
    /// dependent state (active/focused eviction, hover recompute).
    /// Used by [`crate::Ui::frame`] for the discarded pass — pass
    /// 2's recording must see empty queues so `Response::clicked()`
    /// returns `false` everywhere and clicks aren't double-fired.
    /// Capacity-retained on the backing buffers.
    pub(crate) fn drain_per_frame_queues(&mut self) {
        for cap in &mut self.captures {
            cap.release = None;
            if let Some(press) = &mut cap.press {
                press.fresh = false;
                if press.drag == PressDrag::Started {
                    press.drag = PressDrag::Active;
                }
            }
        }
        self.signal_since_last_frame = InputSignal::None;
        self.frame_had_action = false;
        self.frame_pointer_events.clear();
        self.frame_target_deltas.clear();
        self.frame_keyboard_events.clear();
    }

    /// Re-resolve `hovered` / `scroll_target` / `pinch_target` against
    /// `cascade` using the current `pointer_pos` — the single owner of
    /// the target-triple assignment (the `PointerMoved` / `PointerLeft`
    /// arms, `end_frame`, and the cold-start warmup all route through
    /// it). The warmup case: pre-frame-1 input events arrived with an
    /// empty cascade so their hit-tests resolved to nothing; after the
    /// warmup record pass has built a real cascade, `Ui::frame` calls
    /// this to route the held pointer position onto the right widgets
    /// before the user-visible record pass runs — so hover styling on
    /// frame 1 reflects the actual content under the cursor.
    pub(crate) fn refresh_pointer_targets(&mut self, cascade: &Cascade) {
        if let Some(p) = self.pointer_pos {
            let hits = cascade.hit_test_targets(p, Sense::hovers, Sense::scrolls, Sense::pinches);
            self.hovered = hits.hover;
            self.scroll_target = hits.scroll;
            self.pinch_target = hits.pinch;
        } else {
            self.hovered = None;
            self.scroll_target = None;
            self.pinch_target = None;
        }
    }

    /// Once-per-frame close-out (from `FrameCycle::finalize_frame`, after the
    /// final record pass): recompute hover, drop transient per-frame
    /// flags, evict captured widgets that disappeared from the tree.
    /// Call after `CascadeEngine::run` (whose result `cascade` is
    /// passed here).
    pub(crate) fn end_frame(&mut self, cascade: &Cascade) {
        self.drain_per_frame_queues();
        self.scopes.end_frame();
        // `modifiers` deliberately persists: modifier state is a running
        // snapshot, not per-frame. Held shift across multiple frames must
        // stay `true`.
        for cap in &mut self.captures {
            if let Some(press) = &cap.press
                && !cascade.by_id.contains_key(&press.target)
            {
                cap.press = None;
            }
        }
        // Focus eviction: same model as the per-button capture eviction
        // above. A focused widget that vanished from the tree drops
        // focus to None; otherwise next frame's keystrokes route to a
        // ghost.
        if let Some(focused) = self.focused
            && !cascade.by_id.contains_key(&focused)
        {
            self.focused = None;
        }
        self.refresh_pointer_targets(cascade);
    }

    /// Returns the raw scroll and pinch deltas attributed to `id` when
    /// their events arrived. Widget policy decides how line deltas map
    /// to pixels and whether modifiers turn wheel input into zoom.
    pub(crate) fn scroll_delta_for(&self, id: WidgetId) -> ScrollDelta {
        self.target_scroll_delta(id).copied().unwrap_or_default()
    }

    /// Snapshot into [`Self::frame_quiescent`] whether any widget can
    /// hold non-default interaction state this frame: no pointer on the
    /// surface, no routed scroll/pinch target or pending event-time
    /// delta, and no live button capture or per-frame click/double-click
    /// edge. Taken once per record pass so [`Self::response_for`] can
    /// default the interaction half out for every widget at once.
    ///
    /// `focused` is deliberately *not* part of this: [`crate::Ui::request_focus`]
    /// can set it mid-record, after the snapshot is taken, so
    /// `response_for` always reads it live — even on the fast path.
    fn snapshot_frame_quiescent(&mut self) {
        self.frame_quiescent = self.pointer_pos.is_none()
            && self.hovered.is_none()
            && self.scroll_target.is_none()
            && self.pinch_target.is_none()
            && self.frame_target_deltas.is_empty()
            && self
                .captures
                .iter()
                .all(|c| c.press.is_none() && c.release.is_none());
    }

    /// The pointer in `id`'s local space, gathered from scratch.
    ///
    /// The cheap path for a caller that wants *only* this: it does the
    /// three lookups itself rather than running the whole
    /// [`Self::response_for`] probe, which computes the same value into
    /// [`ResponseState::pointer_local`] as one field of a much larger
    /// gather. Both end at `pointer_in_widget_space`, so the arithmetic
    /// is shared even though the lookups are not.
    pub(crate) fn pointer_local_for(
        &self,
        id: WidgetId,
        cascade: &Cascade,
        layout: &Layout,
    ) -> Option<Vec2> {
        let pointer = self.pointer_pos?;
        let loc = cascade.locate(id)?;
        let layout_rect = layout.arranged_rect(loc.endpoint);
        let transform = cascade.entries[loc.entry_idx as usize].transform;
        Some(pointer_in_widget_space(pointer, layout_rect.min, transform))
    }

    pub(crate) fn response_for(
        &self,
        id: WidgetId,
        cascade: &Cascade,
        layout: &Layout,
    ) -> ResponseState {
        // Geometry half — needed every frame for theme picking and
        // layout-relative math. `locate` is the lone hash probe, and it
        // yields both the entry index and the endpoint the layout
        // columns are keyed by.
        let loc = cascade.locate(id);
        // One gather of the whole `EntryRow` — `entries` is AoS precisely
        // so these three land on one cache line instead of three.
        let entry = loc.map(|l| cascade.entries[l.entry_idx as usize]);
        let rect = entry.map(|e| e.rect);
        // The arranged rect lives on `Layout`, which owns it. Reading it
        // through the endpoint rather than from a per-node copy on the
        // cascade costs nothing in freshness — the cascade is rebuilt (or
        // provably skipped) whenever an arranged rect moves, so `layout`
        // and `cascade` always describe the same arrangement.
        let layout_rect = loc.map(|l| layout.arranged_rect(l.endpoint));
        let transform = entry.map_or(TranslateScale::IDENTITY, |e| e.transform);
        // Cascade flattens parent-disabled into each entry, so this is
        // the **effective** ancestor-or-self disabled — one frame stale.
        // Widgets that need lag-free self-toggle response merge their
        // own `node.disabled` on top after calling.
        let disabled = entry.is_some_and(|e| e.disabled);

        // Built once, here — the quiescent path returns it as-is and the
        // interaction half below assigns into it. Two constructions let a
        // newly-added field be filled on one path and silently defaulted
        // on the other.
        //
        // `focused` sits in the geometry half despite being interaction
        // state: it is read live rather than from the quiescent snapshot,
        // because `request_focus` can set it mid-record, after
        // `frame_quiescent` was taken.
        let mut state = ResponseState {
            rect,
            layout_rect,
            transform,
            disabled,
            focused: self.focused == Some(id),
            ..ResponseState::default()
        };

        // On a quiescent frame every remaining field is already at its
        // default, so skip the per-button capture scan and the
        // scroll/zoom lookups every idle widget would otherwise pay.
        if self.frame_quiescent {
            return state;
        }

        let me_under_pointer = self.hovered == Some(id);
        let left_press = self.capture(PointerButton::Left).press;
        // Hover is left-capture-gated: while some *other* widget holds
        // the left press, nothing else reads hovered.
        state.hovered = me_under_pointer && left_press.is_none_or(|p| p.target == id);

        // One uniform slice per button. Phase priority mirrors the
        // capture: a live press is `Down` (its `fresh` edge) or
        // `Held`; with no press, a release edge is `Up` — so a
        // same-batch press+release collapses to `Up{click}` (the
        // completed click outranks the lost press edge) and a
        // same-batch re-press collapses to `Down` (the live capture
        // outranks the stale release).
        // Drag exclusivity: only the priority-first latched button
        // owns the widget's drag, so at most one slot goes live.
        let mut drag_owned = false;
        for btn in PointerButton::all() {
            let cap = self.capture(btn);
            let phase = match &cap.press {
                Some(press) if press.target == id => {
                    if press.fresh {
                        ButtonPhase::Down { press: press.seq }
                    } else {
                        ButtonPhase::Held
                    }
                }
                _ => match &cap.release {
                    Some(release) if release.target == id => ButtonPhase::Up {
                        click: match release.kind {
                            ReleaseKind::Click { count } => Some(count),
                            ReleaseKind::DragStopped | ReleaseKind::Miss => None,
                        },
                    },
                    _ => ButtonPhase::Idle,
                },
            };
            let mut drag = match &cap.release {
                Some(release)
                    if release.target == id && release.kind == ReleaseKind::DragStopped =>
                {
                    Drag::Stopped
                }
                _ => Drag::None,
            };
            // A threshold-crossed press overrides the stale stop edge
            // (same-frame stop-and-relatch reports the fresh gesture).
            // Rect-independent: the pointer can leave `id`'s rect
            // mid-drag and the delta keeps tracking.
            if !drag_owned
                && let Some(pointer) = self.pointer_pos
                && let Some(press) = &cap.press
                && press.target == id
                && press.drag != PressDrag::None
            {
                let delta = transform.inverse_vector(pointer - press.origin);
                drag = if press.drag == PressDrag::Started {
                    Drag::Started { delta }
                } else {
                    Drag::Active { delta }
                };
                drag_owned = true;
            }
            *state.button_mut(btn) = ButtonState { phase, drag };
        }

        state.scroll = self.scroll_delta_for(id);
        state.pointer_local = self
            .pointer_pos
            .zip(layout_rect)
            .map(|(pointer, layout)| pointer_in_widget_space(pointer, layout.min, transform));

        state
    }
}

#[cfg(test)]
mod tests;
