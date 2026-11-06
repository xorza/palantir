//! [`UpdateCtx`]: what [`App::update`](crate::App::update) holds of a window.

use crate::clipboard::Clipboard;
use crate::display::Display;
use crate::image_registry::error::ImageTooLarge;
use crate::image_registry::image_handle::ImageHandle;
use crate::input::Input;
use crate::input::key_reader::KeyReader;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_press::KeyPress;
use crate::primitives::input::pointer::PointerEvent;
use crate::primitives::input::shortcut::Shortcut;
use crate::primitives::input::watch::{KeyboardWake, PointerWake};
use crate::primitives::paint::image::Image;
use crate::theme::Theme;
use crate::ui::Ui;
use crate::ui::host_seam::HostSeam;
use crate::ui::resources::UiResources;
use crate::ui::state::StateMap;
use crate::window::screenshot::{Screenshot, ScreenshotId};
use crate::window::window_geometry::WindowGeometry;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

/// What [`App::update`](crate::App::update) has of a window: its [`Input`], its widgets' state
/// rows, the host's facts, the shared resources, the theme, the display and the time. A view of
/// disjoint parts of the [`Ui`], which holds them for the record passes, as Masonry's `EventCtx`
/// holds references to what each pass shares. Each read's logic sits on the part that owns it, so
/// this view and the `Ui` both call it. Refused: a `&Ui` beside it with [`Input`] moved out for the
/// hook, where `ui.focus()` reads an empty input with no error; `Ui` over interior mutability, as
/// egui's `Context` is, a lock on each read; `Input<'a>` over `&mut Ui` with `Deref`, a lifetime in
/// each helper and 15 forwards; `&mut Ui` with a panic, as GPUI's `debug_assert_paint` checks its
/// phases, which compiles wrong code; the state rows on `Input`, whose logic never reads them; a
/// `Ui` of references too, which changes about 1500 signatures for no gain to a caller.
///
/// A read that a scope routes reads from where the view was made: the window's root in
/// [`App::update`](crate::App::update), so an overlay's claim silences it, and the record position
/// through [`Ui::update_ctx`], so a helper reads inside an overlay it records in.
///
/// It holds no part that records, so a stage that has only this cannot record:
///
/// ```compile_fail,E0308
/// # use palantir::{Button, UpdateCtx};
/// fn input(ctx: &mut UpdateCtx<'_>) {
///     Button::new().show(ctx);
/// }
/// ```
#[derive(Debug)]
pub struct UpdateCtx<'a> {
    /// This frame's input. A helper that reads nothing a scope routes takes this alone.
    pub input: &'a mut Input,
    state: &'a mut StateMap,
    host: &'a HostSeam,
    resources: &'a UiResources,
    reader: KeyReader,
    theme: &'a Rc<Theme>,
    display: Display,
    now: Duration,
    record_frame_id: u64,
}

impl<'a> UpdateCtx<'a> {
    /// The view of `ui`, from its parts.
    pub(super) fn of(ui: &'a mut Ui) -> Self {
        Self {
            reader: ui.key_reader(),
            display: ui.frame_runtime.stamp().display,
            now: ui.frame_runtime.stamp().time,
            record_frame_id: ui.frame_runtime.record_frame_id,
            theme: ui.window_theme.shown(),
            input: &mut ui.input,
            state: &mut ui.memory.state,
            host: &ui.host,
            resources: &ui.resources,
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

    /// The theme this frame shows. See [`Ui::theme`].
    #[inline]
    pub const fn theme(&self) -> &Rc<Theme> {
        self.theme
    }

    /// Which recorded frame this is, which its record passes read too. See
    /// [`Ui::record_frame_id`].
    #[inline]
    pub const fn record_frame_id(&self) -> u64 {
        self.record_frame_id
    }

    /// Whether the window manager asked this window to close this frame. See
    /// [`Ui::close_requested`].
    pub const fn close_requested(&self) -> bool {
        self.host.close_requested()
    }

    /// The image `id` asked for, once it landed. See [`Ui::screenshot_taken`].
    pub fn screenshot_taken(&self, id: ScreenshotId) -> Option<&Screenshot> {
        self.host.screenshot_taken(id)
    }

    /// The window's geometry. See [`Ui::window_geometry`].
    pub fn window_geometry(&self) -> WindowGeometry {
        self.host.window_geometry(self.display)
    }

    /// A handle on the host's clipboard. See [`Ui::clipboard`].
    pub fn clipboard(&self) -> Clipboard {
        self.resources.clipboard().clone()
    }

    /// Load `image`. See [`Ui::load_image`].
    pub fn load_image(&self, image: &Image) -> Result<ImageHandle, ImageTooLarge> {
        self.resources.load_image(image)
    }

    /// The longest image side the device holds. See [`Ui::max_image_dimension`].
    pub fn max_image_dimension(&self) -> Option<NonZeroU32> {
        self.resources.max_image_dimension()
    }

    /// Monotonic time of this frame. See [`Ui::now`].
    #[inline]
    pub const fn now(&self) -> Duration {
        self.now
    }
}
