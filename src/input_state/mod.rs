//! The live input state machine: what survives across input events.

pub(crate) mod arranged_frame;
pub(crate) mod capture;
pub(crate) mod event_outcome;
pub(crate) mod focus;
mod ime;
pub(crate) mod input_queue;
pub(crate) mod key_reader;
mod keyboard;
mod pointer;
pub(crate) mod scope;
pub(crate) mod target_scroll_delta;
pub(crate) mod watches;

use crate::cascade::Cascade;
use crate::cascade::{TabDirection, TabDomain, WidgetLocation};
use crate::input::ime_preedit::ImePreedit;
use crate::input::input_event::InputEvent;
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
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::{PointerButton, PointerEvent};
use crate::input::policy::{FocusPolicy, InputPolicy, InputSignal};
use crate::input::shortcut::Shortcut;
use crate::input::watch::{KeyboardWake, PointerWake};
use crate::input::zoom_factor::ZoomFactor;
use crate::input_state::arranged_frame::ArrangedFrame;
use crate::input_state::capture::{PressDrag, ReleaseKind};
use crate::input_state::event_outcome::EventOutcome;
use crate::input_state::focus::{Focus, FocusCause};
use crate::input_state::ime::Ime;
use crate::input_state::input_queue::InputQueue;
use crate::input_state::key_reader::KeyReader;
use crate::input_state::keyboard::{HeldKey, Keyboard};
use crate::input_state::pointer::Pointer;
use crate::input_state::scope::Scopes;
use crate::input_state::watches::Watches;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::scene::plane::Plane;
use glam::Vec2;
use std::mem;
use std::time::Duration;

fn pointer_in_widget_space(pointer: Vec2, layout_origin: Vec2, transform: TranslateScale) -> Vec2 {
    let surface_origin = transform.apply_point(layout_origin);
    transform.inverse_vector(pointer - surface_origin)
}

/// What input leaves behind between events: the pointer and what it routes to, each button's
/// capture, focus, the modifiers and the IME composition, plus this frame's edges for the record
/// passes to read.
#[derive(Debug, Default)]
pub(crate) struct InputState {
    pointer: Pointer,
    keyboard: Keyboard,
    ime: Ime,
    focus: Focus,
    /// Snapshot of "no widget can hold non-default interaction state this frame".
    /// Taken before `App::update` and per record pass so [`Self::response_for`] can
    /// skip the interaction half; `false` before the first fill. `focused` is excluded.
    frame_quiescent: bool,
    /// The text of a held event being replayed, kept for the next.
    held_text: String,
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
    queue: InputQueue,
}

#[derive(Clone, Copy, Debug)]
struct Traversal {
    domain: TabDomain,
    direction: TabDirection,
}

/// What an event applies against beside the input's own state: the cascade it hit-tests in, and
/// how soon a second press on the same target chains a run, which the host shares among its
/// windows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InputCtx<'a> {
    pub(crate) cascade: &'a Cascade,
    pub(crate) double_click: Duration,
}

impl InputState {
    /// Start a record pass: drop last pass's watches and resolve this pass's scope path once.
    pub(crate) fn pre_record(&mut self, cascade: &Cascade) {
        self.subs.clear();
        self.scopes.resolve(self.focus.focused(), cascade);
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
        while i < self.keyboard.presses.len() {
            let press = self.keyboard.presses[i];
            let Some(Traversal { domain, direction }) = self.traversal(press, cascade) else {
                i += 1;
                continue;
            };
            if let Some(next) = cascade.next_tab_stop(domain, self.focus.focused(), direction) {
                self.focus.move_to(Some(next), FocusCause::Keyboard);
            }
            self.keyboard.presses.remove(i);
            self.scopes.resolve(self.focus.focused(), cascade);
        }
    }

    /// Where `press` moves focus, if the framework owns it.
    fn traversal(&self, press: KeyPress, cascade: &Cascade) -> Option<Traversal> {
        match KeyClass::of(press) {
            KeyClass::Focus if !self.scopes.path_takes(KeyClass::Focus) => {
                let direction = if press.mods.shift {
                    TabDirection::Prev
                } else {
                    TabDirection::Next
                };
                Some(Traversal {
                    domain: cascade.tab_domain(self.focus.focused()),
                    direction,
                })
            }
            KeyClass::Caret if press.mods == Modifiers::NONE => {
                let group = cascade.arrow_group_of(self.focus.focused()?)?;
                let direction = match (group.axis, press.key) {
                    (Axis::Y, Key::ArrowDown) | (Axis::X, Key::ArrowRight) => TabDirection::Next,
                    (Axis::Y, Key::ArrowUp) | (Axis::X, Key::ArrowLeft) => TabDirection::Prev,
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

    pub(crate) const fn focus_first_within(&mut self, ancestor: WidgetId) {
        self.focus.focus_first_within(ancestor);
    }

    #[inline]
    pub(crate) const fn focused(&self) -> Option<WidgetId> {
        self.focus.focused()
    }

    /// The composition, while the widget it arrived for still holds focus; a move this frame hides
    /// it, and [`Self::end_frame`] retires it.
    pub(crate) fn ime_preedit(&self) -> Option<ImePreedit<'_>> {
        self.ime.shown_to(self.focus.focused())
    }

    #[inline]
    pub(crate) const fn focus_visible(&self) -> bool {
        self.focus.visible()
    }

    #[inline]
    pub(crate) const fn modifiers(&self) -> Modifiers {
        self.keyboard.modifiers
    }

    #[inline]
    pub(crate) const fn pointer_pos(&self) -> Option<Vec2> {
        self.pointer.pos
    }

    #[inline]
    pub(crate) const fn hovered(&self) -> Option<WidgetId> {
        self.pointer.hovered
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
        self.focus.move_to(id, FocusCause::Program);
    }

    pub(crate) fn release_input_scope(&mut self, owner: WidgetId) {
        self.scopes.close(owner);
    }

    /// The state a warmup pass records against: no input, but this state's focus.
    pub(crate) fn warmup_scratch(&self) -> Self {
        Self {
            focus: self.focus.warmup_scratch(),
            ..Self::default()
        }
    }

    /// Keep the focus moves and scope withdrawals a warmup pass recorded.
    pub(crate) fn adopt_warmup(&mut self, warmup: &Self) {
        self.focus.adopt_warmup(&warmup.focus);
        self.scopes.adopt_closing(&warmup.scopes);
    }

    pub(crate) const fn watch_pointer(&mut self, flags: PointerWake) {
        self.subs.pointer_mask.insert(flags);
    }

    pub(crate) const fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.subs.keyboard_mask.insert(flags);
    }

    pub(crate) fn watch_shortcut(&mut self, shortcut: Shortcut) {
        self.subs.watch_shortcut(shortcut);
    }

    /// The raw keyboard stream as seen from `reader`'s plane; plane-gated only, to keep arrival
    /// order.
    pub(crate) fn keyboard_events(&self, reader: Plane) -> &[KeyPress] {
        if self.silenced(reader) {
            return &[];
        }
        &self.keyboard.presses
    }

    /// Whether an overlay's scope cuts `reader`'s plane off (strictly below, so its own body keeps
    /// reading).
    fn silenced(&self, reader: Plane) -> bool {
        self.scopes.silences(reader)
    }

    /// The pointer watch stream as seen from `reader`'s plane, gated like
    /// [`Self::keyboard_events`]. Watches bypass hit-testing, so without this a
    /// `Main`-layer `SCROLL` watcher would keep receiving events under a modal.
    pub(crate) fn pointer_events(&self, reader: Plane) -> &[PointerEvent] {
        if self.silenced(reader) {
            return &[];
        }
        &self.pointer.events
    }

    /// Whether `shortcut` was pressed and granted to the scope `reader`'s parent sits in.
    pub(crate) fn shortcut_pressed(
        &mut self,
        reader: KeyReader,
        cascade: &Cascade,
        shortcut: Shortcut,
    ) -> bool {
        self.subs.watch_shortcut(shortcut);
        self.granted_press(reader, cascade, |press| shortcut.matches(press))
    }

    /// Whether `key` was pressed, whatever the modifiers, and granted to the scope `reader`'s
    /// parent sits in.
    pub(crate) fn key_pressed(&mut self, reader: KeyReader, cascade: &Cascade, key: Key) -> bool {
        self.subs.watch_key(key);
        self.granted_press(reader, cascade, |press| press.key.same_key(key))
    }

    /// Whether a press this frame passes `matches` and is granted to `reader`'s scope.
    fn granted_press(
        &mut self,
        reader: KeyReader,
        cascade: &Cascade,
        matches: impl Fn(KeyPress) -> bool,
    ) -> bool {
        if self.keyboard.presses.is_empty() || self.silenced(reader.plane) {
            return false;
        }
        let scope = self.scopes.reader(reader.parent, cascade);
        self.keyboard
            .presses
            .iter()
            .any(|press| matches(*press) && self.scopes.reaches(KeyClass::of(*press), scope))
    }

    /// Whether `key` is down now, whatever the modifiers. A fact of the keyboard, as the
    /// modifiers are, so not routed.
    pub(crate) fn key_held(&mut self, key: Key) -> bool {
        self.subs.watch_key(key);
        self.keyboard.is_held(key)
    }

    /// Whether `key` came up this frame, or was held when the surface lost the keyboard.
    pub(crate) fn key_released(&mut self, key: Key) -> bool {
        self.subs.watch_key(key);
        self.keyboard.was_released(key)
    }

    /// Every edge the pointer produced this frame, widget by widget, from the same
    /// capture state as [`Self::response_for`]. Three slots per button, at most two
    /// filled (`InputQueue` admits one press-or-release per button per frame). A widget
    /// `cascade` holds disabled, itself or by an ancestor, has none, as its response is empty:
    /// the press still latched on it, so nothing behind it takes the input.
    #[inline]
    pub(crate) fn pointer_actions<'a>(
        &'a self,
        cascade: &'a Cascade,
    ) -> impl Iterator<Item = PointerAction> + 'a {
        let disabled = move |id| {
            cascade
                .locate(id)
                .is_some_and(|loc| cascade.entry(loc).disabled)
        };
        PointerButton::ALL.into_iter().flat_map(move |button| {
            let cap = self.pointer.capture(button);
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
            [pressed, dragging, ended]
                .into_iter()
                .flatten()
                .filter(move |action| !disabled(action.id))
        })
    }

    /// Push a pointer event to the frame's pointer events; returns whether it
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
            self.pointer.events.push(make(pos));
        }
        true
    }

    /// Push `Leave`, which belongs to no single watch class and carries no position.
    fn push_unclassed(&mut self, event: PointerEvent) -> bool {
        if self.subs.pointer_mask.is_empty() {
            return false;
        }
        self.pointer.events.push(event);
        true
    }

    /// Accumulate one scroll delta on the current targets and wake watchers. Pixels and lines
    /// travel in separate lanes (see [`ScrollDelta`]).
    fn on_scroll(&mut self, pixels: Vec2, lines: Vec2) -> EventOutcome {
        let mut delivered = false;
        for share in self
            .pointer
            .scroll_targets
            .route(pixels, lines)
            .into_iter()
            .flatten()
        {
            let delta = self.pointer.delta_mut(share.target);
            delta.pixels += share.pixels;
            delta.lines += share.lines;
            delivered = true;
        }
        let subbed = self.push_positioned(PointerWake::SCROLL, |pos| PointerEvent::Scroll {
            pos,
            pixels,
            lines,
        });
        if delivered || subbed {
            self.queue.note_delta();
        }
        EventOutcome::repaint(delivered || subbed)
    }

    /// Push for position-routed events (scroll, pinch); they wake only with a pointer on the
    /// surface.
    fn push_positioned(
        &mut self,
        wake: PointerWake,
        make: impl FnOnce(Vec2) -> PointerEvent,
    ) -> bool {
        self.pointer.pos.is_some() && self.push_pointer_event(wake, self.pointer.pos, make)
    }

    /// Feed a palantir-native input event, hit-tested against this frame's latest
    /// cascade. `now` is the arrival time on the host's clock (see
    /// [`Ui::on_input`](crate::Ui::on_input)).
    pub(crate) fn on_input(
        &mut self,
        event: InputEvent<'_>,
        ctx: InputCtx<'_>,
        now: Duration,
    ) -> InputDelta {
        if !event.is_valid() {
            return InputDelta::default();
        }
        if self.queue.is_empty() && self.ready(&event) {
            return self.apply(event, ctx, now);
        }
        self.queue.defer(event, now);
        InputDelta {
            repaint_requested: true,
        }
    }

    /// Whether `event` applies this frame (see [`InputQueue`]). A press of a button still held
    /// means its release was lost: the capture ends as a cut-off gesture, not as a release that
    /// hit-tests a click nobody made, and the press waits a frame so that end is seen.
    fn ready(&mut self, event: &InputEvent<'_>) -> bool {
        if !self.queue.admits(event) {
            return false;
        }
        if let InputEvent::PointerPressed(button) = *event
            && self.pointer.capture(button).press.is_some()
        {
            self.pointer.capture_mut(button).abandon_press();
            self.queue.note_capture(button);
            self.signal_since_last_frame.raise(InputSignal::Repaint);
            return false;
        }
        true
    }

    /// End the frame for input: forget what it changed and apply held events. Returns whether input
    /// owes the next frame.
    pub(crate) fn next_frame(&mut self, ctx: InputCtx<'_>) -> bool {
        self.queue.next_frame();
        // An eviction's release edge (see `end_frame`) belongs to the frame starting now, so that
        // button's next change waits a frame, as after any release.
        for button in PointerButton::ALL {
            if self.pointer.capture(button).release.is_some() {
                self.queue.note_capture(button);
            }
        }
        while let Some(held) = self.queue.front() {
            if !self.ready(&held.event) {
                break;
            }
            self.queue.pop_front();
            if held.event.text().is_none() {
                self.apply(held.event, ctx, held.at);
                continue;
            }
            let mut text = mem::take(&mut self.held_text);
            text.clear();
            text.push_str(self.queue.text(held.text));
            self.apply(held.event.with_text(&text), ctx, held.at);
            self.held_text = text;
        }
        self.signal_since_last_frame != InputSignal::None || !self.queue.is_empty()
    }

    /// Apply `event`, which the queue admitted, and say whether it asks for a frame; each kind of
    /// event goes to its handler.
    fn apply(&mut self, event: InputEvent<'_>, ctx: InputCtx<'_>, now: Duration) -> InputDelta {
        // A host event that passed the screen above is at least `Inert`, enough to force
        // a record under `InputPolicy::Always`; a refused event mutates nothing.
        self.signal_since_last_frame.raise(InputSignal::Inert);
        let outcome = match event {
            InputEvent::PointerMoved(p) => self.on_pointer_moved(p, ctx.cascade),
            InputEvent::PointerLeft => self.on_pointer_left(ctx.cascade),
            InputEvent::PointerPressed(btn) => self.on_pointer_pressed(btn, ctx, now),
            InputEvent::PointerReleased(btn) => self.on_pointer_released(btn, ctx.cascade),
            InputEvent::ScrollPixels(d) => self.on_scroll(d, Vec2::ZERO),
            InputEvent::ScrollLines(d) => self.on_scroll(Vec2::ZERO, d),
            InputEvent::Zoom(f) => self.on_zoom(f),
            InputEvent::KeyDown {
                key,
                repeat,
                physical,
                text,
            } => self.on_key_down(
                KeyPress {
                    key,
                    mods: self.keyboard.modifiers,
                    repeat,
                    physical,
                    text,
                },
                ctx.cascade,
            ),
            InputEvent::ImeCommit(text) => self.on_ime_commit(text),
            InputEvent::ImePreedit(preedit) => {
                self.ime.compose(preedit, self.focus.focused());
                EventOutcome::repaint(self.ime.owner.is_some())
            }
            InputEvent::KeyUp { key } => {
                if !self.keyboard.release(key) {
                    return InputDelta::default();
                }
                EventOutcome::repaint(self.subs.watches_key(key))
            }
            InputEvent::SurfaceFocusLost => self.on_surface_focus_lost(),
            InputEvent::ModifiersChanged(m) => {
                self.keyboard.modifiers = m;
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

    fn on_pointer_moved(&mut self, p: Vec2, cascade: &Cascade) -> EventOutcome {
        let prev_hover = self.pointer.hovered;
        let prev_scroll = self.pointer.scroll_targets;
        let prev_pinch = self.pointer.pinch_target;
        let latched = self.pointer.move_to(p);
        self.refresh_pointer_targets(cascade);
        let move_subbed = self.push_pointer_event(PointerWake::MOVE, Some(p), PointerEvent::Move);
        EventOutcome {
            repaint: self.pointer.hovered != prev_hover
                || self.pointer.scroll_targets != prev_scroll
                || self.pointer.pinch_target != prev_pinch
                || self.pointer.any_press()
                || move_subbed,
            settles: latched,
        }
    }

    fn on_pointer_left(&mut self, cascade: &Cascade) -> EventOutcome {
        let observable = self.pointer.hovered.is_some()
            || self.pointer.scroll_targets.any()
            || self.pointer.pinch_target.is_some()
            || self.pointer.any_press();
        self.pointer.pos = None;
        self.refresh_pointer_targets(cascade);
        let pointer_subbed = self.push_unclassed(PointerEvent::Leave);
        EventOutcome::repaint(observable || pointer_subbed)
    }

    fn on_pointer_pressed(
        &mut self,
        btn: PointerButton,
        ctx: InputCtx<'_>,
        now: Duration,
    ) -> EventOutcome {
        let InputCtx {
            cascade,
            double_click,
        } = ctx;
        let pointer_pos = self.pointer.pos;
        let targets = pointer_pos.map(|p| cascade.hit_test_press(p));
        let hit = targets.and_then(|t| t.click);
        let buttons_subbed = self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
            PointerEvent::Down { pos, button: btn }
        });
        // Any press breaks every other button's multi-click run.
        for (idx, cap) in self.pointer.captures.iter_mut().enumerate() {
            if idx != btn.idx() {
                cap.run = None;
            }
        }
        let cap = self.pointer.capture_mut(btn);
        debug_assert!(
            cap.press.is_none(),
            "a press of a held button ends the lost capture before it applies",
        );
        match hit.zip(pointer_pos) {
            Some((target, pos)) => {
                cap.begin_press(target, pos, now, double_click);
                self.queue.note_capture(btn);
            }
            None => cap.run = None,
        }
        // Focus updates on the left button only, via a separate hit test (focusability is
        // independent of clickability).
        let prev_focus = self.focus.focused();
        if btn == PointerButton::Left {
            let next = match (targets.and_then(|t| t.focus), self.focus_policy) {
                (Some(id), _) => Some(id),
                (None, FocusPolicy::ClearOnMiss) => None,
                (None, FocusPolicy::PreserveOnMiss) => prev_focus,
            };
            self.focus.move_to(next, FocusCause::Pointer);
        }
        // A press on inert surface is a no-op, so `OnDelta` stays on the paint-anim path.
        EventOutcome {
            repaint: hit.is_some() || self.focus.focused() != prev_focus || buttons_subbed,
            settles: buttons_subbed,
        }
    }

    fn on_pointer_released(&mut self, btn: PointerButton, cascade: &Cascade) -> EventOutcome {
        let pointer_pos = self.pointer.pos;
        let was_captured = self.pointer.capture(btn).press.is_some();
        if was_captured {
            self.queue.note_capture(btn);
        }
        let cap = self.pointer.capture_mut(btn);
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
        let buttons_subbed = self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
            PointerEvent::Up { pos, button: btn }
        });
        EventOutcome {
            repaint: was_captured || buttons_subbed,
            settles: settles || buttons_subbed,
        }
    }

    fn on_zoom(&mut self, f: f32) -> EventOutcome {
        let target = self.pointer.pinch_target;
        let factor =
            ZoomFactor::new(f).expect("`InputEvent::is_valid` screened the zoom factor above");
        if let Some(target) = target {
            let delta = self.pointer.delta_mut(target);
            delta.zoom = delta.zoom.combine(factor);
        }
        let subbed =
            self.push_positioned(PointerWake::PINCH, |pos| PointerEvent::Zoom { pos, factor });
        if target.is_some() || subbed {
            self.queue.note_delta();
        }
        EventOutcome::repaint(target.is_some() || subbed)
    }

    fn on_key_down(&mut self, kp: KeyPress, cascade: &Cascade) -> EventOutcome {
        let KeyPress {
            key,
            repeat,
            physical,
            text,
            ..
        } = kp;
        // Held whether or not anyone reads the press: a later frame may ask.
        self.keyboard.hold(HeldKey { key, physical }, repeat);
        // Wake when a focused widget would consume the key, a chord watcher asked for
        // it, or a `KeyboardWake::KEY` watcher records raw keys. A bare modifier wakes only
        // a watcher that asked, since widgets get modifier state via `ModifiersChanged`.
        let bare_modifier = input_queue::is_bare_modifier(key, text);
        let traverses = KeyClass::of(kp) == KeyClass::Focus && !cascade.tab_stops.is_empty();
        let observable = (self.focus.focused().is_some() && !bare_modifier)
            || traverses
            || self.subs.matches_press(kp)
            || self.subs.keyboard_mask.contains(KeyboardWake::KEY);
        if observable {
            self.keyboard.presses.push(kp);
            self.queue.note_key(key, text);
        }
        EventOutcome::settle(observable)
    }

    /// Committed text, typed in place among the presses; the composition it replaces ends.
    fn on_ime_commit(&mut self, text: &str) -> EventOutcome {
        self.ime.end();
        let observable = self.focus.focused().is_some();
        if observable {
            self.keyboard.type_text(text);
            self.queue.note_text();
        }
        EventOutcome::settle(observable)
    }

    fn on_surface_focus_lost(&mut self) -> EventOutcome {
        // The platform stops reporting modifiers while another surface is focused, so a
        // stale value is not observable.
        let observable = self.keyboard.modifiers != Modifiers::NONE
            || self.pointer.any_press()
            || !self.ime.text.is_empty()
            || self
                .keyboard
                .held
                .iter()
                .any(|held| self.subs.watches_key(held.key));
        self.keyboard.lose_surface();
        for button in PointerButton::ALL {
            if self.pointer.capture(button).press.is_some() {
                self.pointer.capture_mut(button).abandon_press();
                self.queue.note_capture(button);
            }
        }
        // The input method's composition belongs to the surface that had the keyboard.
        self.ime.retire();
        EventOutcome::repaint(observable)
    }

    /// Read and reset [`Self::frame_had_action`]; [`crate::Ui::frame`] uses it to decide on a
    /// discarded pre-pass.
    pub(crate) fn take_action_flag(&mut self) -> bool {
        mem::take(&mut self.frame_had_action)
    }

    /// Drain the per-frame input queues without touching cascade-dependent state.
    /// The discarded pass needs empty queues so clicks do not double-fire.
    pub(crate) fn drain_per_frame_queues(&mut self) {
        self.pointer.end_frame();
        self.keyboard.end_frame();
        self.signal_since_last_frame = InputSignal::None;
        self.frame_had_action = false;
    }

    /// Re-resolve `hovered` / `scroll_targets` / `pinch_target` from `pointer_pos`;
    /// the single owner of that assignment. Also routes the held pointer after
    /// cold-start warmup, whose pre-frame events hit an empty cascade.
    pub(crate) fn refresh_pointer_targets(&mut self, cascade: &Cascade) {
        self.pointer.retarget(cascade);
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
        for cap in &mut self.pointer.captures {
            let vanished = cap
                .press
                .is_some_and(|press| !cascade.contains(press.target));
            if vanished {
                cap.abandon_press();
                // The release edge belongs to the next frame; raise the signal so it records
                // instead of dropping the edge.
                self.signal_since_last_frame.raise(InputSignal::Repaint);
            }
        }
        if self.focus.reconcile(cascade) {
            // The next frame records against the new focus (ring, scope path), so it must not
            // repaint the retained tree.
            self.signal_since_last_frame.raise(InputSignal::Repaint);
        }
        // A composition hidden by a focus move would come back when focus does.
        if self.ime.owner != self.focus.focused() {
            self.ime.retire();
        }
        self.refresh_pointer_targets(cascade);
    }

    pub(crate) fn scroll_delta_for(&self, id: WidgetId) -> ScrollDelta {
        self.pointer.delta(id).copied().unwrap_or_default()
    }

    /// Snapshot into [`Self::frame_quiescent`] whether any widget can hold
    /// non-default interaction state. `focused` is excluded because
    /// [`crate::Ui::set_focus`] can set it mid-record. The pointer test covers the
    /// routed targets ([`Self::refresh_pointer_targets`] clears them when `pointer_pos`
    /// is `None`), so the fast path opens only while the pointer is off the surface.
    pub(crate) fn snapshot_frame_quiescent(&mut self) {
        debug_assert!(
            self.pointer.pos.is_some()
                || (self.pointer.hovered.is_none()
                    && !self.pointer.scroll_targets.any()
                    && self.pointer.pinch_target.is_none()),
            "a routed target outlived the pointer, so `pointer_pos.is_none()` \
             no longer answers for it",
        );
        self.frame_quiescent = self.pointer.pos.is_none()
            && self.pointer.target_deltas.is_empty()
            && self
                .pointer
                .captures
                .iter()
                .all(|c| c.press.is_none() && c.release.is_none());
    }

    /// The pointer in `id`'s local space, the cheap path; shares `pointer_in_widget_space` with
    /// [`Self::response_for`].
    pub(crate) fn pointer_local_for(&self, id: WidgetId, frame: ArrangedFrame<'_>) -> Option<Vec2> {
        let pointer = self.pointer.pos?;
        let loc = frame.cascade.locate(id)?;
        let layout_rect = frame.layout.arranged_rect(loc.endpoint);
        let transform = frame.cascade.entry(loc).transform;
        Some(pointer_in_widget_space(pointer, layout_rect.min, transform))
    }

    pub(crate) fn response_for(
        &self,
        id: WidgetId,
        loc: Option<WidgetLocation>,
        frame: ArrangedFrame<'_>,
    ) -> ResponseState {
        // Geometry half, needed every frame. `entries` is AoS so the fields share a cache line.
        let entry = loc.map(|l| *frame.cascade.entry(l));
        let rect = entry.map(|e| e.rect);
        let layout_rect = loc.map(|l| frame.layout.arranged_rect(l.endpoint));
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
            focused: self.focus.focused() == Some(id),
            ..ResponseState::default()
        };

        // On a quiescent frame every remaining field is default; skip the capture scan and
        // scroll/zoom lookups.
        if self.frame_quiescent {
            return state;
        }

        let me_under_pointer = self.pointer.hovered == Some(id);
        let left_press = self.pointer.capture(PointerButton::Left).press;
        // Gated on the left capture: while another widget holds the press, the pointer belongs to
        // that gesture.
        state.pointer_over = me_under_pointer && left_press.is_none_or(|p| p.target == id);

        // One slice per button, each with its own drag. A live press is `Down` (fresh) or
        // `Held`; otherwise a release edge is `Up`.
        for btn in PointerButton::ALL {
            let cap = self.pointer.capture(btn);
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
            // A threshold-crossed press overrides the stale stop edge. Read off the press,
            // not the live pointer, so it agrees with `pointer_actions`: leaving the window
            // mid-drag does not end the gesture.
            let drag = match &cap.press {
                Some(press) if press.target == id && press.drag != PressDrag::None => {
                    let delta = transform.inverse_vector(press.travel);
                    if press.drag == PressDrag::Started {
                        Drag::Started { delta }
                    } else {
                        Drag::Active { delta }
                    }
                }
                _ => match &cap.release {
                    Some(release) if release.target == id && release.kind.ended_drag() => {
                        Drag::Stopped
                    }
                    _ => Drag::None,
                },
            };
            *state.button_mut(btn) = ButtonState::new(phase, drag);
        }

        state.scroll = self.scroll_delta_for(id);
        state.pointer_local = self
            .pointer
            .pos
            .zip(layout_rect)
            .map(|(pointer, layout)| pointer_in_widget_space(pointer, layout.min, transform));

        state
    }
}

/// Read by a harness to know what it already fed.
impl InputState {
    /// Whether input waits for a later frame (one change of a kind per frame).
    pub(crate) fn has_held_input(&self) -> bool {
        !self.queue.is_empty()
    }

    /// The modifier set once every held event has landed; the live `modifiers` lags what a
    /// feeder already sent.
    pub(crate) fn modifiers_after_held_input(&self) -> Modifiers {
        self.queue
            .last_held_modifiers()
            .unwrap_or(self.keyboard.modifiers)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::input::scroll_targets::ScrollTargets;
    use crate::input_state::InputState;
    use crate::primitives::identity::widget_id::WidgetId;

    /// Routing state the widget suites assert on.
    impl InputState {
        pub(crate) const fn scroll_targets(&self) -> ScrollTargets {
            self.pointer.scroll_targets
        }

        pub(crate) const fn pinch_target(&self) -> Option<WidgetId> {
            self.pointer.pinch_target
        }
    }
}

#[cfg(test)]
mod tests;
