//! [`Input`], the live input state machine: what survives across input events and the latest
//! arranged frame they meet. The types it reads and hands out are in `primitives::input`. Refused: one
//! merged module, a cycle through `scene`, which imports `Sense`, and `cascade`, which this imports.

pub(crate) mod arranged;
#[cfg(feature = "internals")]
pub(crate) mod bench;
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

use crate::cascade::{TabDirection, TabDomain};
use crate::input::arranged::Arranged;
use crate::input::capture::ReleaseKind;
use crate::input::event_outcome::EventOutcome;
use crate::input::focus::{Focus, FocusCause, FocusEntry};
use crate::input::ime::Ime;
use crate::input::input_queue::{HeldEvent, InputQueue};
use crate::input::key_reader::KeyReader;
use crate::input::keyboard::{HeldKey, Keyboard};
use crate::input::pointer::Pointer;
use crate::input::scope::Scopes;
use crate::input::watches::Watches;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::ime_preedit::ImePreedit;
use crate::primitives::input::input_event::InputEvent;
use crate::primitives::input::interaction::input_delta::InputDelta;
use crate::primitives::input::interaction::pointer_action::PointerAction;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::interaction::scroll_delta::ScrollDelta;
use crate::primitives::input::interaction::wheel_delta::WheelDelta;
use crate::primitives::input::key_class::KeyClass;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_press::KeyPress;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::{PointerButton, PointerEvent};
use crate::primitives::input::policy::{FocusPolicy, InputPolicy, InputSignal};
use crate::primitives::input::shortcut::Shortcut;
use crate::primitives::input::tab_end::TabEnd;
use crate::primitives::input::watch::{KeyboardWake, PointerWake};
use crate::primitives::input::zoom_factor::ZoomFactor;
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
/// passes to read. [`Ui`](crate::Ui) is its public face.
///
/// It owns the latest arranged frame, the one thing an event means anything against, so no read
/// takes it as a parameter. Refused: the cascade and layout on `Ui`, which threaded them through 27
/// signatures here.
#[derive(Debug)]
pub(crate) struct Input {
    /// The latest arranged frame. The engines write it, and the encoder and damage read it too.
    pub(crate) arranged: Arranged,
    pointer: Pointer,
    keyboard: Keyboard,
    ime: Ime,
    focus: Focus,
    /// Snapshot of "no widget can hold non-default interaction state this frame".
    /// Taken per record pass so [`Ui::response_for`](crate::Ui::response_for) can
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
    /// Wake-gate watches, cleared as a frame's first pass opens. The masks persist across silent
    /// frames so a dormant popup is paged in by the next click.
    watches: Watches,
    queue: InputQueue,
}

#[derive(Clone, Copy, Debug)]
struct Traversal {
    domain: TabDomain,
    direction: TabDirection,
}

impl Input {
    /// An input with nothing arranged. No public constructor: a `Ui` owns its one.
    pub(crate) fn new() -> Self {
        Self {
            arranged: Arranged::default(),
            pointer: Pointer::default(),
            keyboard: Keyboard::default(),
            ime: Ime::default(),
            focus: Focus::default(),
            frame_quiescent: false,
            held_text: String::new(),
            scopes: Scopes::default(),
            focus_policy: FocusPolicy::default(),
            input_policy: InputPolicy::default(),
            frame_had_action: false,
            signal_since_last_frame: InputSignal::default(),
            watches: Watches::default(),
            queue: InputQueue::default(),
        }
    }

    /// Drop last frame's watches.
    pub(crate) fn clear_watches(&mut self) {
        self.watches.clear();
    }

    /// Start a record pass: resolve this pass's scope path once, take focus keys, and note whether
    /// any widget can hold state.
    pub(crate) fn pre_record(&mut self) {
        self.scopes
            .resolve(self.focus.focused(), &self.arranged.cascade);
        self.traverse_focus();
        self.snapshot_frame_quiescent();
    }

    /// Move focus for each unclaimed Tab and Shift+Tab ([`KeyClass::Focus`]),
    /// removing those presses from the key stream; scopes resolve again after each
    /// move. Arrows do the same inside an arrow group
    /// ([`Configure::arrow_focus`](crate::Configure::arrow_focus)) when no scope inside
    /// the group takes them.
    fn traverse_focus(&mut self) {
        let mut i = 0;
        while i < self.keyboard.presses.len() {
            let press = self.keyboard.presses[i];
            let Some(Traversal { domain, direction }) = self.traversal(press) else {
                i += 1;
                continue;
            };
            if let Some(next) =
                self.arranged
                    .cascade
                    .next_tab_stop(domain, self.focus.focused(), direction)
            {
                self.focus.move_to(Some(next), FocusCause::Keyboard);
            }
            self.keyboard.presses.remove(i);
            self.scopes
                .resolve(self.focus.focused(), &self.arranged.cascade);
        }
    }

    /// Where `press` moves focus, if the framework owns it.
    fn traversal(&self, press: KeyPress) -> Option<Traversal> {
        let cascade = &self.arranged.cascade;
        match press.class() {
            KeyClass::Focus if !self.scopes.path_takes(KeyClass::Focus) => {
                let direction = if press.mods.contains(Modifiers::SHIFT) {
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
                    domain: TabDomain::Within(group.id),
                    direction,
                })
            }
            _ => None,
        }
    }

    /// Move focus to the Tab stop at `end` under `ancestor` at the end of this frame. See
    /// [`Ui::set_focus_within`](crate::Ui::set_focus_within).
    pub(crate) const fn set_focus_within(&mut self, ancestor: WidgetId, end: TabEnd) {
        self.focus.enter_within(FocusEntry {
            within: ancestor,
            end,
        });
    }

    /// Currently focused widget id, or `None`. See [`Ui::focus`](crate::Ui::focus).
    #[inline]
    pub(crate) const fn focus(&self) -> Option<WidgetId> {
        self.focus.focused()
    }

    /// The composition, while the widget it arrived for still holds focus; a move this frame hides
    /// it, and [`Self::settle`] retires it.
    pub(crate) fn ime_preedit(&self) -> Option<ImePreedit<'_>> {
        self.ime.shown_to(self.focus.focused())
    }

    #[inline]
    pub(crate) const fn focus_visible(&self) -> bool {
        self.focus.visible()
    }

    /// Currently held modifier keys, and a watch of them. See
    /// [`Ui::modifiers`](crate::Ui::modifiers).
    #[inline]
    pub(crate) const fn modifiers(&mut self) -> Modifiers {
        self.watch_keyboard(KeyboardWake::MODIFIER);
        self.keyboard.modifiers
    }

    /// [`Self::modifiers`] with no watch. See [`Ui::peek_modifiers`](crate::Ui::peek_modifiers).
    #[inline]
    pub(crate) const fn peek_modifiers(&self) -> Modifiers {
        self.keyboard.modifiers
    }

    /// Pointer position in logical pixels (surface space), or `None` off-surface, and a watch of
    /// it. See [`Ui::pointer_pos`](crate::Ui::pointer_pos).
    #[inline]
    pub(crate) const fn pointer_pos(&mut self) -> Option<Vec2> {
        self.watch_pointer(PointerWake::MOVE);
        self.pointer.pos
    }

    /// [`Self::pointer_pos`] with no watch. See
    /// [`Ui::peek_pointer_pos`](crate::Ui::peek_pointer_pos).
    #[inline]
    pub(crate) const fn peek_pointer_pos(&self) -> Option<Vec2> {
        self.pointer.pos
    }

    /// The widget the pointer hovers, under every button's capture ([`Pointer::hovered`]).
    #[inline]
    pub(crate) fn hovered(&self) -> Option<WidgetId> {
        self.pointer.hovered()
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

    /// Move keyboard focus to `id` now. See [`Ui::set_focus`](crate::Ui::set_focus).
    #[inline]
    pub(crate) const fn set_focus(&mut self, id: WidgetId) {
        self.move_focus(Some(id));
    }

    /// Leave nothing focused. See [`Ui::clear_focus`](crate::Ui::clear_focus).
    #[inline]
    pub(crate) const fn clear_focus(&mut self) {
        self.move_focus(None);
    }

    /// Move focus. Does not re-route this pass; see [`Scopes`].
    const fn move_focus(&mut self, id: Option<WidgetId>) {
        self.focus.move_to(id, FocusCause::Program);
    }

    pub(crate) fn withdraw_input_scope(&mut self, owner: WidgetId) {
        self.scopes.close(owner);
    }

    /// The state a warmup pass records against: no input, but this state's focus. It takes the
    /// arranged frame, which the pass fills, and [`Self::adopt_warmup`] brings it back.
    pub(crate) fn warmup_scratch(&mut self) -> Self {
        Self {
            arranged: mem::take(&mut self.arranged),
            focus: self.focus.warmup_scratch(),
            ..Self::new()
        }
    }

    /// Take back the arranged frame a warmup pass filled, and keep the focus moves and scope
    /// withdrawals it recorded.
    pub(crate) fn adopt_warmup(&mut self, warmup: Self) {
        self.arranged = warmup.arranged;
        self.focus.adopt_warmup(&warmup.focus);
        self.scopes.adopt_closing(&warmup.scopes);
    }

    pub(crate) const fn watch_pointer(&mut self, flags: PointerWake) {
        self.watches.pointer_mask.insert(flags);
    }

    /// Declare interest in off-focus keyboard categories, as
    /// [`Ui::keyboard_events`](crate::Ui::keyboard_events) does.
    #[inline]
    pub(crate) const fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.watches.keyboard_mask.insert(flags);
    }

    pub(crate) fn watch_shortcut(&mut self, shortcut: Shortcut) {
        self.watches.watch_shortcut(shortcut);
    }

    /// The raw keyboard stream as seen from `reader`'s plane; plane-gated only, to keep arrival
    /// order.
    pub(crate) fn keyboard_events_at(&self, reader: Plane) -> &[KeyPress] {
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
    /// `keyboard_events`. Watches bypass hit-testing, so without this a
    /// `Main`-layer `SCROLL` watcher would keep receiving events under a modal.
    pub(crate) fn pointer_events_at(&self, reader: Plane) -> &[PointerEvent] {
        if self.silenced(reader) {
            return &[];
        }
        &self.pointer.events
    }

    /// Whether `shortcut` was pressed and granted to the scope `reader`'s parent sits in.
    pub(crate) fn shortcut_pressed_at(&mut self, reader: KeyReader, shortcut: Shortcut) -> bool {
        self.watches.watch_shortcut(shortcut);
        self.granted_press(reader, |press| shortcut.matches(press))
    }

    /// Whether `key` was pressed, whatever the modifiers, and granted to the scope `reader`'s
    /// parent sits in.
    pub(crate) fn key_pressed_at(&mut self, reader: KeyReader, key: Key) -> bool {
        self.watches.watch_key(key);
        self.granted_press(reader, |press| press.key.same_key(key))
    }

    /// Whether a press of `class` came this frame, whatever its key and modifiers, and is granted
    /// to the scope `reader`'s parent sits in: what a scope that takes `class` acts on, as Shift
    /// and Space are still [`KeyClass::Space`]. Watches nothing, as its reader is focused.
    pub(crate) fn class_pressed_at(&mut self, reader: KeyReader, class: KeyClass) -> bool {
        self.granted_press(reader, |press| press.class() == class)
    }

    /// Whether a press this frame passes `matches` and is granted to `reader`'s scope.
    fn granted_press(&mut self, reader: KeyReader, matches: impl Fn(KeyPress) -> bool) -> bool {
        if self.keyboard.presses.is_empty() || self.silenced(reader.plane) {
            return false;
        }
        let scope = self.scopes.reader(reader.parent, &self.arranged.cascade);
        self.keyboard
            .presses
            .iter()
            .any(|press| matches(*press) && self.scopes.reaches(press.class(), scope))
    }

    /// Whether `key` is down now, whatever the modifiers. A fact of the keyboard, as the
    /// modifiers are, so not routed.
    pub(crate) fn key_held(&mut self, key: Key) -> bool {
        self.watches.watch_key(key);
        self.keyboard.is_held(key)
    }

    /// Whether `key` came up this frame, or was held when the surface lost the keyboard.
    pub(crate) fn key_released(&mut self, key: Key) -> bool {
        self.watches.watch_key(key);
        self.keyboard.was_released(key)
    }

    /// Every edge the pointer produced this frame, widget by widget, from the same
    /// capture state as [`Ui::response_for`](crate::Ui::response_for). Three slots per button, at most two
    /// filled (`InputQueue` admits one press-or-release per button per frame). A widget
    /// the arranged frame holds disabled, itself or by an ancestor, has none, as its response is
    /// empty: the press still latched on it, so nothing behind it takes the input.
    #[inline]
    pub(crate) fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_ {
        let cascade = &self.arranged.cascade;
        let disabled = move |id| {
            cascade
                .endpoint(id)
                .is_some_and(|at| cascade.entry(at).disabled)
        };
        PointerButton::ALL.into_iter().flat_map(move |button| {
            self.pointer
                .capture(button)
                .edges(button)
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
        if !self.watches.pointer_mask.contains(sense) {
            return false;
        }
        if let Some(pos) = pos {
            self.pointer.events.push(make(pos));
        }
        true
    }

    /// Push `Leave`, which belongs to no single watch class and carries no position.
    fn push_unclassed(&mut self, event: PointerEvent) -> bool {
        if self.watches.pointer_mask.is_empty() {
            return false;
        }
        self.pointer.events.push(event);
        true
    }

    /// Accumulate one scroll delta on the current targets and wake watchers. Each unit
    /// travels in its own lane (see [`ScrollDelta`]).
    fn on_scroll(&mut self, wheel: WheelDelta) -> EventOutcome {
        let mut delivered = false;
        for share in self
            .pointer
            .scroll_targets
            .route(wheel)
            .into_iter()
            .flatten()
        {
            let delta = self.pointer.delta_mut(share.target);
            delta.wheel += share.delta;
            delivered = true;
        }
        let watched = self.push_positioned(PointerWake::SCROLL, |pos| PointerEvent::Scroll {
            pos,
            delta: wheel,
        });
        if delivered || watched {
            self.queue.note_delta();
        }
        EventOutcome::repaint(delivered || watched)
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

    /// Feed a palantir-native input event, hit-tested against this frame's latest cascade;
    /// `double_click` is how soon a second press on the same target chains a run, which the host
    /// shares among its windows. Refused: a field on `Input`, which cannot read the settings, so
    /// each entry would copy the value first, state synced by hand. `now` is the arrival time on
    /// the host's clock (see [`WindowFrames::on_input`](crate::ui::window_frames::WindowFrames::on_input)).
    pub(crate) fn on_input(
        &mut self,
        event: InputEvent<'_>,
        double_click: Duration,
        now: Duration,
    ) -> InputDelta {
        if !event.is_valid() {
            return InputDelta::default();
        }
        if self.queue.is_empty() {
            if self.ready(&event) {
                return self.apply(event, double_click, now);
            }
            self.wait_out_lost_release(&event);
        }
        if self.queue.is_full() {
            while let Some(held) = self.queue.front() {
                self.queue.pop_front();
                self.cut_held_press(&held.event);
                self.apply_held(held, double_click);
            }
            self.cut_held_press(&event);
            self.apply(event, double_click, now);
        } else {
            self.queue.defer(event, now);
        }
        InputDelta {
            repaint_requested: true,
        }
    }

    /// Before a flush applies `event` unready ([`InputQueue::is_full`]): a press after a lost
    /// release cuts its gesture off, as [`Self::wait_out_lost_release`] does, though the cut and
    /// the press then share a frame, so the cut notes no edge of its own.
    fn cut_held_press(&mut self, event: &InputEvent<'_>) {
        if let Some(button) = self.lost_release(event) {
            self.pointer.capture_mut(button).abandon_press();
        }
    }

    /// The button whose release `event` shows was lost: a press of a button still held.
    const fn lost_release(&self, event: &InputEvent<'_>) -> Option<PointerButton> {
        match *event {
            InputEvent::PointerPressed(button) if self.pointer.capture(button).is_pressed() => {
                Some(button)
            }
            _ => None,
        }
    }

    /// End `button`'s held press as a cut-off gesture, not as a release that hit-tests a click
    /// nobody made; its end is an edge of the frame it lands in.
    fn cut_gesture(&mut self, button: PointerButton) {
        self.pointer.capture_mut(button).abandon_press();
        self.queue.note_capture(button);
    }

    /// Where `event`, which [`Self::ready`] refused, shows a lost release: cut that gesture now,
    /// so the frame the press waits for sees its end first.
    fn wait_out_lost_release(&mut self, event: &InputEvent<'_>) {
        if self.queue.admits(event)
            && let Some(button) = self.lost_release(event)
        {
            self.cut_gesture(button);
            self.signal_since_last_frame.raise(InputSignal::Repaint);
        }
    }

    /// Apply `held`, its text read back from the queue.
    fn apply_held(&mut self, held: HeldEvent, double_click: Duration) {
        if held.event.text().is_none() {
            self.apply(held.event, double_click, held.at);
            return;
        }
        let mut text = mem::take(&mut self.held_text);
        text.clear();
        text.push_str(self.queue.text(held.text));
        self.apply(held.event.with_text(&text), double_click, held.at);
        self.held_text = text;
    }

    /// Whether `event` applies this frame (see [`InputQueue`]): the queue admits it, and it is no
    /// press after a lost release, which waits a frame so its gesture's end is seen.
    fn ready(&self, event: &InputEvent<'_>) -> bool {
        self.queue.admits(event) && self.lost_release(event).is_none()
    }

    /// Admit the events the queue held for the frame starting now, a frame's worth, in order.
    /// Returns whether input owes the next frame.
    pub(crate) fn admit_held(&mut self, double_click: Duration) -> bool {
        self.queue.next_frame();
        // An eviction's release edge (see `settle`) belongs to the frame starting now, so that
        // button's next change waits a frame, as after any release.
        for button in PointerButton::ALL {
            if self.pointer.capture(button).has_edge() {
                self.queue.note_capture(button);
            }
        }
        while let Some(held) = self.queue.front() {
            if !self.ready(&held.event) {
                self.wait_out_lost_release(&held.event);
                break;
            }
            self.queue.pop_front();
            self.apply_held(held, double_click);
        }
        self.signal_since_last_frame != InputSignal::None || !self.queue.is_empty()
    }

    /// Apply `event`, which the queue admitted, and say whether it asks for a frame; each kind of
    /// event goes to its handler.
    fn apply(
        &mut self,
        event: InputEvent<'_>,
        double_click: Duration,
        now: Duration,
    ) -> InputDelta {
        // A host event that passed the screen above is at least `Inert`, enough to force
        // a record under `InputPolicy::Always`; a refused event mutates nothing.
        self.signal_since_last_frame.raise(InputSignal::Inert);
        let outcome = match event {
            InputEvent::PointerMoved(p) => self.on_pointer_moved(p),
            InputEvent::PointerLeft => self.on_pointer_left(),
            InputEvent::PointerPressed(btn) => self.on_pointer_pressed(btn, double_click, now),
            InputEvent::PointerReleased(btn) => self.on_pointer_released(btn),
            InputEvent::Scroll(wheel) => self.on_scroll(wheel),
            InputEvent::Pinch(factor) => self.on_pinch(factor),
            InputEvent::KeyDown {
                key,
                repeat,
                physical,
                text,
            } => self.on_key_down(KeyPress {
                key,
                mods: self.keyboard.modifiers,
                repeat,
                physical,
                text,
            }),
            InputEvent::ImeCommit(text) => self.on_ime_commit(text),
            InputEvent::ImePreedit(preedit) => {
                self.ime.compose(preedit, self.focus.focused());
                EventOutcome::repaint(self.ime.owner.is_some())
            }
            InputEvent::KeyUp { key } => {
                if !self.keyboard.release(key) {
                    return InputDelta::default();
                }
                EventOutcome::repaint(self.watches.watches_key(key))
            }
            InputEvent::SurfaceFocusLost => self.on_surface_focus_lost(),
            InputEvent::ModifiersChanged(m) => {
                self.keyboard.modifiers = m;
                EventOutcome::repaint(self.watches.keyboard_mask.contains(KeyboardWake::MODIFIER))
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

    fn on_pointer_moved(&mut self, p: Vec2) -> EventOutcome {
        let prev_hover = self.pointer.under_pointer;
        let prev_scroll = self.pointer.scroll_targets;
        let prev_pinch = self.pointer.pinch_target;
        let latched = self.pointer.move_to(p);
        self.refresh_pointer_targets();
        let move_watched = self.push_pointer_event(PointerWake::MOVE, Some(p), PointerEvent::Moved);
        EventOutcome {
            repaint: self.pointer.under_pointer != prev_hover
                || self.pointer.scroll_targets != prev_scroll
                || self.pointer.pinch_target != prev_pinch
                || self.pointer.any_press()
                || move_watched,
            settles: latched,
        }
    }

    fn on_pointer_left(&mut self) -> EventOutcome {
        let observable = self.pointer.under_pointer.is_some()
            || self.pointer.scroll_targets.any()
            || self.pointer.pinch_target.is_some()
            || self.pointer.any_press();
        self.pointer.pos = None;
        self.refresh_pointer_targets();
        let pointer_watched = self.push_unclassed(PointerEvent::Left);
        EventOutcome::repaint(observable || pointer_watched)
    }

    fn on_pointer_pressed(
        &mut self,
        btn: PointerButton,
        double_click: Duration,
        now: Duration,
    ) -> EventOutcome {
        let pointer_pos = self.pointer.pos;
        let targets = pointer_pos.map(|p| self.arranged.cascade.hit_test_press(p));
        let hit = targets.and_then(|t| t.click);
        let buttons_watched = self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
            PointerEvent::Pressed { pos, button: btn }
        });
        // Any press breaks every other button's multi-click run.
        for (idx, cap) in self.pointer.captures.iter_mut().enumerate() {
            if idx != btn.idx() {
                cap.break_run();
            }
        }
        let cap = self.pointer.capture_mut(btn);
        debug_assert!(
            !cap.is_pressed(),
            "a press of a held button ends the lost capture before it applies",
        );
        match hit.zip(pointer_pos) {
            Some((target, pos)) => {
                cap.begin_press(target, pos, now, double_click);
                self.queue.note_capture(btn);
            }
            None => cap.break_run(),
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
            repaint: hit.is_some() || self.focus.focused() != prev_focus || buttons_watched,
            settles: buttons_watched,
        }
    }

    fn on_pointer_released(&mut self, btn: PointerButton) -> EventOutcome {
        let pointer_pos = self.pointer.pos;
        let was_captured = self.pointer.capture(btn).is_pressed();
        if was_captured {
            self.queue.note_capture(btn);
        }
        let cascade = &self.arranged.cascade;
        let kind = self
            .pointer
            .capture_mut(btn)
            .release_on(|| cascade.hit_test_press(pointer_pos?).click);
        // A `Miss` tears down a capture only one widget reads, which the settle rule
        // excludes; `Click` and `DragStopped` are edges apps act on, so they settle.
        let settles = kind.is_some_and(|kind| kind != ReleaseKind::Miss);
        let buttons_watched = self.push_pointer_event(PointerWake::BUTTONS, pointer_pos, |pos| {
            PointerEvent::Released { pos, button: btn }
        });
        EventOutcome {
            repaint: was_captured || buttons_watched,
            settles: settles || buttons_watched,
        }
    }

    fn on_pinch(&mut self, factor: ZoomFactor) -> EventOutcome {
        let target = self.pointer.pinch_target;
        if let Some(target) = target {
            let delta = self.pointer.delta_mut(target);
            delta.zoom = delta.zoom.combine(factor);
        }
        let watched = self.push_positioned(PointerWake::PINCH, |pos| PointerEvent::Pinch {
            pos,
            factor,
        });
        if target.is_some() || watched {
            self.queue.note_delta();
        }
        EventOutcome::repaint(target.is_some() || watched)
    }

    fn on_key_down(&mut self, kp: KeyPress) -> EventOutcome {
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
        let traverses =
            kp.class() == KeyClass::Focus && !self.arranged.cascade.tab_stops.is_empty();
        let observable = (self.focus.focused().is_some() && !bare_modifier)
            || traverses
            || self.watches.matches_press(kp)
            || self.watches.keyboard_mask.contains(KeyboardWake::KEY);
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
                .any(|held| self.watches.watches_key(held.key));
        self.keyboard.lose_surface();
        for button in PointerButton::ALL {
            if self.pointer.capture(button).is_pressed() {
                self.cut_gesture(button);
            }
        }
        // The input method's composition belongs to the surface that had the keyboard.
        self.ime.retire();
        EventOutcome::repaint(observable)
    }

    /// Read and reset [`Self::frame_had_action`]; [`WindowFrames::frame`](crate::ui::window_frames::WindowFrames::frame) uses it to decide on a
    /// discarded pre-pass.
    pub(crate) fn take_action_flag(&mut self) -> bool {
        mem::take(&mut self.frame_had_action)
    }

    /// Clear this frame's edges and signals without touching cascade-dependent state, so a
    /// second pass sees no click fire twice.
    pub(crate) fn clear_edges(&mut self) {
        self.pointer.end_frame();
        self.keyboard.end_frame();
        self.signal_since_last_frame = InputSignal::None;
        self.frame_had_action = false;
    }

    /// Re-resolve `hovered` / `scroll_targets` / `pinch_target` from `pointer_pos`;
    /// the single owner of that assignment. Also routes the held pointer after
    /// cold-start warmup, whose pre-frame events hit an empty cascade.
    pub(crate) fn refresh_pointer_targets(&mut self) {
        self.pointer.retarget(&self.arranged.cascade);
    }

    /// Once-per-frame close-out after the final record pass: recompute hover, drop transient flags,
    /// evict captured widgets that left the tree.
    pub(crate) fn settle(&mut self) {
        self.clear_edges();
        self.scopes.end_frame();
        // `modifiers` persists: a held shift must stay `true` across frames.
        //
        // Eviction ends a capture through the same call a release does; dropping the
        // press alone would skip `Drag::Stopped`, losing the commit of `Slider` and
        // `DragValue`.
        for cap in &mut self.pointer.captures {
            let vanished = cap
                .pressed_on()
                .is_some_and(|target| !self.arranged.cascade.contains(target));
            if vanished {
                cap.abandon_press();
                // The release edge belongs to the next frame; raise the signal so it records
                // instead of dropping the edge.
                self.signal_since_last_frame.raise(InputSignal::Repaint);
            }
        }
        if self.focus.reconcile(&self.arranged.cascade) {
            // The next frame records against the new focus (ring, scope path), so it must not
            // repaint the retained tree.
            self.signal_since_last_frame.raise(InputSignal::Repaint);
        }
        // A composition hidden by a focus move would come back when focus does.
        if self.ime.owner != self.focus.focused() {
            self.ime.retire();
        }
        self.refresh_pointer_targets();
    }

    /// True when focus sits on `ancestor` or in its subtree, per the arranged frame.
    #[inline]
    pub(crate) fn is_focus_within(&self, ancestor: WidgetId) -> bool {
        self.focus
            .focused()
            .is_some_and(|focused| self.arranged.cascade.is_within(focused, ancestor))
    }

    /// True when the pointer's hover target is `ancestor` or in its subtree, per the arranged
    /// frame.
    #[inline]
    pub(crate) fn is_hover_within(&self, ancestor: WidgetId) -> bool {
        self.hovered()
            .is_some_and(|hovered| self.arranged.cascade.is_within(hovered, ancestor))
    }

    fn scroll_delta_for(&self, id: WidgetId) -> ScrollDelta {
        self.pointer.delta(id).copied().unwrap_or_default()
    }

    /// Snapshot into [`Self::frame_quiescent`] whether any widget can hold
    /// non-default interaction state. `focused` is excluded because
    /// [`crate::Ui::set_focus`] can set it mid-record. The pointer test covers the
    /// routed targets ([`Self::refresh_pointer_targets`] clears them when `pointer_pos`
    /// is `None`), so the fast path opens only while the pointer is off the surface.
    fn snapshot_frame_quiescent(&mut self) {
        debug_assert!(
            self.pointer.pos.is_some()
                || (self.pointer.under_pointer.is_none()
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
                .all(|c| !c.is_pressed() && !c.has_edge());
    }

    /// `id`'s response, where the arranged frame holds it, which the reading pass resolved at
    /// `resolved_at` ([`Cascade::endpoint_at`](crate::cascade::Cascade::endpoint_at)).
    pub(crate) fn response(&self, id: WidgetId, resolved_at: usize) -> Response {
        let at = self.arranged.cascade.endpoint_at(id, resolved_at);
        // Geometry half, needed every frame. `entries` is AoS so the fields share a cache line.
        let entry = at.map(|at| *self.arranged.cascade.entry(at));
        let rect = entry.map(|e| e.rect);
        let layout_rect = at.map(|at| self.arranged.layout.arranged_rect(at));
        let transform = entry.map_or(TranslateScale::IDENTITY, |e| e.transform);
        // Cascade flattens parent-disabled into each entry: effective ancestor-or-self disabled,
        // one frame stale.
        let disabled = entry.is_some_and(|e| e.disabled);

        // Built once so a new field cannot be filled on one path and defaulted on the other.
        // `focused` is read live since `set_focus` can run after `frame_quiescent`.
        let mut state = Response {
            rect,
            layout_rect,
            transform,
            disabled,
            focused: self.focus.focused() == Some(id),
            ..Response::idle(id)
        };

        // On a quiescent frame every remaining field is default; skip the capture scan and
        // scroll/zoom lookups.
        if self.frame_quiescent {
            return state;
        }

        state.pointer_over = self.pointer.hovered() == Some(id);

        // One slice per button, each with its own drag.
        for btn in PointerButton::ALL {
            *state.button_mut(btn) = self.pointer.capture(btn).button_state(id, transform);
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
impl Input {
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
    use crate::input::Input;
    use crate::primitives::identity::widget_id::WidgetId;
    use crate::primitives::input::scroll_targets::ScrollTargets;

    /// Routing state the widget suites assert on.
    impl Input {
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
