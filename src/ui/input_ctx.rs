//! [`InputCtx`]: what the input stage of a frame holds.

use crate::display::Display;
use crate::input::Input;
use crate::input::key_reader::KeyReader;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_press::KeyPress;
use crate::primitives::input::pointer::PointerEvent;
use crate::primitives::input::shortcut::Shortcut;
use crate::primitives::input::watch::{KeyboardWake, PointerWake};
use crate::ui::Ui;
use crate::ui::state::StateMap;
use std::time::Duration;

/// What [`App::input`](crate::App::input) has of a window: its [`Input`], its widgets' state rows,
/// the display and the time. A view of disjoint parts of the [`Ui`], which holds them for the record
/// passes, as Masonry's `EventCtx` holds references to what each pass shares. Refused: the state
/// rows on `Input`, whose logic never reads them; a `Ui` of references too, which changes each of
/// its signatures for no gain to a caller.
///
/// A read that a scope routes reads from where the view was made: the window's root in
/// [`App::input`](crate::App::input), so an overlay's claim silences it, and the record position
/// through [`Ui::input_ctx`], so a helper reads inside an overlay it records in.
///
/// It holds no part that records, so a stage that has only this cannot record:
///
/// ```compile_fail,E0308
/// # use palantir::{Button, InputCtx};
/// fn input(ctx: &mut InputCtx<'_>) {
///     Button::new().show(ctx);
/// }
/// ```
#[derive(Debug)]
pub struct InputCtx<'a> {
    /// This frame's input. A helper that reads nothing a scope routes takes this alone.
    pub input: &'a mut Input,
    state: &'a mut StateMap,
    reader: KeyReader,
    display: Display,
    now: Duration,
}

impl<'a> InputCtx<'a> {
    /// The view of `ui`, from its parts.
    pub(super) fn of(ui: &'a mut Ui) -> Self {
        Self {
            reader: ui.key_reader(),
            display: ui.frame_runtime.stamp().display,
            now: ui.frame_runtime.stamp().time,
            input: &mut ui.input,
            state: &mut ui.memory.state,
        }
    }

    /// `true` if `key` went down this frame, whatever the modifiers, and the press reaches this
    /// view's reader. See [`Ui::key_pressed`].
    pub fn key_pressed(&mut self, key: Key) -> bool {
        self.input.key_pressed_at(self.reader, key)
    }

    /// `true` if `shortcut` was pressed this frame and the press reaches this view's reader. See
    /// [`Ui::shortcut_pressed`].
    pub fn shortcut_pressed(&mut self, shortcut: impl Into<Shortcut>) -> bool {
        self.input.shortcut_pressed_at(self.reader, shortcut.into())
    }

    /// This frame's pointer events, and a watch of `wake`. See [`Ui::pointer_events`].
    pub fn pointer_events(&mut self, wake: PointerWake) -> &[PointerEvent] {
        self.input.watch_pointer(wake);
        self.input.pointer_events_at(self.reader.plane)
    }

    /// This frame's presses, in arrival order, and a watch of `wake`. See
    /// [`Ui::keyboard_events`].
    pub fn keyboard_events(&mut self, wake: KeyboardWake) -> &[KeyPress] {
        self.input.watch_keyboard(wake);
        self.input.keyboard_events_at(self.reader.plane)
    }

    /// The cross-frame state row for `id`, or `None`. See [`Ui::state`].
    ///
    /// # Panics
    ///
    /// While the row is lent to a [`Ui::with_state`] body.
    #[track_caller]
    pub fn state<S: 'static>(&self, id: WidgetId) -> Option<&S> {
        self.state.try_get::<S>(id)
    }

    /// Store `value` as `id`'s state. See [`Ui::set_state`].
    ///
    /// # Panics
    ///
    /// While the row is lent to a [`Ui::with_state`] body.
    #[track_caller]
    pub fn set_state<S: 'static>(&mut self, id: WidgetId, value: S) {
        self.state.set(id, value);
    }

    /// Remove `id`'s state and return it. See [`Ui::remove_state`].
    ///
    /// # Panics
    ///
    /// While the row is lent to a [`Ui::with_state`] body.
    #[track_caller]
    pub fn remove_state<S: 'static>(&mut self, id: WidgetId) -> Option<S> {
        self.state.remove::<S>(id)
    }

    /// Active `Display`. See [`Ui::display`].
    #[inline]
    pub const fn display(&self) -> Display {
        self.display
    }

    /// Monotonic time of this frame. See [`Ui::now`].
    #[inline]
    pub const fn now(&self) -> Duration {
        self.now
    }
}
