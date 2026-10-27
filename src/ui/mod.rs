//! The recorder: [`Ui`], the handle a widget or host authors a frame through,
//! and the retained state that frame reads and writes.

#[cfg(feature = "internals")]
pub(crate) mod bench;
mod frame_cycle;
pub(crate) mod frame_engines;
pub(crate) mod frame_input;
pub(crate) mod frame_report;
pub(crate) mod frame_runtime;
mod frame_stats_readout;
pub(crate) mod layer_scope;
pub(crate) mod resources;
pub(crate) mod screenshot;
pub(crate) mod singletons;
pub(crate) mod state;

use crate::animation::AnimationMap;
use crate::animation::animation_map_typed::AnimationClock;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_spec::AnimationSpec;
use crate::app::App;
use crate::cascade::Cascade;
use crate::cascade::WidgetLocation;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::diagnostics::frame_stats::FrameStats;
use crate::display::Display;
use crate::display::frame_stamp::FrameStamp;
use crate::display::user_scale::UserScale;
use crate::icons::icon_set::IconSet;
use crate::icons::icon_table::IconTable;
use crate::image_registry::image_handle::ImageHandle;
use crate::input::ime_preedit::ImePreedit;
use crate::input::input_event::InputEvent;
use crate::input::interaction::input_delta::InputDelta;
use crate::input::interaction::pointer_action::PointerAction;
use crate::input::interaction::response_state::ResponseState;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerEvent;
use crate::input::policy::FocusPolicy;
use crate::input::policy::InputPolicy;
use crate::input::shortcut::Shortcut;
use crate::input::watch::{KeyboardWake, PointerWake};
use crate::input_state::InputState;
use crate::input_state::arranged_frame::ArrangedFrame;
use crate::input_state::key_reader::KeyReader;
use crate::layout::Layout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::layout_mode::{GridDefId, ScrollbarsDefId};
use crate::primitives::layout::track::Track;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::text::interned_str::InternedStr;
use crate::primitives::text::text_input::TextInput;
use crate::renderer::error::ImageTooLarge;
use crate::renderer::frontend::FrameScene;
use crate::renderer::gpu_paint::gpu_views::{GpuViews, RecordedView};
use crate::scene::forest::{Forest, NodeChrome};
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::scene::seen_ids::{LastFrame, ResolvedId};
use crate::shape::Lower;
use crate::shape::paint_animation::PaintAnimation;
use crate::shape::record_store::RecordStore;
use crate::text::error::FontLoadError;
use crate::text::font_family::FontFamily;
use crate::text::font_source::FontSource;
use crate::text::probe::TextProbe;
use crate::text::run::TextRun;
use crate::theme::Theme;
use crate::ui::frame_cycle::FrameCycle;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_input::FrameInput;
use crate::ui::frame_report::FrameReport;
use crate::ui::frame_runtime::FrameRuntime;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::layer_scope::LayerScope;
use crate::ui::resources::UiResources;
use crate::ui::screenshot::Screenshot;
use crate::ui::singletons::Singletons;
use crate::ui::state::StateMap;
use crate::window::cursor_icon::CursorIcon;
use crate::window::vsync::Vsync;
use crate::window::window_commands::WindowCommands;
use crate::window::window_config::WindowConfig;
use crate::window::window_directory::WindowDirectory;
use crate::window::window_frame_state::WindowFrameState;
use crate::window::window_geometry::WindowGeometry;
use crate::window::window_output::WindowOutput;
use crate::window::window_requests::WindowRequests;
use crate::window::window_token::WindowToken;
use glam::{UVec2, Vec2};
use std::fmt;
use std::mem;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

/// Recorder + input/response broker. Coordinates are logical pixels;
/// `Display::scale_factor` converts to physical at the wgpu boundary. Fields are
/// private so widgets, host and encoder share one method surface.
#[derive(Debug)]
pub struct Ui {
    forest: Forest,
    /// Refcounted so a widget can hold the theme across the `&mut Ui` its `show`
    /// takes; cloning the bundle per widget per frame would be a large copy.
    theme: Rc<Theme>,
    /// Cross-frame widget state, keyed by `WidgetId`.
    state: StateMap,
    /// State shared across all instances of a widget kind.
    singletons: Singletons,
    /// Live `GpuView`s. The shape records only the redraw epoch; the encoder
    /// looks the view up by the node's `WidgetId`. Swept with [`StateMap`].
    gpu_views: GpuViews,
    resources: UiResources,
    layout: Layout,
    /// Cascaded clip/disabled/invisible/transform per node plus the hit index.
    /// Written in the paint phase; read by the encoder, input dispatch and damage.
    cascade: Cascade,
    /// Whether [`Self::cascade`] is the last painted frame's; only then does
    /// [`SeenIds::last_frame_endpoint`](crate::scene::seen_ids::SeenIds::last_frame_endpoint)
    /// name a row of it.
    cascade_is_last_frame: bool,
    input: InputState,
    display: Display,
    animation: AnimationMap,
    /// Frame clock, wake queue, repaint/relayout flags and prior-frame validity.
    frame_runtime: FrameRuntime,
    /// Recorder-to-host requests retained across frames.
    window_requests: WindowRequests,
    /// Host-to-recorder facts refreshed before each windowed frame.
    window_frame: WindowFrameState,
    /// The logical region this frame asked an image of, clamped to the window; the driver takes
    /// it once the frame is recorded.
    screenshot_request: Option<Rect>,
    /// The image that landed for this frame, if one did; replaced or cleared before every frame.
    /// Boxed: a rare, cold value kept out of the fields every pass walks (`hot_struct_sizes`).
    screenshot_taken: Option<Box<Screenshot>>,
}

/// Widget- and host-facing authoring API; mostly one-line delegations to sealed subsystems.
impl Ui {
    pub(crate) const fn frame_scene(&self) -> FrameScene<'_> {
        FrameScene {
            forest: &self.forest,
            layout: &self.layout,
            cascade: &self.cascade,
            gpu_views: &self.gpu_views,
            stamp: FrameStamp::new(self.display, self.frame_runtime.time),
        }
    }

    /// Construct a per-window `Ui`; each owns its own [`Forest`].
    pub(crate) fn new(resources: UiResources) -> Self {
        Self {
            resources,
            forest: Forest::default(),
            theme: Rc::default(),
            state: StateMap::default(),
            singletons: Singletons::default(),
            gpu_views: GpuViews::default(),
            layout: Layout::default(),
            cascade: Cascade::default(),
            cascade_is_last_frame: false,
            input: InputState::default(),
            display: Display::default(),
            animation: AnimationMap::default(),
            frame_runtime: FrameRuntime::default(),
            window_requests: WindowRequests::default(),
            window_frame: WindowFrameState::default(),
            screenshot_request: None,
            screenshot_taken: None,
        }
    }

    /// The active theme. `.clone()` is a refcount bump, so a style can outlive the
    /// `show(ui)` reborrow:
    ///
    /// ```
    /// # use palantir::{Button, Ui};
    /// # fn file_button(ui: &mut Ui) {
    /// let theme = ui.theme().clone();
    /// Button::new().label("File").style(&theme.button).show(ui);
    /// # }
    /// ```
    #[inline]
    pub const fn theme(&self) -> &Rc<Theme> {
        &self.theme
    }

    /// Replace the whole theme; takes an `Rc` so swapping prebuilt themes copies nothing.
    #[inline]
    pub fn set_theme(&mut self, theme: impl Into<Rc<Theme>>) {
        self.theme = theme.into();
    }

    /// Drive one application frame for `input.window` via [`FrameCycle`]. Pass the
    /// same `engines` every frame; a fresh set discards the measure cache, cascade and
    /// damage baseline, forcing a full repaint.
    pub(crate) fn frame<T: App>(
        &mut self,
        engines: &mut FrameEngines,
        input: FrameInput,
        app: &mut T,
    ) -> FrameReport {
        FrameCycle::new(self, engines).run(input, app)
    }

    /// The scale the current cascade was laid out at; a host divides event
    /// positions by it. `None` before the first frame.
    #[cfg(any(test, feature = "winit"))]
    pub(crate) fn laid_out_scale(&self) -> Option<f32> {
        self.frame_runtime
            .prev_stamp
            .map(|stamp| stamp.display.scale_factor())
    }

    /// Feed an event that arrived at `now`. The winit host stamps it on an input clock
    /// of its own, which runs on while a hidden window's frame clock skips the span,
    /// so two presses either side never read as a double click; the offscreen host
    /// stamps it from the source its frames read. The [`InputDelta`] says whether
    /// to redraw.
    #[inline]
    pub(crate) fn on_input(&mut self, event: InputEvent<'_>, now: Duration) -> InputDelta {
        self.input.on_input(event, &self.cascade, now)
    }

    // `peek_*` reads without declaring a wake. `watch_*` and plain reads declare
    // "wake me when this changes"; forgetting that freezes pointer-derived paint,
    // while forgetting a peek only costs frames. Watches are cleared pre-record:
    // re-call each active frame.

    /// Declare interest in off-target pointer events of `flags`.
    #[inline]
    pub const fn watch_pointer(&mut self, flags: PointerWake) {
        self.input.watch_pointer(flags);
    }

    /// Declare interest in off-focus keyboard categories; specific chords use
    /// [`Self::watch_key`].
    #[inline]
    pub const fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.input.watch_keyboard(flags);
    }

    /// Declare interest in one shortcut. Duplicate watchers collapse.
    #[inline]
    pub fn watch_key(&mut self, shortcut: Shortcut) {
        self.input.watch_key(shortcut);
    }

    /// Pointer events captured this frame; empty without a [`PointerWake`] watcher.
    /// Gated like [`Self::keyboard_events`]: an overlay's scope empties it for
    /// the planes strictly below, since watches bypass hit-testing.
    #[inline]
    pub fn pointer_events(&self) -> &[PointerEvent] {
        self.input.pointer_events(self.forest.cur_plane())
    }

    /// This frame's presses, in arrival order. Gated like [`Self::pointer_events`].
    #[inline]
    pub fn keyboard_events(&self) -> &[KeyPress] {
        self.input.keyboard_events(self.forest.cur_plane())
    }

    /// `true` if any press this frame matches `shortcut`. Auto-watches the chord,
    /// else the keyboard wake-gate parks off-focus presses until the next frame.
    #[inline]
    pub fn key_pressed(&mut self, shortcut: Shortcut) -> bool {
        let reader = KeyReader {
            plane: self.forest.cur_plane(),
            parent: self.forest.cur_parent_id(),
        };
        self.input.key_pressed(reader, &self.cascade, shortcut)
    }

    /// [`Self::key_pressed`] read as `reader` rather than the record position.
    #[inline]
    pub(crate) fn key_pressed_as(&mut self, reader: WidgetId, shortcut: Shortcut) -> bool {
        let reader = KeyReader {
            plane: self.forest.cur_plane(),
            parent: Some(reader),
        };
        self.input.key_pressed(reader, &self.cascade, shortcut)
    }

    /// Re-record this frame after measure, once at most (roughly a 2x frame); prefer
    /// a layout driver or [`App::update`].
    ///
    /// # Panics
    ///
    /// Panics outside a record pass.
    #[track_caller]
    pub fn request_relayout(&mut self) {
        // `FrameCycle::run` clears the flag before the app runs, so a call outside a
        // record would be dropped silently.
        assert!(
            self.forest.is_recording(),
            "Ui::request_relayout outside a record pass: it re-runs *this* \
             frame's record, so there is nothing for it to retry — drop the \
             call, or move the work into App::update",
        );
        self.frame_runtime.relayout_requested = true;
    }

    /// Monotonic time of the current frame, accumulated from host `dt`s. Pair with
    /// [`Self::request_repaint`] for continuous animation.
    #[inline]
    pub const fn now(&self) -> Duration {
        self.frame_runtime.time
    }

    /// Request the mouse cursor. Last writer wins; reset to [`CursorIcon::Default`]
    /// each record pass, so re-request every frame. Ignored headless.
    #[inline]
    pub const fn set_cursor(&mut self, cursor: CursorIcon) {
        self.window_requests.levels.cursor = cursor;
    }

    /// The cursor requested so far this record pass. Frame-scoped, unlike
    /// [`Self::vsync`].
    #[inline]
    pub const fn cursor(&self) -> CursorIcon {
        self.window_requests.levels.cursor
    }

    /// Set this window's presentation pacing, a retained level. Setting the mode in
    /// force is free; a real change recreates the swapchain after the frame.
    #[inline]
    pub const fn set_vsync(&mut self, vsync: Vsync) {
        self.window_requests.levels.vsync = vsync;
    }

    /// This window's presentation pacing, as last set or as the swapchain was opened.
    #[inline]
    pub const fn vsync(&self) -> Vsync {
        self.window_requests.levels.vsync
    }

    /// Ask the host for another frame after this one. Cleared at the top of every `frame`.
    pub fn request_repaint(&mut self) {
        tracing::trace!(
            target: "palantir.repaint",
            render_frame = self.frame_runtime.render_frame_id,
            "request_repaint",
        );
        self.frame_runtime.repaint_requested = true;
    }

    /// Ask for an image of this frame: the whole window for `None`, else `region` in logical
    /// pixels, clamped to the window. The host copies the frame as it paints it, and the image
    /// arrives through [`Self::screenshot_taken`] in the first frame after the device finishes
    /// the copy; on an [`OffscreenHost`](crate::OffscreenHost),
    /// [`wait_for_gpu`](crate::OffscreenHost::wait_for_gpu) and then one frame lands it. A frame
    /// paints in full when it asks; a second request in one frame replaces the first.
    ///
    /// # Panics
    ///
    /// Panics if `region` is not finite: its corner and its size are *offsets*. A negative size
    /// is an empty region, as it paints nothing.
    #[track_caller]
    pub fn request_screenshot(&mut self, region: Option<Rect>) {
        let window = self.display.logical_rect();
        let region = region.map_or(window, |region| {
            vec2::offset(region.min);
            vec2::offset(Vec2::new(region.size.w, region.size.h));
            region.clamp_to(window)
        });
        self.screenshot_request = Some(region);
        self.frame_runtime.repaint_requested = true;
    }

    /// The image a [`Self::request_screenshot`] asked for, during the one frame it lands in, as a
    /// response's `clicked` holds for the frame of the click; `None` in every other frame. Images
    /// land in the order they were asked for, one per frame, and the frame one lands in records.
    pub fn screenshot_taken(&self) -> Option<&Screenshot> {
        self.screenshot_taken.as_deref()
    }

    /// The region this frame asked an image of, taken by the driver once the frame is recorded.
    pub(crate) const fn take_screenshot_request(&mut self) -> Option<Rect> {
        self.screenshot_request.take()
    }

    /// Hand the next frame the image that landed for it, or none.
    pub(crate) fn set_screenshot_taken(&mut self, screenshot: Option<Screenshot>) {
        if screenshot.is_some() {
            self.frame_runtime.repaint_requested = true;
        }
        self.screenshot_taken = screenshot.map(Box::new);
    }

    /// Schedule a one-shot wake at `now + after`. Duplicate deadlines collapse.
    pub fn request_repaint_after(&mut self, after: Duration) {
        tracing::trace!(
            target: "palantir.repaint",
            ?after,
            render_frame = self.frame_runtime.render_frame_id,
            "request_repaint_after",
        );
        let deadline = self.frame_runtime.time.saturating_add(after);
        self.frame_runtime.schedule_wake(
            deadline,
            WakeReasons::REAL,
            self.display.refresh_millihertz,
        );
    }

    /// Open a top-level OS window addressed by `token`, unique among live windows.
    /// Deferred to the host after this frame, which applies every window's opens and
    /// closes in the order they were called, the last call per token winning: a later
    /// `config` replaces an earlier one, a later [`Self::close_window`] cancels the open,
    /// and an open after a close of a live window recreates it. An open of a live
    /// `token` is ignored with a warning. A window the platform or GPU cannot open is
    /// logged and stays closed, so [`Self::is_window_open`] answers `false`.
    ///
    /// # Panics
    ///
    /// Panics on a host with no window lifecycle ([`OffscreenHost`](crate::OffscreenHost)).
    pub fn open_window(&mut self, token: WindowToken, config: WindowConfig) {
        self.window_requests.commands.open(token, config);
    }

    /// Request that the window addressed by `token` close, deferred and ordered like
    /// [`Self::open_window`]. No-op for an unknown `token`.
    ///
    /// # Panics
    ///
    /// Panics on a host with no window lifecycle.
    #[inline]
    pub fn close_window(&mut self, token: WindowToken) {
        self.window_requests.commands.close(token);
    }

    /// `true` for the frame where the OS asked to close this window. It auto-closes
    /// after the frame unless you call [`Self::keep_open`]:
    ///
    /// ```
    /// # use palantir::{Ui, WindowToken};
    /// # struct App { unsaved: bool, show_quit_dialog: bool }
    /// # impl App {
    /// # fn demo(&mut self, ui: &mut Ui, win: WindowToken) {
    /// if ui.close_requested() && self.unsaved {
    ///     ui.keep_open();               // veto this frame's auto-close
    ///     self.show_quit_dialog = true; // remember to prompt
    /// }
    /// // …later, on the dialog's "Discard"/"Save" button:
    /// ui.close_window(win);             // close for real
    /// # }
    /// # }
    /// ```
    ///
    /// Always `false` headless.
    #[inline]
    pub const fn close_requested(&self) -> bool {
        self.window_frame.close_requested
    }

    /// Veto the auto-close pending from [`Self::close_requested`].
    #[inline]
    pub const fn keep_open(&mut self) {
        self.window_requests.close_vetoed = true;
    }

    // The recorder-host seam: two methods the host writes before a frame, two it reads after.

    /// Publish this frame's window-manager facts. Asserts the previous close veto
    /// did not survive; [`Self::drain_window_output`] clears it.
    #[cfg(feature = "winit")]
    pub(crate) fn set_window_facts(&mut self, facts: WindowFrameState) {
        debug_assert!(
            !self.window_requests.close_vetoed,
            "a veto outlived the frame that raised it",
        );
        self.window_frame = facts;
    }

    /// Seed the pacing level from the swapchain the host opened, so a control
    /// writing its value back does not override an explicit present mode.
    #[cfg(feature = "winit")]
    #[inline]
    pub(crate) const fn seed_vsync(&mut self, vsync: Vsync) {
        self.window_requests.levels.vsync = vsync;
    }

    /// Drain this frame's window scratch into `commands` and return the levels the
    /// host applies afterwards; see [`WindowRequests::drain`].
    pub(crate) fn drain_window_output(
        &mut self,
        token: WindowToken,
        commands: &mut WindowCommands,
    ) -> WindowOutput {
        let close_requested = self.window_frame.close_requested;
        let levels = self.window_requests.drain(token, close_requested, commands);
        self.window_frame = WindowFrameState::default();
        levels
    }

    /// This frame's record payloads, valid until the next record pass refills the arena.
    #[inline]
    pub(crate) const fn record_store(&self) -> &RecordStore {
        &self.forest.record_store
    }

    /// This window's live geometry for persist-and-restore. Placement position is
    /// `None` where the platform reports none (Wayland). In the window manager's
    /// logical pixels (see [`Display::system_logical_size`]), since the platform never
    /// hears about [`Self::set_user_scale`].
    #[expect(
        clippy::cast_sign_loss,
        reason = "a display's logical size is non-negative"
    )]
    pub fn window_geometry(&self) -> WindowGeometry {
        let logical = self.display.system_logical_size();
        WindowGeometry {
            inner_size: UVec2::new(
                (logical.w.round() as u32).max(1),
                (logical.h.round() as u32).max(1),
            ),
            placement: self.window_frame.placement,
        }
    }

    /// This app's debug-overlay flags, by value; modify then [`Self::set_debug_overlay`].
    ///
    /// ```
    /// # use palantir::{Checkbox, Configure, Ui};
    /// # fn demo(ui: &mut Ui) {
    /// let mut overlay = ui.debug_overlay();
    /// Checkbox::new(&mut overlay.damage_rect).label("damage rects").show(ui);
    /// Checkbox::new(&mut overlay.frame_stats).label("frame stats").show(ui);
    /// ui.set_debug_overlay(overlay);
    /// # }
    /// ```
    #[inline]
    pub fn debug_overlay(&self) -> DebugOverlayConfig {
        self.resources.diagnostics().overlay.get()
    }

    /// Replace this app's debug-overlay flags, visible in every window.
    #[inline]
    pub fn set_debug_overlay(&mut self, overlay: DebugOverlayConfig) {
        self.resources.diagnostics().overlay.set(overlay);
    }

    /// This frame's diagnostic counters, for the [`frame_stats`] overlay.
    ///
    /// [`frame_stats`]: crate::diagnostics::frame_stats
    pub(crate) fn frame_stats(&self) -> FrameStats {
        FrameStats {
            frame_id: self.frame_runtime.frame_id,
            render_frame_id: self.frame_runtime.render_frame_id,
            fps: self.frame_runtime.fps_ema,
            settle_frames: self.frame_runtime.settle_frames,
            gpu: self.resources.diagnostics().gpu_pass_stats.last_pass(),
        }
    }

    /// Whether the window addressed by `token` is live, as of this frame's start;
    /// changes made this frame show next frame.
    #[inline]
    pub fn is_window_open(&self, token: WindowToken) -> bool {
        self.resources.windows().contains(token)
    }

    /// The app-global live-window set behind [`Self::is_window_open`].
    #[inline]
    pub(crate) const fn window_directory(&self) -> &WindowDirectory {
        self.resources.windows()
    }

    /// Attach a paint primitive to the active node. Direct text counts toward
    /// layout only on a leaf; container-owned text is an overlay shaped against the
    /// container's padded width.
    pub fn add_shape<S: Lower>(&mut self, shape: S) {
        self.forest.add_shape(shape);
    }

    /// Load an icon set and get back an owning [`IconSet`]. Hold it: dropping the
    /// last clone unloads the data and rasters at the next submit. Idempotent while a
    /// set is held. No `Result`: a malformed icon is reported when drawn.
    #[inline]
    pub fn load_icons(&self, table: impl Into<Rc<IconTable>>) -> IconSet {
        self.resources.icons().register(table.into())
    }

    /// Load an image and get back an owning [`ImageHandle`]; dropping the last clone
    /// frees the GPU texture. Keep `image` to refill it for [`ImageHandle::update`].
    ///
    /// # Errors
    ///
    /// An image axis exceeds the device's 2D texture limit.
    #[inline]
    pub fn load_image(&self, image: &Image) -> Result<ImageHandle, ImageTooLarge> {
        self.resources.load_image(image)
    }

    /// Register a font and return the [`FontFamily`] of its first face. Permanent
    /// (fontdb face ids are never reused). A collection registers every face, reachable
    /// via [`FontFamily::named`](crate::FontFamily::named). Re-shapes this frame's text.
    ///
    /// ```
    /// # fn load(ui: &palantir::Ui, iosevka: &'static [u8]) -> Result<(), palantir::FontLoadError> {
    /// let mono = ui.load_font(iosevka)?; // the bytes of `include_bytes!("Iosevka.ttf")`
    /// let sans = ui.load_font("/usr/share/fonts/TTF/Charter.ttf")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// A `&str` is a path, never a family name.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Io`] when the file cannot be read, [`FontLoadError::NoFaces`]
    /// when it holds no parsable face, [`FontLoadError::FamilyTableFull`] when the
    /// family table is full.
    #[inline]
    pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError> {
        self.resources.text().load_font(source)
    }

    /// Whether a face answers to `family`. Without one, shaping falls back to
    /// [`FontFamily::SANS`](crate::FontFamily::SANS) and warns once.
    #[inline]
    pub fn has_font(&self, family: FontFamily) -> bool {
        self.resources.text().has_font(family)
    }

    /// Every family the shaper's database knows, system fonts included. Cold.
    #[inline]
    pub fn font_families(&self) -> Vec<FontFamily> {
        self.resources.text().font_families()
    }

    /// The largest width or height [`Self::load_image`] accepts (the device's
    /// `max_texture_dimension_2d`); `None` for a CPU recorder. Larger images are
    /// rejected, not shrunk.
    #[inline]
    pub const fn max_image_dimension(&self) -> Option<NonZeroU32> {
        self.resources.texture_limit().max_dimension()
    }

    /// A handle on the host's clipboard, shared by every window of that host. A
    /// clone, since a widget reads it inside a keyboard walk already holding `&mut Ui`.
    ///
    /// Backed by the OS clipboard plus an in-process fallback for refused writes.
    /// [`WinitHost`](crate::WinitHost) uses the OS clipboard;
    /// [`OffscreenHost`](crate::OffscreenHost) only when asked via
    /// [`OffscreenHostBuilder::system_clipboard`](crate::OffscreenHostBuilder::system_clipboard).
    /// Without the `system-clipboard` feature only the in-process buffer exists.
    #[inline]
    pub fn clipboard(&self) -> Clipboard {
        self.resources.clipboard().clone()
    }

    /// Record `view`: refresh its row in [`Self::gpu_views`] and append an image
    /// shape carrying the row's `epoch` to the active node.
    pub(crate) fn gpu_view(&mut self, view: RecordedView) {
        let frame = self.frame_runtime.render_frame_id;
        let epoch = self.gpu_views.record(view, frame);
        self.forest.add_gpu_view(epoch);
    }

    /// Format `args` into the record-pass text storage and return an arena-backed
    /// [`InternedStr`]. Usually reached through [`fmt!`](crate::fmt).
    ///
    /// Valid only for the pass that minted it, in this window; holding it into a later
    /// pass or another window panics.
    #[must_use]
    #[inline]
    pub fn fmt(&mut self, args: fmt::Arguments<'_>) -> InternedStr {
        self.forest.record_store.intern_fmt(args)
    }

    /// Normalize borrowed, owned or interned text into an [`InternedStr`]. An
    /// [`InternedStr`] from an earlier pass or another window panics.
    #[must_use]
    pub fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr {
        self.forest.record_store.intern(text.into())
    }

    /// The characters of `text`, a handle this pass interned. The borrow holds
    /// `self`; copy to a scratch `String` first if you also need `&mut Ui`.
    ///
    /// # Panics
    ///
    /// Panics when `text` was interned by an earlier pass or another window.
    #[must_use]
    pub fn text(&self, text: InternedStr) -> &str {
        self.forest.record_store.text(text)
    }

    /// Append `shape` to the active node and animate it at paint time. The recorded
    /// shape is identical every frame, so the widget never re-records. Drops silently
    /// if the shape was noop-collapsed.
    ///
    /// ```
    /// # use palantir::widget::{PaintAnimation, PaintRepeat, Shape, curves};
    /// # use palantir::{Rect, RgbaF32, Ui};
    /// # use std::time::Duration;
    /// # fn demo(ui: &mut Ui) {
    /// ui.add_shape_animated(
    ///     Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(RgbaF32::WHITE),
    ///     PaintAnimation::alpha(0.4, 1.0)
    ///         .with_period(Duration::from_secs(2))
    ///         .with_repeat(PaintRepeat::Forever)
    ///         .with_curve(curves::sine),
    /// );
    /// # }
    /// ```
    pub fn add_shape_animated<S: Lower>(&mut self, shape: S, animation: PaintAnimation) {
        self.forest.add_shape_animated(shape, animation);
    }

    /// Open a side layer that paints above `Main`, escapes ancestor clip and
    /// hit-tests on top. Configure on the returned [`LayerScope`] and finish with
    /// [`LayerScope::show`].
    ///
    /// Called inside another layer's body, the new one paints and hit-tests above
    /// the one it opened from, whatever their ranks: a [`Layer::Popup`] opened in a
    /// [`Layer::Modal`] dialog sits over the dialog. Focus rules read the layer it
    /// asked for, so a dropdown in a dialog still traps Tab as a popup does.
    #[inline]
    pub fn layer(&mut self, layer: Layer) -> LayerScope<'_> {
        LayerScope::new(self, layer)
    }

    /// Withdraw an [`input_scope`](crate::Configure::input_scope) this pass recorded.
    /// Holds through the end of the next frame; the current pass is unaffected, since
    /// scope paths resolve at pass start against a one-frame-old cascade.
    #[inline]
    pub fn release_input_scope(&mut self, id: WidgetId) {
        self.input.release_input_scope(id);
    }

    /// Resolve a widget's identity recipe against the open parent into its record
    /// id. [`Widget::resolve`] is the one caller.
    ///
    /// [`Widget::resolve`]: crate::widget::Widget::resolve
    #[inline]
    pub(crate) fn resolve_ident(&mut self, ident: Ident) -> ResolvedId {
        self.forest.widget_id(ident)
    }

    /// Open `node` under `resolved`, painting `chrome` and, for keyboard-sourced
    /// focus, the theme's focus ring. Pairs with [`Self::close_node`]. Widget code
    /// calls `Widget::record`, never this.
    #[inline]
    #[track_caller]
    pub(crate) fn open_node(&mut self, resolved: ResolvedId, node: &Node, chrome: Option<&Chrome>) {
        let ring = if self.input.focused() == Some(resolved.id()) && self.input.focus_visible() {
            let theme = &self.theme.focus_ring;
            Stroke::new(domain::color(theme.color), domain::length(theme.width))
        } else {
            Stroke::NONE
        };
        self.forest
            .open_node(resolved, node, NodeChrome { chrome, ring });
    }

    #[inline]
    pub(crate) fn close_node(&mut self) {
        self.forest.close_node();
    }

    /// Intern a grid's track definition into the current layer. Called by [`Widget::grid_tracks`].
    ///
    /// [`Widget::grid_tracks`]: crate::widget::Widget::grid_tracks
    #[inline]
    pub(crate) fn push_grid_def(&mut self, rows: &[Track], cols: &[Track]) -> GridDefId {
        self.forest.push_grid_def(rows, cols)
    }

    /// Intern a bar overlay's definition into the current layer. Called by
    /// [`Widget::scrollbar_def`].
    ///
    /// [`Widget::scrollbar_def`]: crate::widget::Widget::scrollbar_def
    #[inline]
    pub(crate) fn push_scrollbars_def(&mut self, def: ScrollbarsDef) -> ScrollbarsDefId {
        self.forest.push_scrollbars_def(def)
    }

    /// The content extent the scroll viewport `id` measured last frame, before
    /// zoom. `Size::ZERO` for a non-viewport or a widget not yet laid out.
    #[inline]
    pub(crate) fn scroll_content(&self, id: WidgetId) -> Size {
        // Bridge: `Layout` keys by `(layer, node)`, callers hold a `WidgetId`.
        self.cascade
            .endpoint(id)
            .map_or(Size::ZERO, |endpoint| self.layout.scroll_content(endpoint))
    }

    /// Snapshot of input/cascade state for a widget. `rect` and `disabled` are last
    /// frame's; interaction fields are computed against this frame's input, so read it
    /// during record. The widget's own `NodeFlags::is_disabled` is folded in by
    /// `Widget::response`.
    pub fn response_for(&self, id: WidgetId) -> ResponseState {
        let loc = self.last_frame_location(id);
        let mut state = self.input.response_for(id, loc, self.arranged_frame());
        // Cascade lags a frame; fold in this frame's ancestor-disabled so a newly
        // disabled subtree paints disabled at once.
        state.merge_disabled(self.forest.ancestor_disabled());
        state
    }

    /// Last frame's cascade and layout, as a response reads them.
    const fn arranged_frame(&self) -> ArrangedFrame<'_> {
        ArrangedFrame {
            cascade: &self.cascade,
            layout: &self.layout,
        }
    }

    /// Where the most recent cascade run put `id`; uses
    /// [`SeenIds::last_frame_endpoint`](crate::scene::seen_ids::SeenIds::last_frame_endpoint)
    /// when valid, else `Cascade::by_id`.
    #[inline]
    fn last_frame_location(&self, id: WidgetId) -> Option<WidgetLocation> {
        let endpoint = match self.forest.ids.last_frame_endpoint(id) {
            LastFrame::At(endpoint) if self.cascade_is_last_frame => Some(endpoint),
            LastFrame::Absent if self.cascade_is_last_frame => None,
            _ => return self.cascade.locate(id),
        };
        debug_assert_eq!(
            endpoint,
            self.cascade.endpoint(id),
            "the positional read of {id:?} left the cascade's row",
        );
        endpoint.map(|endpoint| self.cascade.location(endpoint))
    }

    /// The cross-frame state row for `id`, or `None` if `(id, S)` was never stored.
    /// Creates nothing.
    pub fn state<S: 'static>(&self, id: WidgetId) -> Option<&S> {
        self.state.try_get::<S>(id)
    }

    /// Lend the cross-frame state row for `id` to `body` alongside the `Ui`
    /// (`S::default()` on first use).
    ///
    /// ```
    /// # use palantir::{Button, Configure, Text, Ui, WidgetId};
    /// # #[derive(Default)]
    /// # struct Page { clicks: u32, note: String }
    /// # fn demo(ui: &mut Ui, page_id: WidgetId) {
    /// ui.with_state::<Page, _>(page_id, |ui, page| {
    ///     if Button::new().label("click").show(ui).clicked() {
    ///         page.clicks += 1;
    ///     }
    ///     Text::new(&page.note).show(ui);
    /// });
    /// # }
    /// ```
    ///
    /// A row is dropped after a frame that recorded `id` is followed by one that does
    /// not; a row under an id no node records lives as long as the `Ui`. Re-entering
    /// the same `(id, S)` inside `body` sees a default row, overwritten on restore.
    pub fn with_state<S: Default + 'static, R>(
        &mut self,
        id: WidgetId,
        body: impl FnOnce(&mut Self, &mut S) -> R,
    ) -> R {
        let mut value = mem::take(self.state.get_or_insert_with(id, S::default));
        let out = body(self, &mut value);
        // Re-probed: `body` may have inserted rows of the same `S`, reallocating the store.
        *self.state.get_or_insert_with(id, S::default) = value;
        out
    }

    /// The one `S` this `Ui` holds, or `None`. For state a widget kind shares across
    /// instances; keyed by type, never swept.
    pub fn singleton<S: 'static>(&self) -> Option<&S> {
        self.singletons.get::<S>()
    }

    /// Lend the one `S` to `body` beside the `Ui`, as [`Self::with_state`]. A nested
    /// call for the same `S` sees the default and its writes are lost.
    pub fn with_singleton<S: Default + 'static, R>(
        &mut self,
        body: impl FnOnce(&mut Self, &mut S) -> R,
    ) -> R {
        let mut value = mem::take(self.singletons.get_or_default::<S>());
        let out = body(self, &mut value);
        *self.singletons.get_or_default::<S>() = value;
        out
    }

    /// Advance the animation row `request` names toward its target and return the
    /// current value. [`AnimationSpec::SNAP`] and `None` land on the target now, drop
    /// any stale row and request no repaint.
    // Keep the no-map/no-spec return in the widget's block so a static theme does
    // not pay an outlined call plus a large `V` return-slot handoff.
    #[inline(always)]
    pub fn animate<V: Animatable>(
        &mut self,
        request: AnimationRequest<V>,
        spec: impl Into<Option<AnimationSpec>>,
    ) -> V {
        let spec = spec.into().unwrap_or(AnimationSpec::SNAP);
        let clock = AnimationClock {
            dt: self.frame_runtime.dt,
            render_frame_id: self.frame_runtime.render_frame_id,
        };
        let r = self.animation.animate(request, spec, clock);
        if !r.settled {
            self.frame_runtime.repaint_requested = true;
        }
        r.cur
    }

    /// Currently focused widget id, or `None`.
    #[inline]
    pub const fn focus(&self) -> Option<WidgetId> {
        self.input.focused()
    }

    /// True when focus sits on `ancestor` or in its subtree, per the most recent
    /// cascade (one frame of lag). Layers are separate trees, so popup focus is not
    /// within its anchor.
    #[inline]
    pub fn is_focus_within(&self, ancestor: WidgetId) -> bool {
        self.input
            .focused()
            .is_some_and(|f| self.cascade.is_within(f, ancestor))
    }

    /// True when the pointer's hover target is `ancestor` or in its subtree.
    /// Occlusion-aware, and changes only when a repaint is already scheduled.
    #[inline]
    pub fn is_hover_within(&self, ancestor: WidgetId) -> bool {
        self.input
            .hovered()
            .is_some_and(|h| self.cascade.is_within(h, ancestor))
    }

    /// Active `Display` (physical surface size and both scale factors).
    #[inline]
    pub const fn display(&self) -> Display {
        self.display
    }

    /// The application's own scale, multiplied onto the platform's. `UserScale::ONE`
    /// until set.
    #[inline]
    pub fn user_scale(&self) -> UserScale {
        self.resources.user_scale().get()
    }

    /// Scale the whole UI by `scale` on top of the platform factor. App-global;
    /// takes effect next frame, so this frame's [`Self::display`] reports the old value.
    ///
    /// ```
    /// # use palantir::{Ui, UserScale};
    /// # fn demo(ui: &mut Ui) {
    /// // Whatever the app binds its own zoom-in chord to.
    /// let scale = ui.user_scale();
    /// ui.set_user_scale(scale.stepped_up());
    /// # }
    /// ```
    ///
    /// Restore a persisted value from the
    /// [`WinitHostBuilder::build`](crate::WinitHostBuilder::build) factory. Each
    /// distinct scale re-rasterizes every glyph; see [`UserScale`].
    #[inline]
    pub fn set_user_scale(&mut self, scale: UserScale) {
        self.resources.user_scale().set(scale);
    }

    /// This frame's monotonic index over frames authoring code ran on. Retained
    /// state stamps it to notice a gap (it was skipped); paint-only frames advance
    /// nothing. See [`Self::render_frame_id`].
    #[inline]
    pub const fn frame_id(&self) -> u64 {
        self.frame_runtime.frame_id
    }

    /// This frame's monotonic index among frames that reached the screen. Counts
    /// paint-only frames, so use [`Self::frame_id`] to detect a skipped reader.
    #[inline]
    pub const fn render_frame_id(&self) -> u64 {
        self.frame_runtime.render_frame_id
    }

    /// Shape `run` and return its geometry (caret positions, click-to-offset,
    /// selection rects). `&mut self` so an overlapping second probe is a compile error
    /// (the probe holds the shaper's exclusive lease) rather than a runtime panic.
    #[inline]
    pub fn probe_text<'a>(&'a mut self, run: TextRun<'a>) -> TextProbe<'a> {
        self.resources.text().layout(&run)
    }

    /// Every edge the pointer produced this frame, widget by widget: the collation
    /// counterpart to [`Self::response_for`]'s polling. Edges, not levels (see
    /// [`PointerEdge`](crate::PointerEdge)). Read during record.
    #[inline]
    pub fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_ {
        self.input.pointer_actions()
    }

    /// Move keyboard focus to `id` now, bypassing [`FocusPolicy`]. Key-class routing
    /// moves next record pass, so a widget that blurs on Escape does not also hand it
    /// to the overlay around it. A widget that cannot take focus (not focusable,
    /// disabled or hidden) loses it as the frame ends.
    #[inline]
    pub const fn set_focus(&mut self, id: WidgetId) {
        self.input.set_focus(Some(id));
    }

    /// Leave nothing focused; same routing lag as [`Self::set_focus`].
    #[inline]
    pub const fn clear_focus(&mut self) {
        self.input.set_focus(None);
    }

    /// Ask for IME text this frame, with `caret` (logical px) where the platform
    /// places its candidate list. A level asked for every frame it is wanted; a frame
    /// with no call turns IME off.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `caret` is an
    /// [offset](crate::widget::domain::offset).
    #[inline]
    #[track_caller]
    pub const fn request_ime(&mut self, caret: Rect) {
        caret.validate();
        self.window_requests.levels.ime = Some(caret);
    }

    /// The input method's uncommitted text, for the focused widget to draw at its
    /// caret; `None` when no composition is live.
    #[inline]
    pub fn ime_preedit(&self) -> Option<ImePreedit<'_>> {
        self.input.ime_preedit()
    }

    /// Whether focus came from the keyboard (focus ring showing) rather than a
    /// press, like CSS `:focus-visible`.
    #[inline]
    pub const fn is_focus_visible(&self) -> bool {
        self.input.focus_visible()
    }

    /// Move focus to the first Tab stop under `ancestor`, as a popup opened from the
    /// keyboard does. Takes effect at the end of this frame; focus returns to the
    /// previous holder when `ancestor` leaves the tree.
    #[inline]
    pub const fn focus_first_within(&mut self, ancestor: WidgetId) {
        self.input.focus_first_within(ancestor);
    }

    /// Pointer position in logical pixels (surface space), or `None` off-surface.
    /// `&mut` because reading auto-asserts a [`PointerWake::MOVE`] watch so
    /// pointer-derived paint keeps repainting. For hover styling prefer
    /// [`Self::is_hover_within`].
    #[inline]
    pub const fn pointer_pos(&mut self) -> Option<Vec2> {
        self.watch_pointer(PointerWake::MOVE);
        self.input.pointer_pos()
    }

    /// Pointer position in `id`'s pre-transform local coordinates. `None` when
    /// off-surface or the widget did not arrange last frame. Auto-watches
    /// [`PointerWake::MOVE`]; see [`Self::peek_pointer_local`].
    #[inline]
    pub fn pointer_local(&mut self, id: WidgetId) -> Option<Vec2> {
        self.watch_pointer(PointerWake::MOVE);
        self.input.pointer_local_for(id, self.arranged_frame())
    }

    /// Currently held modifier keys. Auto-watches [`KeyboardWake::MODIFIER`]; when
    /// gated on something that already woke the frame, use [`Self::peek_modifiers`].
    #[inline]
    pub const fn modifiers(&mut self) -> Modifiers {
        self.watch_keyboard(KeyboardWake::MODIFIER);
        self.input.modifiers()
    }

    /// [`Self::pointer_pos`] without the [`PointerWake::MOVE`] watch. Right only
    /// when another event guarantees the frame; stale for continuously derived paint.
    #[inline]
    pub const fn peek_pointer_pos(&self) -> Option<Vec2> {
        self.input.pointer_pos()
    }

    /// [`Self::pointer_local`] without the [`PointerWake::MOVE`] watch; same caveat as
    /// [`Self::peek_pointer_pos`].
    #[inline]
    pub fn peek_pointer_local(&self, id: WidgetId) -> Option<Vec2> {
        self.input.pointer_local_for(id, self.arranged_frame())
    }

    /// [`Self::modifiers`] without the [`KeyboardWake::MODIFIER`] watch. Use the
    /// watched read when a bare modifier press must repaint on its own.
    #[inline]
    pub const fn peek_modifiers(&self) -> Modifiers {
        self.input.modifiers()
    }

    /// What a press on a non-focusable widget does to focus. See [`FocusPolicy`].
    #[inline]
    pub const fn focus_policy(&self) -> FocusPolicy {
        self.input.focus_policy()
    }

    /// Set the press-on-non-focusable behavior. See [`FocusPolicy`].
    #[inline]
    pub const fn set_focus_policy(&mut self, p: FocusPolicy) {
        self.input.set_focus_policy(p);
    }

    /// Which input signal gates a full record pass. See [`InputPolicy`].
    #[inline]
    pub const fn input_policy(&self) -> InputPolicy {
        self.input.input_policy()
    }

    /// Set the record gate's input signal. [`InputPolicy::OnDelta`] skips inert
    /// pointer moves; [`InputPolicy::Always`] sees every event.
    #[inline]
    pub const fn set_input_policy(&mut self, p: InputPolicy) {
        self.input.set_input_policy(p);
    }
}

/// The doors past [`Ui`]'s private fields, for white-box suites and benches.
/// No `#[inline]`: nothing here reaches an optimized build that would want it.
#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::theme::Theme;
    use std::rc::Rc;

    #[cfg(test)]
    use crate::animation::AnimationMap;
    use crate::cascade::Cascade;
    use crate::display::Display;
    use crate::display::frame_stamp::FrameStamp;
    use crate::input_state::InputState;
    use crate::internals::inspected_widget::InspectedWidget;
    use crate::layout::Layout;
    #[cfg(test)]
    use crate::layout::plane_layout::PlaneLayout;
    #[cfg(test)]
    use crate::primitives::geometry::rect::Rect;
    #[cfg(test)]
    use crate::scene::endpoint::Endpoint;
    use crate::scene::forest::Forest;
    #[cfg(test)]
    use crate::scene::plane::Plane;
    #[cfg(test)]
    use crate::scene::tree::Tree;
    #[cfg(test)]
    use crate::scene::tree::node_id::NodeId;
    #[cfg(test)]
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::ui::frame_runtime::FrameRuntime;
    #[cfg(all(test, feature = "winit"))]
    use crate::window::window_frame_state::WindowFrameState;
    #[cfg(test)]
    use crate::window::window_requests::WindowRequests;

    /// What the frame harness reads and seeds.
    impl Ui {
        /// The input machine, for tests asserting routing state the public surface hides.
        pub(crate) const fn input(&self) -> &InputState {
            &self.input
        }

        pub(crate) const fn cascade(&self) -> &Cascade {
            &self.cascade
        }

        pub(crate) const fn frame_runtime(&self) -> &FrameRuntime {
            &self.frame_runtime
        }

        /// Set the last frame's stamp. `Some` makes the next frame warm (skips warmup); `None` is a
        /// cold start.
        pub(crate) const fn set_prev_stamp(&mut self, stamp: Option<FrameStamp>) {
            self.frame_runtime.prev_stamp = stamp;
        }

        /// Replace the display the next frame lays out at, as the window driver does.
        pub(crate) const fn set_display(&mut self, display: Display) {
            self.display = display;
        }
    }

    /// Read by tools that drive a `Ui` from outside.
    impl Ui {
        /// The last frame's interactive widgets, in paint order, into `out`.
        pub fn inspect(&self, out: &mut Vec<InspectedWidget>) {
            InspectedWidget::collect(&self.forest, &self.cascade, out);
        }
    }

    /// Read by benches as well as tests.
    impl Ui {
        /// The whole forest, for callers that re-run a pass over every plane.
        pub(crate) const fn forest(&self) -> &Forest {
            &self.forest
        }

        /// The font database's epoch, for recomputing a reuse key this `Ui` folds it into.
        pub(crate) fn font_epoch(&self) -> u32 {
            self.resources.text().font_epoch()
        }

        /// The whole layout table, for callers that walk every plane.
        pub(crate) const fn layout_tables(&self) -> &Layout {
            &self.layout
        }

        /// Close the input frame as the end of a frame does (dropping its edges, then applying
        /// held events) and return whether input owes another. For a caller streaming input with
        /// no frame recorded between.
        pub(crate) fn end_input_frame(&mut self) -> bool {
            self.input.end_frame(&self.cascade);
            self.input.next_frame(&self.cascade)
        }
    }

    /// Only this crate's own tests call these.
    #[cfg(test)]
    impl Ui {
        /// [`Self::input`], mutably, for tests that drive routing state.
        pub(crate) fn input_mut(&mut self) -> &mut InputState {
            &mut self.input
        }

        /// One plane's recorded tree; a `Layer` names its base plane.
        pub(crate) fn tree(&self, plane: impl Into<Plane>) -> &Tree {
            &self.forest.trees[plane.into()]
        }

        /// One plane's arranged columns; a `Layer` names its base plane. Callers wanting the whole
        /// table use [`Self::layout_tables`].
        pub(crate) fn layout(&self, plane: impl Into<Plane>) -> &PlaneLayout {
            &self.layout[plane.into()]
        }

        /// `node`'s arranged rect on `plane`: pre-transform, unclipped, world coords.
        pub(crate) fn arranged_rect(&self, plane: impl Into<Plane>, node: NodeId) -> Rect {
            self.layout.arranged_rect(Endpoint {
                plane: plane.into(),
                node,
            })
        }

        /// The shaper the recorder measures and paints text with, for cache and measure-count
        /// probes.
        pub(crate) fn shaper(&self) -> &TextShaper {
            self.resources.text()
        }

        /// The animation rows, for tests that count what is resident.
        pub(crate) fn animation_mut(&mut self) -> &mut AnimationMap {
            &mut self.animation
        }

        /// Only the winit host's own tests write here.
        #[cfg(feature = "winit")]
        pub(crate) fn frame_runtime_mut(&mut self) -> &mut FrameRuntime {
            &mut self.frame_runtime
        }

        pub(crate) fn window_requests(&self) -> &WindowRequests {
            &self.window_requests
        }

        /// Only the winit host's own tests write here.
        #[cfg(feature = "winit")]
        pub(crate) fn window_frame_mut(&mut self) -> &mut WindowFrameState {
            &mut self.window_frame
        }
    }

    impl Ui {
        /// The active theme, for in-place edits. Gated: apps build a [`Theme`] and call
        /// [`set_theme`](Ui::set_theme). Copy-on-write, so a handle from [`Ui::theme`]
        /// keeps its values.
        pub fn theme_mut(&mut self) -> &mut Theme {
            Rc::make_mut(&mut self.theme)
        }
    }
}

#[cfg(test)]
mod tests;
