//! The live input state machine: what survives across input events.

use crate::cascade::Cascade;
use crate::cascade::entry::{TabDirection, TabDomain, WidgetLocation};
use crate::common::span::Span;
use crate::input::capture::{Capture, DRAG_THRESHOLD, PressDrag, ReleaseKind};
use crate::input::event_outcome::EventOutcome;
use crate::input::ime_preedit::ImePreedit;
use crate::input::input_event::InputEvent;
use crate::input::input_queue::InputQueue;
use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::interaction::button_state::ButtonState;
use crate::input::interaction::drag::Drag;
use crate::input::interaction::input_delta::InputDelta;
use crate::input::interaction::pointer_action::PointerAction;
use crate::input::interaction::pointer_edge::PointerEdge;
use crate::input::interaction::response_state::ResponseState;
use crate::input::interaction::scroll_delta::ScrollDelta;
use crate::input::key_class::KeyClass;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::key_text::KeyText;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::policy::{FocusPolicy, InputPolicy, InputSignal};
use crate::input::scope::Scopes;
use crate::input::scroll_targets::ScrollTargets;
use crate::input::shortcut::Shortcut;
use crate::input::target_scroll_delta::TargetScrollDelta;
use crate::input::watch::{KeyboardWake, PointerWake, Watches};
use crate::input::zoom_factor::ZoomFactor;
use crate::layout::Layout;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::scene::layer::Layer;
use glam::Vec2;
use std::mem;
use std::time::Duration;

fn pointer_in_widget_space(pointer: Vec2, layout_origin: Vec2, transform: TranslateScale) -> Vec2 {
    let surface_origin = transform.apply_point(layout_origin);
    transform.inverse_vector(pointer - surface_origin)
}

/// Per-frame rebuilt data lives in [`crate::cascade::Cascade`].
#[derive(Debug, Default)]
pub(crate) struct InputState {
    pointer_pos: Option<Vec2>,
    hovered: Option<WidgetId>,
    /// Topmost scroll-sensing widgets under the pointer.
    pub(crate) scroll_targets: ScrollTargets,
    /// Topmost `Sense::PINCH` widget under the pointer; separate from scroll so a widget can pan
    /// without pinch-zooming.
    pub(crate) pinch_target: Option<WidgetId>,
    frame_target_deltas: Vec<TargetScrollDelta>,
    /// Per-button press capture, indexed by [`PointerButton::idx`].
    captures: [Capture; PointerButton::COUNT],
    /// Snapshot of "no widget can hold non-default interaction state this frame".
    /// Taken before `App::update` and per record pass so [`Self::response_for`] can
    /// skip the interaction half; `false` before the first fill. `focused` is excluded.
    frame_quiescent: bool,
    /// This frame's presses, in arrival order.
    frame_keyboard_events: Vec<KeyPress>,
    /// Persists across `end_frame`; updated only on `ModifiersChanged`.
    modifiers: Modifiers,
    /// Set on a left press that lands on a focusable widget; evicted in [`Self::end_frame`] when it
    /// leaves the tree.
    focused: Option<WidgetId>,
    /// Whether [`Self::focused`] came from the keyboard and so shows the focus ring (CSS
    /// `:focus-visible`).
    focus_visible: bool,
    /// Focus-return rows, one per overlay focus is inside.
    focus_returns: Vec<FocusReturn>,
    held_text: String,
    /// The input method's uncommitted text and its cursor; see [`Self::ime_preedit`].
    ime_preedit: String,
    ime_cursor: Option<Span>,
    /// The widget focused when the preedit arrived; a focus move retires it.
    ime_owner: Option<WidgetId>,
    /// The topmost `Modal`-layer root last frame recorded, to notice a new dialog.
    modal_root: Option<WidgetId>,
    focus_first: Option<WidgetId>,
    /// This pass's scope routing, resolved once per record pass; see [`Scopes`].
    scopes: Scopes,
    focus_policy: FocusPolicy,
    input_policy: InputPolicy,
    /// Whether any event this pass wrote state an earlier-recorded widget could have
    /// read (see [`EventOutcome::settles`]); taken by [`Self::take_action_flag`].
    frame_had_action: bool,
    signal_since_last_frame: InputSignal,
    /// Wake-gate watches, cleared pre-record. The masks persist across silent frames so a dormant
    /// popup is paged in by the next click.
    subs: Watches,
    /// Pointer events this frame, gated per category on [`Watches::pointer_mask`]; read via
    /// [`Self::pointer_events`].
    frame_pointer_events: Vec<PointerEvent>,
    queue: InputQueue,
}

#[derive(Clone, Copy, Debug)]
struct Traversal {
    domain: TabDomain,
    direction: TabDirection,
}

#[derive(Clone, Copy, Debug)]
struct FocusReturn {
    overlay: WidgetId,
    to: WidgetId,
}

impl InputState {
    /// Start a record pass: drop last pass's watches and resolve this pass's scope path once.
    pub(crate) fn pre_record(&mut self, cascade: &Cascade) {
        self.subs.clear();
        self.scopes.resolve(self.focused, cascade);
        self.traverse_focus(cascade);
        self.snapshot_frame_quiescent();
    }

    /// Move focus for each unclaimed Tab and Shift+Tab ([`KeyClass::Focus`]),
    /// removing those presses from the key stream; scopes resolve again after each
    /// move. Arrows do the same inside an arrow group
    /// ([`Configure::arrow_focus`](crate::Configure::arrow_focus)) when no scope inside
    /// the group takes them.
    fn traverse_focus(&mut self, cascade: &Cascade) {
        let mut i = 0;
        while i < self.frame_keyboard_events.len() {
            let press = self.frame_keyboard_events[i];
            let Some(Traversal { domain, direction }) = self.traversal(press, cascade) else {
                i += 1;
                continue;
            };
            if let Some(next) = cascade.next_tab_stop(domain, self.focused, direction) {
                if let TabDomain::Root(root) = domain {
                    self.enter_overlay(root, cascade);
                }
                self.focused = Some(next);
                self.focus_visible = true;
            }
            self.frame_keyboard_events.remove(i);
            self.scopes.resolve(self.focused, cascade);
        }
    }

    /// Where `press` moves focus, if the framework owns it.
    fn traversal(&self, press: KeyPress, cascade: &Cascade) -> Option<Traversal> {
        match KeyClass::of(press) {
            KeyClass::Focus if !self.scopes.path_takes(KeyClass::Focus) => {
                let direction = if press.mods.shift {
                    TabDirection::Previous
                } else {
                    TabDirection::Next
                };
                Some(Traversal {
                    domain: cascade.tab_domain(self.focused),
                    direction,
                })
            }
            KeyClass::Caret if press.mods == Modifiers::NONE => {
                let group = cascade.arrow_group_of(self.focused?)?;
                let direction = match (group.axis, press.key) {
                    (Axis::Y, Key::ArrowDown) | (Axis::X, Key::ArrowRight) => TabDirection::Next,
                    (Axis::Y, Key::ArrowUp) | (Axis::X, Key::ArrowLeft) => TabDirection::Previous,
                    _ => return None,
                };
                let claimed = self
                    .scopes
                    .path_takes_within(KeyClass::Caret, group.id, cascade);
                (!claimed).then_some(Traversal {
                    domain: TabDomain::Group(group.id),
                    direction,
                })
            }
            _ => None,
        }
    }

    /// Record where focus returns to as it enters `overlay`, unless it is already inside.
    fn enter_overlay(&mut self, overlay: WidgetId, cascade: &Cascade) {
        let Some(from) = self.focused else {
            return;
        };
        let inside = cascade.is_within(from, overlay)
            || self.focus_returns.iter().any(|row| row.overlay == overlay);
        if !inside {
            self.focus_returns.push(FocusReturn { overlay, to: from });
        }
    }

    pub(crate) const fn focus_first_within(&mut self, ancestor: WidgetId) {
        self.focus_first = Some(ancestor);
    }

    #[inline]
    pub(crate) const fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    pub(crate) fn ime_preedit(&self) -> Option<ImePreedit<'_>> {
        (!self.ime_preedit.is_empty() && self.ime_owner.is_some() && self.ime_owner == self.focused)
            .then(|| ImePreedit {
                text: &self.ime_preedit,
                cursor: self.ime_cursor,
            })
    }

    #[inline]
    pub(crate) const fn focus_visible(&self) -> bool {
        self.focus_visible
    }

    #[inline]
    pub(crate) const fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    #[inline]
    pub(crate) const fn pointer_pos(&self) -> Option<Vec2> {
        self.pointer_pos
    }

    #[inline]
    pub(crate) const fn hovered(&self) -> Option<WidgetId> {
        self.hovered
    }

    #[inline]
    pub(crate) const fn focus_policy(&self) -> FocusPolicy {
        self.focus_policy
    }

    #[inline]
    pub(crate) const fn set_focus_policy(&mut self, policy: FocusPolicy) {
        self.focus_policy = policy;
    }

    #[inline]
    pub(crate) const fn input_policy(&self) -> InputPolicy {
        self.input_policy
    }

    #[inline]
    pub(crate) const fn set_input_policy(&mut self, policy: InputPolicy) {
        self.input_policy = policy;
    }

    #[inline]
    pub(crate) const fn signal_since_last_frame(&self) -> InputSignal {
        self.signal_since_last_frame
    }

    /// Move focus. Does not re-route this pass; see [`Scopes`].
    pub(crate) const fn set_focus(&mut self, id: Option<WidgetId>) {
        self.focused = id;
    }

    pub(crate) fn release_input_scope(&mut self, owner: WidgetId) {
        self.scopes.close(owner);
    }

    /// The state a warmup pass records against: no input, but this state's focus.
    pub(crate) fn warmup_scratch(&self) -> Self {
        Self {
            focused: self.focused,
            ..Self::default()
        }
    }

    /// Keep the focus moves and scope withdrawals a warmup pass recorded.
    pub(crate) fn adopt_warmup(&mut self, warmup: &Self) {
        self.focused = warmup.focused;
        self.focus_first = warmup.focus_first.or(self.focus_first);
        self.scopes.adopt_closing(&warmup.scopes);
    }

    pub(crate) const fn watch_pointer(&mut self, flags: PointerWake) {
        self.subs.pointer_mask.insert(flags);
    }

    pub(crate) const fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.subs.keyboard_mask.insert(flags);
    }

    pub(crate) fn watch_key(&mut self, shortcut: Shortcut) {
        self.subs.watch_key(shortcut);
    }

    /// The raw keyboard stream as seen from `reader`'s layer; layer-gated only, to keep arrival
    /// order.
    pub(crate) fn keyboard_events(&self, reader: Layer) -> &[KeyPress] {
        if self.silenced(reader) {
            return &[];
        }
        &self.frame_keyboard_events
    }

    /// Whether an overlay's scope cuts `reader`'s layer off (strictly below, so its own body keeps
    /// reading).
    fn silenced(&self, reader: Layer) -> bool {
        self.scopes.silences(reader)
    }

    /// The pointer watch stream as seen from `reader`'s layer, gated like
    /// [`Self::keyboard_events`]. Watches bypass hit-testing, so without this a
    /// `Main`-layer `SCROLL` watcher would keep receiving events under a modal.
    pub(crate) fn pointer_events(&self, reader: Layer) -> &[PointerEvent] {
        if self.silenced(reader) {
            return &[];
        }
        &self.frame_pointer_events
    }

    /// Whether `shortcut` was pressed and granted to the scope `parent` sits in.
    pub(crate) fn key_pressed(
        &mut self,
        reader: Layer,
        parent: Option<WidgetId>,
        cascade: &Cascade,
        shortcut: Shortcut,
    ) -> bool {
        self.subs.watch_key(shortcut);
        if self.frame_keyboard_events.is_empty() || self.silenced(reader) {
            return false;
        }
        let scope = self.scopes.reader(parent, cascade);
        self.frame_keyboard_events.iter().any(|press| {
            shortcut.matches(*press) && self.scopes.grant(KeyClass::of(*press)) == scope
        })
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
    /// Every edge the pointer produced this frame, widget by widget, from the same
    /// capture state as [`Self::response_for`]. Three slots per button, at most two
    /// filled (`InputQueue` admits one press-or-release per button per frame).
    pub(crate) fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_ {
        PointerButton::ALL.into_iter().flat_map(move |button| {
            let cap = self.capture(button);
            let of = move |id, edge| PointerAction { id, button, edge };
            let press = cap.press.as_ref();
            let pressed = press
                .filter(|press| press.fresh)
                .map(|press| of(press.target, PointerEdge::Pressed { count: press.count }));
            let dragging = press
                .filter(|press| press.drag == PressDrag::Started)
                .map(|press| of(press.target, PointerEdge::DragStarted));
            // A release destroys the press and `InputQueue` holds a new press for the next frame,
            // so never both.
            let ended = cap.release.as_ref().and_then(|release| {
                let edge = match release.kind.click() {
                    Some(count) => PointerEdge::Clicked { count },
                    None if release.kind.ended_drag() => PointerEdge::DragStopped,
                    None => return None,
                };
                Some(of(release.target, edge))
            });
            [pressed, dragging, ended].into_iter().flatten()
        })
    }

    const fn capture(&self, b: PointerButton) -> &Capture {
        &self.captures[b.idx()]
    }

    #[inline]
    const fn capture_mut(&mut self, b: PointerButton) -> &mut Capture {
        &mut self.captures[b.idx()]
    }

    /// Push a pointer event to [`Self::frame_pointer_events`]; returns whether it
    /// should wake the next frame, even with no `pos` (an off-surface press still
    /// wakes). The event is only pushed when there is a position.
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

    /// Push `Leave`, which belongs to no single watch class and carries no position.
    fn push_unclassed(&mut self, event: PointerEvent) -> bool {
        if self.subs.pointer_mask.is_empty() {
            return false;
        }
        self.frame_pointer_events.push(event);
        true
    }

    /// Whether any button holds a live capture; a captured widget tracks the pointer wherever it
    /// is.
    fn any_press(&self) -> bool {
        self.captures.iter().any(|c| c.press.is_some())
    }

    /// Accumulate one scroll delta on the current targets and wake watchers. Pixels and lines
    /// travel in separate lanes (see [`ScrollDelta`]).
    fn on_scroll(&mut self, pixels: Vec2, lines: Vec2) -> EventOutcome {
        let mut delivered = false;
        for share in self
            .scroll_targets
            .route(pixels, lines)
            .into_iter()
            .flatten()
        {
            let delta = self.target_scroll_delta_mut(share.target);
            delta.pixels += share.pixels;
            delta.lines += share.lines;
            delivered = true;
        }
        let subbed = self.push_positioned(PointerWake::SCROLL, |pos| PointerEvent::Scroll {
            pos,
            pixels,
            lines,
        });
        EventOutcome::repaint(delivered || subbed)
    }

    /// Push for position-routed events (scroll, pinch); they wake only with a pointer on the
    /// surface.
    fn push_positioned(
        &mut self,
        wake: PointerWake,
        make: impl FnOnce(Vec2) -> PointerEvent,
    ) -> bool {
        self.pointer_pos.is_some() && self.push_pointer_event(wake, self.pointer_pos, make)
    }

    /// Feed a palantir-native input event, hit-tested against this frame's latest
    /// cascade. `now` is the arrival time on the host's clock (see
    /// [`Ui::on_input`](crate::Ui::on_input)).
    pub(crate) fn on_input(
        &mut self,
        event: InputEvent<'_>,
        cascade: &Cascade,
        now: Duration,
    ) -> InputDelta {
        if !event.is_valid() {
            return InputDelta::default();
        }
        // A press of a still-held button means its release was lost; synthesize the release first.
        if let InputEvent::PointerPressed(button) = event
            && self.captures[button.idx()].press.is_some()
        {
            self.on_input(InputEvent::PointerReleased(button), cascade, now);
        }
        if !self.queue.is_empty() || !self.queue.admits(&event) {
            self.queue.defer(event, now);
            return InputDelta {
                repaint_requested: true,
            };
        }
        self.apply(event, cascade, now)
    }

    /// End the frame for input: forget what it changed and apply held events. Returns whether input
    /// owes the next frame.
    pub(crate) fn next_frame(&mut self, cascade: &Cascade) -> bool {
        self.queue.next_frame();
        while let Some(held) = self.queue.pop_admitted() {
            if held.event.text().is_none() {
                self.apply(held.event, cascade, held.at);
                continue;
            }
            let mut text = mem::take(&mut self.held_text);
            text.clear();
            text.push_str(self.queue.text(held.text));
            self.apply(held.event.with_text(&text), cascade, held.at);
            self.held_text = text;
        }
        self.signal_since_last_frame != InputSignal::None || !self.queue.is_empty()
    }

    fn apply(&mut self, event: InputEvent<'_>, cascade: &Cascade, now: Duration) -> InputDelta {
        // A host event that passed the screen above is at least `Inert`, enough to force
        // a record under `InputPolicy::Always`; a refused event mutates nothing.
        self.signal_since_last_frame.raise(InputSignal::Inert);
        let outcome = match event {
            InputEvent::PointerMoved(p) => {
                let prev_hover = self.hovered;
                let prev_scroll = self.scroll_targets;
                let prev_pinch = self.pinch_target;
                self.pointer_pos = Some(p);
                // Per-button drag latch once travel crosses `DRAG_THRESHOLD`; a right-drag latch
                // suppresses the click too.
                let mut latched = false;
                for cap in &mut self.captures {
                    if let Some(press) = &mut cap.press {
                        press.travel = p - press.origin;
                        if press.drag == PressDrag::None
                            && press.travel.length_squared() >= DRAG_THRESHOLD * DRAG_THRESHOLD
                        {
                            press.drag = PressDrag::Started;
                            latched = true;
                            // A press that became a drag ends the multi-click run.
                            cap.run = None;
                        }
                    }
                }
                self.refresh_pointer_targets(cascade);
                let move_subbed =
                    self.push_pointer_event(PointerWake::MOVE, Some(p), PointerEvent::Move);
                EventOutcome {
                    repaint: self.hovered != prev_hover
                        || self.scroll_targets != prev_scroll
                        || self.pinch_target != prev_pinch
                        || self.any_press()
                        || move_subbed,
                    settles: latched,
                }
            }
            InputEvent::PointerLeft => {
                let observable = self.hovered.is_some()
                    || self.scroll_targets.any()
                    || self.pinch_target.is_some()
                    || self.any_press();
                self.pointer_pos = None;
                self.refresh_pointer_targets(cascade);
                let pointer_subbed = self.push_unclassed(PointerEvent::Leave);
                EventOutcome::repaint(observable || pointer_subbed)
            }
            InputEvent::PointerPressed(btn) => {
                let pointer_pos = self.pointer_pos;
                let targets = pointer_pos.map(|p| cascade.hit_test_press(p));
                let hit = targets.and_then(|t| t.click);
                let buttons_subbed =
                    self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
                        PointerEvent::Down { pos, button: btn }
                    });
                // Any press breaks every other button's multi-click run.
                for (index, cap) in self.captures.iter_mut().enumerate() {
                    if index != btn.idx() {
                        cap.run = None;
                    }
                }
                let cap = self.capture_mut(btn);
                debug_assert!(
                    cap.press.is_none(),
                    "a held button's press synthesizes its release first",
                );
                match hit.zip(pointer_pos) {
                    Some((target, pos)) => {
                        cap.begin_press(target, pos, now);
                        self.queue.note_button(btn);
                    }
                    None => cap.run = None,
                }
                // Focus updates on the left button only, via a separate hit test (focusability is
                // independent of clickability).
                let prev_focus = self.focused;
                if btn == PointerButton::Left {
                    match (targets.and_then(|t| t.focus), self.focus_policy) {
                        (Some(id), _) => self.focused = Some(id),
                        (None, FocusPolicy::ClearOnMiss) => self.focused = None,
                        (None, FocusPolicy::PreserveOnMiss) => {}
                    }
                    self.focus_visible = false;
                }
                // A press on inert surface is a no-op, so `OnDelta` stays on the paint-anim path.
                EventOutcome {
                    repaint: hit.is_some() || self.focused != prev_focus || buttons_subbed,
                    settles: buttons_subbed,
                }
            }
            InputEvent::PointerReleased(btn) => {
                let pointer_pos = self.pointer_pos;
                let was_captured = self.capture(btn).press.is_some();
                if was_captured {
                    self.queue.note_button(btn);
                }
                let cap = self.capture_mut(btn);
                // A `Miss` tears down a capture only one widget reads, which the settle rule
                // excludes; `Click` and `DragStopped` are edges apps act on, so they settle.
                let mut settles = false;
                cap.end_press(|press| {
                    // A latched drag ending is its own edge; otherwise a release back on the widget
                    // is a click carrying its press's run number.
                    let kind = if press.drag == PressDrag::None {
                        let hit = pointer_pos.and_then(|p| cascade.hit_test_press(p).click);
                        if hit == Some(press.target) {
                            ReleaseKind::Click { count: press.count }
                        } else {
                            ReleaseKind::Miss
                        }
                    } else {
                        ReleaseKind::DragStopped
                    };
                    settles = !matches!(kind, ReleaseKind::Miss);
                    kind
                });
                let buttons_subbed =
                    self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
                        PointerEvent::Up { pos, button: btn }
                    });
                EventOutcome {
                    repaint: was_captured || buttons_subbed,
                    settles: settles || buttons_subbed,
                }
            }
            InputEvent::ScrollPixels(d) => self.on_scroll(d, Vec2::ZERO),
            InputEvent::ScrollLines(d) => self.on_scroll(Vec2::ZERO, d),
            InputEvent::Zoom(f) => {
                let target = self.pinch_target;
                if let Some(target) = target {
                    let delta = self.target_scroll_delta_mut(target);
                    let f = ZoomFactor::new(f)
                        .expect("`InputEvent::is_valid` screened the zoom factor above");
                    delta.zoom = delta.zoom.combine(f);
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
                text,
            } => {
                let kp = KeyPress {
                    key,
                    mods: self.modifiers,
                    repeat,
                    physical,
                    text,
                };
                // Wake when a focused widget would consume the key, a chord watcher asked for
                // it, or a `KeyboardWake::KEY` watcher records raw keys. A bare modifier wakes only
                // a watcher that asked, since widgets get modifier state via `ModifiersChanged`.
                let bare_modifier = key == Key::Other && text.is_empty();
                let traverses =
                    KeyClass::of(kp) == KeyClass::Focus && !cascade.tab_stops.is_empty();
                let observable = (self.focused.is_some() && !bare_modifier)
                    || traverses
                    || self.subs.matches_press(kp)
                    || self.subs.keyboard_mask.contains(KeyboardWake::KEY);
                if observable {
                    self.frame_keyboard_events.push(kp);
                    if text.is_empty() && !bare_modifier {
                        self.queue.note_command_key(key);
                    }
                }
                EventOutcome::settle(observable)
            }
            InputEvent::ImeCommit(text) => {
                // Typed in place among the presses as if each character had its own key; no
                // modifiers.
                self.ime_preedit.clear();
                self.ime_cursor = None;
                let observable = self.focused.is_some();
                if observable {
                    let mut piece = KeyText::EMPTY;
                    for c in text.chars() {
                        if !piece.push(c) {
                            self.frame_keyboard_events.push(KeyPress::typed(piece));
                            piece = KeyText::EMPTY;
                            piece.push(c);
                        }
                    }
                    if !piece.is_empty() {
                        self.frame_keyboard_events.push(KeyPress::typed(piece));
                    }
                }
                EventOutcome::settle(observable)
            }
            InputEvent::ImePreedit(ImePreedit { text, cursor }) => {
                self.ime_preedit.clear();
                self.ime_preedit.push_str(text);
                self.ime_cursor = cursor;
                self.ime_owner = self.focused;
                EventOutcome::repaint(self.focused.is_some())
            }
            InputEvent::SurfaceFocusLost => {
                // The platform stops reporting modifiers while another surface is focused, so a
                // stale value is not observable.
                let observable = self.modifiers != Modifiers::NONE
                    || self.captures.iter().any(|c| c.press.is_some());
                self.modifiers = Modifiers::NONE;
                for cap in &mut self.captures {
                    cap.abandon_press();
                }
                EventOutcome::repaint(observable)
            }
            InputEvent::ModifiersChanged(m) => {
                self.modifiers = m;
                EventOutcome::repaint(self.subs.keyboard_mask.contains(KeyboardWake::MODIFIER))
            }
        };
        if outcome.repaint {
            self.signal_since_last_frame.raise(InputSignal::Repaint);
        }
        self.frame_had_action |= outcome.settles;
        InputDelta {
            repaint_requested: outcome.repaint,
        }
    }

    /// Read and reset [`Self::frame_had_action`]; [`crate::Ui::frame`] uses it to decide on a
    /// discarded pre-pass.
    pub(crate) fn take_action_flag(&mut self) -> bool {
        mem::take(&mut self.frame_had_action)
    }

    /// Drain the per-frame input queues without touching cascade-dependent state.
    /// The discarded pass needs empty queues so clicks do not double-fire.
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

    /// Re-resolve `hovered` / `scroll_targets` / `pinch_target` from `pointer_pos`;
    /// the single owner of that assignment. Also routes the held pointer after
    /// cold-start warmup, whose pre-frame events hit an empty cascade.
    pub(crate) fn refresh_pointer_targets(&mut self, cascade: &Cascade) {
        if let Some(p) = self.pointer_pos {
            let hits = cascade.hit_test_targets(p);
            self.hovered = hits.hover;
            self.scroll_targets = hits.scroll;
            self.pinch_target = hits.pinch;
        } else {
            self.hovered = None;
            self.scroll_targets = ScrollTargets::default();
            self.pinch_target = None;
        }
    }

    /// Once-per-frame close-out after the final record pass: recompute hover, drop transient flags,
    /// evict captured widgets that left the tree.
    pub(crate) fn end_frame(&mut self, cascade: &Cascade) {
        self.drain_per_frame_queues();
        self.scopes.end_frame();
        // `modifiers` persists: a held shift must stay `true` across frames.
        //
        // Eviction ends a capture through the same call a release does; dropping the
        // press alone would skip `Drag::Stopped`, losing the commit of `Slider` and
        // `DragValue`.
        for cap in &mut self.captures {
            let vanished = cap
                .press
                .is_some_and(|press| !cascade.by_id.contains_key(&press.target));
            if vanished {
                cap.abandon_press();
                // The release edge belongs to the next frame; raise the signal so it records
                // instead of dropping the edge.
                self.signal_since_last_frame.raise(InputSignal::Repaint);
            }
        }
        // A focused widget that left the tree drops focus; otherwise keystrokes route to a ghost.
        let before = self.focused;
        if let Some(focused) = self.focused
            && !cascade.by_id.contains_key(&focused)
        {
            self.focused = None;
        }
        self.return_from_closed_overlays(cascade);
        // A dialog takes focus as it appears, like `<dialog>.showModal()`.
        let modal_root = cascade
            .roots
            .iter()
            .rev()
            .find(|row| row.layer == Layer::Modal)
            .map(|row| row.id);
        if modal_root.is_some() && modal_root != self.modal_root {
            self.focus_first = modal_root;
        }
        self.modal_root = modal_root;
        if let Some(ancestor) = self.focus_first.take()
            && let Some(first) = cascade.first_tab_stop_within(ancestor)
        {
            self.enter_overlay(ancestor, cascade);
            self.focused = Some(first);
        }
        if self.focused != before {
            // The next frame records against the new focus (ring, scope path), so it must not
            // repaint the retained tree.
            self.signal_since_last_frame.raise(InputSignal::Repaint);
        }
        self.refresh_pointer_targets(cascade);
    }

    /// Give focus back, newest first, for every overlay that left the cascade, if focus is now
    /// nowhere and the holder remains.
    fn return_from_closed_overlays(&mut self, cascade: &Cascade) {
        while let Some(index) = self
            .focus_returns
            .iter()
            .rposition(|row| !cascade.by_id.contains_key(&row.overlay))
        {
            let row = self.focus_returns.remove(index);
            if self.focused.is_none() && cascade.by_id.contains_key(&row.to) {
                self.focused = Some(row.to);
            }
        }
    }

    pub(crate) fn scroll_delta_for(&self, id: WidgetId) -> ScrollDelta {
        self.target_scroll_delta(id).copied().unwrap_or_default()
    }

    /// Snapshot into [`Self::frame_quiescent`] whether any widget can hold
    /// non-default interaction state. `focused` is excluded because
    /// [`crate::Ui::set_focus`] can set it mid-record. The pointer test covers the
    /// routed targets ([`Self::refresh_pointer_targets`] clears them when `pointer_pos`
    /// is `None`), so the fast path opens only while the pointer is off the surface.
    pub(crate) fn snapshot_frame_quiescent(&mut self) {
        debug_assert!(
            self.pointer_pos.is_some()
                || (self.hovered.is_none()
                    && !self.scroll_targets.any()
                    && self.pinch_target.is_none()),
            "a routed target outlived the pointer, so `pointer_pos.is_none()` \
             no longer answers for it",
        );
        self.frame_quiescent = self.pointer_pos.is_none()
            && self.frame_target_deltas.is_empty()
            && self
                .captures
                .iter()
                .all(|c| c.press.is_none() && c.release.is_none());
    }

    /// The pointer in `id`'s local space, the cheap path; shares `pointer_in_widget_space` with
    /// [`Self::response_for`].
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
        loc: Option<WidgetLocation>,
        cascade: &Cascade,
        layout: &Layout,
    ) -> ResponseState {
        // Geometry half, needed every frame. `entries` is AoS so the fields share a cache line.
        let entry = loc.map(|l| cascade.entries[l.entry_idx as usize]);
        let rect = entry.map(|e| e.rect);
        let layout_rect = loc.map(|l| layout.arranged_rect(l.endpoint));
        let transform = entry.map_or(TranslateScale::IDENTITY, |e| e.transform);
        // Cascade flattens parent-disabled into each entry: effective ancestor-or-self disabled,
        // one frame stale.
        let disabled = entry.is_some_and(|e| e.disabled);

        // Built once so a new field cannot be filled on one path and defaulted on the other.
        // `focused` is read live since `set_focus` can run after `frame_quiescent`.
        let mut state = ResponseState {
            rect,
            layout_rect,
            transform,
            disabled,
            focused: self.focused == Some(id),
            ..ResponseState::default()
        };

        // On a quiescent frame every remaining field is default; skip the capture scan and
        // scroll/zoom lookups.
        if self.frame_quiescent {
            return state;
        }

        let me_under_pointer = self.hovered == Some(id);
        let left_press = self.capture(PointerButton::Left).press;
        // Gated on the left capture: while another widget holds the press, the pointer belongs to
        // that gesture.
        state.pointer_over = me_under_pointer && left_press.is_none_or(|p| p.target == id);

        // One slice per button. A live press is `Down` (fresh) or `Held`; otherwise a
        // release edge is `Up`. Same-batch press+release collapses to `Up{click}`, a
        // re-press to `Down`. Only the priority-first latched button owns the drag.
        let mut drag_owned = false;
        for btn in PointerButton::ALL {
            let cap = self.capture(btn);
            let phase = match &cap.press {
                Some(press) if press.target == id => {
                    if press.fresh {
                        ButtonPhase::Down { count: press.count }
                    } else {
                        ButtonPhase::Held
                    }
                }
                _ => match &cap.release {
                    Some(release) if release.target == id => ButtonPhase::Up {
                        click: release.kind.click(),
                    },
                    _ => ButtonPhase::Idle,
                },
            };
            let mut drag = match &cap.release {
                Some(release) if release.target == id && release.kind.ended_drag() => Drag::Stopped,
                _ => Drag::None,
            };
            // A threshold-crossed press overrides the stale stop edge. Read off the press,
            // not the live pointer, so it agrees with `pointer_actions`: leaving the window
            // mid-drag does not end the gesture.
            if !drag_owned
                && let Some(press) = &cap.press
                && press.target == id
                && press.drag != PressDrag::None
            {
                let delta = transform.inverse_vector(press.travel);
                drag = if press.drag == PressDrag::Started {
                    Drag::Started { delta }
                } else {
                    Drag::Active { delta }
                };
                drag_owned = true;
            }
            *state.button_mut(btn) = ButtonState::new(phase, drag);
        }

        state.scroll = self.scroll_delta_for(id);
        state.pointer_local = self
            .pointer_pos
            .zip(layout_rect)
            .map(|(pointer, layout)| pointer_in_widget_space(pointer, layout.min, transform));

        state
    }
}

// Read by the frame harness to know what it already fed.
#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use crate::input::input_state::InputState;
    use crate::input::keyboard::modifiers::Modifiers;

    impl InputState {
        pub(crate) fn has_held_input(&self) -> bool {
            !self.queue.is_empty()
        }

        /// The modifier set once every held event has landed; the live `modifiers` lags
        /// what a feeder already sent.
        pub(crate) fn modifiers_after_held_input(&self) -> Modifiers {
            self.queue.last_held_modifiers().unwrap_or(self.modifiers)
        }
    }
}

#[cfg(test)]
mod tests;
