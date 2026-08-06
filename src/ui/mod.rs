#[cfg(feature = "bench")]
pub(crate) mod bench;
pub(crate) mod frame;
mod frame_cycle;
pub(crate) mod frame_report;
mod frame_stats;
pub(crate) mod layer_scope;
pub(crate) mod resources;
pub(crate) mod state;

use std::num::NonZeroU32;

use crate::animation::animatable::Animatable;
use crate::animation::{AnimMap, AnimSlot, AnimSpec};
use crate::app::App;
use crate::diagnostics::DebugOverlayConfig;
use crate::display::Display;
use crate::input::keyboard::{KeyboardEvent, Modifiers};
use crate::input::pointer::PointerEvent;
use crate::input::policy::FocusPolicy;
use crate::input::policy::InputPolicy;
use crate::input::response::{InputDelta, ResponseState};
use crate::input::shortcut::Shortcut;
use crate::input::watch::{KeyboardWake, PointerWake};
use crate::input::{InputEvent, InputState};
use crate::layout::Layout;
use crate::layout::engine::LayoutEngine;
use crate::primitives::image::Image;
use crate::primitives::widget_id::WidgetIdMap;
use crate::renderer::frontend::FrameScene;
use crate::renderer::gpu_view::{GpuPaint, GpuPaintRef, GpuViewEntry};
use crate::renderer::image_registry::{ImageHandle, RegisterImageError};
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::tree::paint_anims::PaintAnim;
use crate::text::probe::TextProbe;
use crate::text::run::TextRun;
use crate::{InternedStr, TextInput};

use crate::primitives::widget_id::WidgetId;
use crate::scene::cascade::Cascade;
use crate::scene::cascade::engine::CascadeEngine;
use crate::scene::damage::DamageEngine;
use crate::shape::Lower;
use crate::ui::frame::{FrameInput, FrameRuntime, WakeReasons};
use crate::ui::frame_cycle::FrameCycle;
use crate::ui::frame_report::FrameReport;
use crate::ui::layer_scope::LayerScope;
use crate::ui::resources::UiResources;
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
use crate::widgets::widget::Widget;
use crate::window::{
    CursorIcon, PendingWindow, Vsync, WindowConfig, WindowFrameState, WindowGeometry,
    WindowRequests, WindowToken,
};
use glam::UVec2;
use std::cell::{RefCell, RefMut};
use std::collections::hash_map::Entry;
use std::rc::Rc;
use std::time::Duration;

/// Recorder + input/response broker. All public coordinates are
/// logical pixels (DIPs); `Display::scale_factor` converts to
/// physical at the wgpu boundary. Frame scheduling state is retained
/// internally.
#[derive(Debug)]
pub struct Ui {
    pub(crate) forest: Forest,
    /// Refcounted, deliberately: a widget that styles *from the theme*
    /// has to hold the bundle across the `&mut Ui` its own `show` takes,
    /// and a plain `&ui.theme` borrow cannot survive that reborrow. The
    /// pre-`Rc` shape forced every such site to clone a whole bundle to
    /// launder the borrow — 640 B for a `ButtonTheme`, 692 B for a
    /// `TextEditTheme`, per widget per frame. [`Self::theme`] hands out
    /// the handle that makes it a refcount bump instead; see
    /// [`Self::theme_mut`] for the write side.
    theme: Rc<Theme>,
    /// Cross-frame widget state: per-type dense stores keyed by
    /// `WidgetId` (see [`StateMap`]).
    pub(crate) state: StateMap,
    /// Live `GpuView`s, keyed by `WidgetId` — the only `GpuView` bookkeeping on
    /// the `Ui`. [`Self::gpu_view`] upserts an entry (minting the stable backend
    /// `TextureId` once, refreshing the paint callback); the shape records only
    /// the redraw epoch and the encoder looks the view up here by the node's
    /// `WidgetId`. Swept by the same `removed` set as [`StateMap`].
    gpu_views: WidgetIdMap<GpuViewEntry>,
    /// App-global capabilities available to the recorder.
    pub(crate) resources: UiResources,
    pub(crate) layout_engine: LayoutEngine,
    pub(crate) layout: Layout,
    /// Cascaded clip/disabled/invisible/transform per node + global
    /// hit index. Written by `CascadeEngine::run` in the paint phase
    /// and read by the encoder, input dispatch, and damage compute.
    pub(crate) cascade: Cascade,
    /// Private, deliberately: everything outside this module reaches
    /// input through `Ui`'s public API, so the surface a caller's own
    /// widget has is the surface every widget in this crate is built
    /// on. Tests reach the machine itself through this module's
    /// test-gated `internals`.
    input: InputState,
    /// Selects which "did input arrive?" signal `take_frame_plan`
    /// consults to gate the full record path. Default
    /// [`InputPolicy::OnDelta`] skips record on inert pointer moves
    /// and scroll-over-nothing; flip to [`InputPolicy::Always`] for
    /// telemetry / custom canvases that need every event.
    pub input_policy: InputPolicy,
    pub(crate) cascade_engine: CascadeEngine,
    pub(crate) display: Display,
    pub(crate) damage_engine: DamageEngine,
    anim: AnimMap,
    /// Retained frame clock, wake queue, repaint/relayout flags, and prior-frame
    /// validity state. Kept separate from the widget engines above.
    pub(crate) frame_runtime: FrameRuntime,
    /// Recorder-to-host requests retained across frames.
    pub(crate) window_requests: WindowRequests,
    /// Host-to-recorder facts refreshed before each windowed frame.
    pub(crate) window_frame: WindowFrameState,
}

/// The widget- and host-facing authoring API: input feed, watches,
/// repaint/relayout requests, shape recording, per-widget state, and
/// animation, plus the crate-facing handles a host needs
/// (construction, `Self::frame`, `Self::frame_scene`).
///
/// The frame lifecycle those handles start — record / measure /
/// arrange / cascade / finalize, and the resets between them — is
/// `FrameCycle`'s, and user code never reaches it.
impl Ui {
    pub(crate) fn frame_scene(&self) -> FrameScene<'_> {
        FrameScene {
            forest: &self.forest,
            layout: &self.layout,
            cascade: &self.cascade,
            payloads: self.forest.record_store.payloads.borrow(),
            gpu_views: &self.gpu_views,
            display: self.display,
            time: self.frame_runtime.time,
        }
    }

    /// Construct a per-window `Ui` from its app-global capabilities. Each `Ui`
    /// creates its own [`Forest`], whose retained record payloads
    /// remain isolated from other windows' record passes.
    pub(crate) fn new(resources: UiResources) -> Self {
        let layout_engine = LayoutEngine::new(resources.text.clone());
        Self {
            resources,
            forest: Default::default(),
            theme: Default::default(),
            state: Default::default(),
            gpu_views: Default::default(),
            layout_engine,
            layout: Default::default(),
            cascade: Default::default(),
            input: Default::default(),
            input_policy: Default::default(),
            cascade_engine: Default::default(),
            display: Default::default(),
            damage_engine: Default::default(),
            anim: Default::default(),
            frame_runtime: Default::default(),
            window_requests: Default::default(),
            window_frame: Default::default(),
        }
    }

    /// The active theme. Reads go straight through the `Rc` —
    /// `ui.theme().button.padding` — and `.clone()` on the result is a
    /// refcount bump, **not** a copy of the ~9 KB bundle tree, which is
    /// why this hands back the handle rather than a plain `&Theme`.
    ///
    /// That handle is what a widget builder needs when a style reference
    /// has to stay valid across the `show(ui)` that consumes it — the
    /// `&mut Ui` reborrow would otherwise end a plain `&ui.theme()`
    /// borrow, and the only way out used to be cloning the whole bundle:
    ///
    /// ```ignore
    /// let theme = ui.theme().clone();
    /// Button::new().label("File").style(&theme.menu_button).show(ui);
    /// ```
    #[inline]
    pub fn theme(&self) -> &Rc<Theme> {
        &self.theme
    }

    /// The active theme, for in-place edits (`ui.theme_mut().button.anim
    /// = …`). Copy-on-write: the deep copy fires only if a handle from
    /// [`Self::theme`] is still alive, which in practice means mutating
    /// the theme from inside a widget's `show` — apps restyle between
    /// frames and pay nothing.
    #[inline]
    pub fn theme_mut(&mut self) -> &mut Theme {
        Rc::make_mut(&mut self.theme)
    }

    /// Replace the whole theme. Takes the `Rc` so an app swapping
    /// between prebuilt themes (light/dark, a preferences apply) hands
    /// over a handle instead of copying ~9 KB.
    #[inline]
    pub fn set_theme(&mut self, theme: impl Into<Rc<Theme>>) {
        self.theme = theme.into();
    }

    /// Drive one application frame for `win`, delegating to
    /// [`FrameCycle`] — see there for the pass order and what each pass
    /// resets. `stamp.time` is monotonic host time.
    pub(crate) fn frame<T: App>(
        &mut self,
        input: FrameInput,
        win: WindowToken,
        app: &mut T,
    ) -> FrameReport {
        FrameCycle::new(self).run(input, win, app)
    }

    /// Feed an palantir-native input event. Returns an [`InputDelta`]
    /// the host reads to decide whether to request a redraw — pointer
    /// moves over inert surfaces leave `requests_repaint` false so the
    /// host can skip the frame entirely. Animation/tooltip-delay wakes
    /// still drive paints independently via `FrameReport::repaint_after`.
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.input.on_input(event, &self.cascade)
    }

    // The input surface has three verbs, and every method below is one
    // of them:
    //
    // - `peek_*` reads live state and commits to nothing. `&self`. Use
    //   it when something that already woke this frame gated the read —
    //   the modifier held during a click, the pointer position at a
    //   press.
    // - `watch_*`, and any plain read that isn't a `peek_*`, reads *and*
    //   declares "wake me when this changes". `&mut self`. This is the
    //   default because forgetting it is a silent visual bug: paint
    //   derived from the pointer freezes on screen until some unrelated
    //   event forces a frame. Forgetting a peek only costs frames.
    // - `input_scope`, and `close_scope`, take authority over a stream for
    //   the ambient layer, silencing readers strictly below it.
    //
    // A plain read auto-watches exactly what its own result depends on:
    // `pointer_pos` → `MOVE`, `modifiers` → `MODIFIER`,
    // `key_pressed(sc)` → that chord. The event streams don't, because
    // which category should wake you isn't inferable from a
    // `&[PointerEvent]` — that's what the explicit `watch_*` flags are
    // for. Watches are idempotent and cleared pre-record: re-call each
    // active frame, stop calling to drop the wake.

    /// Declare interest in off-target pointer events of `flags`.
    pub fn watch_pointer(&mut self, flags: PointerWake) {
        self.input.watch_pointer(flags);
    }

    /// Declare interest in off-focus keyboard categories. Hotkey
    /// recorders, accel-underline UIs, command palettes that record
    /// before focus. Specific chords use [`Self::watch_key`].
    pub fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.input.watch_keyboard(flags);
    }

    /// Declare interest in one specific shortcut (e.g.
    /// `Shortcut::key(Key::Escape)`, `Shortcut::ctrl('K')`).
    /// Duplicate watchers collapse.
    pub fn watch_key(&mut self, sc: Shortcut) {
        self.input.watch_key(sc);
    }

    /// Unified pointer event stream captured this frame. Empty when
    /// no [`PointerWake`] watcher is active. Watchers `match`
    /// and filter by rect / button.
    ///
    /// Layer-gated like [`Self::keyboard_events`]: an overlay's scope
    /// empties the stream for every layer strictly below it, and for no
    /// other. Watches deliberately bypass hit-testing, so without this
    /// the scrim that blocks routed input would let a `Main`-layer
    /// pan/zoom watcher keep acting under an open modal.
    pub fn pointer_events(&self) -> &[PointerEvent] {
        self.input.pointer_events(self.forest.current_layer())
    }

    /// Unified keyboard event stream this frame —
    /// [`KeyboardEvent::Down`] from `KeyDown` events and
    /// [`KeyboardEvent::Text`] from typed/IME-committed text, in
    /// arrival order.
    ///
    /// Layer-gated exactly like [`Self::pointer_events`]: a
    /// An overlay's scope empties the stream for every layer strictly
    /// below, and for no other — so the claiming overlay's own body keeps
    /// reading, which is what lets a `TextEdit` inside a popup be typed
    /// into. The claim owner reads its scoped stream through
    /// its own layer.
    pub fn keyboard_events(&self) -> &[KeyboardEvent] {
        self.input.keyboard_events(self.forest.current_layer())
    }

    /// `true` if any [`KeyboardEvent::Down`] this frame matches
    /// `sc`. Iterates [`Self::keyboard_events`]; for repeat or
    /// stateful logic, iterate directly instead.
    ///
    /// Side-effect: auto-watches the chord for wake-up. Without
    /// this, palantir's keyboard wake-gate parks off-focus presses until the
    /// next unrelated frame, and the caller sees the event one gesture late.
    /// Pair with the call-it-every-frame discipline that the
    /// watch system already requires.
    pub fn key_pressed(&mut self, sc: Shortcut) -> bool {
        let layer = self.forest.current_layer();
        let parent = self.forest.current_parent_id();
        self.input.key_pressed(layer, parent, &self.cascade, sc)
    }

    /// Sugar for `key_pressed(Shortcut::key(Key::Escape))`.
    /// Used by overlays without exclusive keyboard capture, such as
    /// [`crate::widgets::modal::Modal`].
    pub fn escape_pressed(&mut self) -> bool {
        use crate::input::keyboard::Key;
        self.key_pressed(Shortcut::key(Key::Escape))
    }

    /// Re-record this frame after measure runs, for authoring code that
    /// realizes its record-time inputs were stale. Capped at one
    /// re-record per frame — so it cannot converge a feedback loop, only
    /// give a single retry.
    ///
    /// **The whole second pass is the record closure**, which makes this
    /// roughly a 2× frame; measure and arrange are ~2% of it. No widget
    /// this crate ships calls it any more. `Scroll` was the last, and it
    /// now resolves its bar geometry in the `layout::scrollbars` driver
    /// after measure instead. Prefer that shape — or handling the edge in
    /// [`App::update`], which runs before any
    /// recording — and reach for this only when neither fits.
    pub fn request_relayout(&mut self) {
        self.frame_runtime.relayout_requested = true;
    }

    /// Monotonic time of the current frame, accumulated from the
    /// per-frame `dt`s the host feeds in. Starts at zero on the first
    /// frame and only moves forward. Read-only on purpose: the clock is
    /// host-driven, and a direct write would desync it from the wake
    /// queue. Use for time-driven animation that needs a continuous
    /// clock rather than a tween toward a fixed target; pair with
    /// [`Self::request_repaint`] to keep the host awake. (Shape-level
    /// continuous motion like `Spinner`'s rides `PaintAnim` instead —
    /// sampled at encode time, no record-time clock read.)
    pub fn now(&self) -> Duration {
        self.frame_runtime.time
    }

    /// Request the mouse cursor shown for this window. Per record pass,
    /// last writer wins — record order is z-order, so the topmost
    /// interested widget's request lands. Reset to
    /// [`CursorIcon::Default`] at the top of every record pass; a widget
    /// that still wants a non-default cursor re-requests it each frame
    /// (typically off its hover/drag response). The host applies it
    /// after the frame, only on change; ignored in headless contexts.
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.window_requests.cursor = cursor;
    }

    /// Ask the host to switch this window's swapchain to `vsync`.
    ///
    /// Unlike [`Self::set_cursor`] this is a **one-shot request**, not a
    /// per-frame value: call it when the setting changes, not every
    /// frame. Applying one recreates the swapchain, which waits for the
    /// GPU to go idle — cheap to ask for repeatedly (the host drops a
    /// request matching the mode already in force) but not something to
    /// re-assert from a hot record pass.
    ///
    /// The host applies it after the frame and schedules the repaint that
    /// carries it, so an idle window still picks the change up. Ignored by
    /// hosts with no swapchain, which is every headless one.
    pub fn set_vsync(&mut self, vsync: Vsync) {
        self.window_requests.vsync = Some(vsync);
    }

    /// Ask the host to schedule another frame after this one. Cleared
    /// at the top of every `frame`; widgets/showcases that need
    /// continuous animation call this each frame to keep the host
    /// awake.
    pub fn request_repaint(&mut self) {
        tracing::trace!(
            target: "palantir.repaint",
            render_frame = self.frame_runtime.render_frame_id,
            "request_repaint",
        );
        self.frame_runtime.repaint_requested = true;
    }

    /// Schedule a one-shot wake at `now + after`. The entry persists
    /// across frames; the frame lifecycle drains entries whose deadline
    /// has fired at the top of each frame. Duplicate deadlines collapse
    /// (sorted + dedup'd), so re-requesting the same wake is a no-op.
    ///
    /// Callers don't need to re-request each frame. To cancel, schedule
    /// nothing else — the wake will fire once, the next frame will run
    /// briefly, and the queue drains.
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

    /// Open a new top-level OS window addressed by `token`. The window
    /// gets its own independent UI tree; [`App::update`] and
    /// [`App::record`] receive its `token`, and you can later poke it via
    /// [`HostHandle::request_repaint`](crate::HostHandle::request_repaint)
    /// or close it with [`Self::close_window`].
    ///
    /// Creation is deferred, not inline: the request is queued and the
    /// host (`WinitHost`) creates the real window on the event-loop
    /// thread right after this frame, so it's safe to call mid-record.
    /// Idempotent within a frame — record passes replay (cold-start
    /// warmup, double-layout pass B), so repeat calls for one `token`
    /// collapse to a single request with the last `config` winning. A
    /// `token` already in use by a live window is ignored with a
    /// warning. If native creation later fails,
    /// [`WinitHost::run`](crate::WinitHost::run) exits and returns the error.
    ///
    /// `token` is yours to define — an enum discriminant, an index, a
    /// document-id hash. It must be unique across live windows. `config`
    /// is the backend-agnostic [`WindowConfig`] (title + size); the
    /// window inherits the app-global GPU settings from startup.
    ///
    /// # Panics
    ///
    /// Panics on a host with no window lifecycle. The
    /// [`OffscreenHost`](crate::OffscreenHost) drives one window and cannot
    /// service the request, and dropping it silently would leave you believing
    /// a window appeared — an app that opens windows needs
    /// [`WinitHost`](crate::WinitHost).
    pub fn open_window(&mut self, token: WindowToken, config: WindowConfig) {
        if let Some(p) = self
            .window_requests
            .commands
            .opens
            .iter_mut()
            .find(|p| p.token == token)
        {
            p.config = config;
            return;
        }
        self.window_requests
            .commands
            .opens
            .push(PendingWindow { token, config });
    }

    /// Request that the window addressed by `token` close. Deferred like
    /// [`Self::open_window`] — the host removes it after this frame. The
    /// last window closing exits the event loop. No-op if `token` names
    /// no live window.
    ///
    /// # Panics
    ///
    /// Panics on a host with no window lifecycle, for the reason given on
    /// [`Self::open_window`]. Drop an
    /// [`OffscreenHost`](crate::OffscreenHost) to release its streams.
    pub fn close_window(&mut self, token: WindowToken) {
        self.window_requests.commands.closes.push(token);
    }

    /// `true` for the single frame where the OS asked to close this window
    /// (titlebar X). The window auto-closes after this frame **unless** you
    /// call [`Self::keep_open`] — so a simple app needs no close handling
    /// at all (X just works), while an app that wants a "save changes?"
    /// prompt vetoes the auto-close and shows a dialog:
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
    /// Always `false` in headless / offscreen contexts (no OS window).
    pub fn close_requested(&self) -> bool {
        self.window_frame.close_requested
    }

    /// Veto the auto-close pending from this frame's [`Self::close_requested`].
    /// The window stays open past this frame; close it for real later with
    /// [`Self::close_window`]. A no-op when no close was requested.
    pub fn keep_open(&mut self) {
        self.window_requests.close_vetoed = true;
    }

    /// This window's live geometry for persist-and-restore across launches.
    /// A computed view, not stored state: the logical inner size comes from
    /// [`Self::display`] (the single source of truth for surface size), and
    /// the physical outer position + maximized flag from the host-refreshed
    /// window-manager facts. Feed it back through
    /// [`WindowConfig::position`](crate::WindowConfig) / `inner_size` /
    /// `maximized` on the next launch to reopen where the user left off.
    /// `outer_position` is `None` on platforms that don't report it
    /// (Wayland). All-zero / `None` in headless contexts.
    pub fn window_geometry(&self) -> WindowGeometry {
        let logical = self.display.logical_size();
        WindowGeometry {
            inner_size: UVec2::new(
                (logical.w.round() as u32).max(1),
                (logical.h.round() as u32).max(1),
            ),
            outer_position: self.window_frame.position,
            maximized: self.window_frame.maximized,
        }
    }

    /// Mutable handle to this app's debug overlay; the guard derefs to
    /// `&mut DebugOverlayConfig`, so write fields straight on it
    /// (`ui.debug_overlay_mut().damage_rect = true`). The overlay is
    /// app-global: the write is visible to every window at once, and the
    /// host repaints idle windows so it shows everywhere — not just the
    /// window that handled the key. Drop the guard before other `Ui`
    /// calls; the `&mut self` borrow enforces that.
    pub fn debug_overlay_mut(&mut self) -> RefMut<'_, DebugOverlayConfig> {
        self.resources.diagnostics.overlay.borrow_mut()
    }

    /// Whether a window addressed by `token` is currently live. Reflects
    /// the set as of this frame's *start*, so a window opened or closed
    /// earlier *this* frame isn't reflected until the next one (the host
    /// drains [`Self::open_window`] / [`Self::close_window`] between
    /// frames). Use it as the source of truth for "is this window up?"
    /// instead of mirroring the state in app code — a window the user
    /// closed via its titlebar drops out of this set automatically.
    pub fn window_open(&self, token: WindowToken) -> bool {
        self.resources.windows.contains(token)
    }

    /// Attach a paint primitive to the active node. Direct text contributes to
    /// layout only on a leaf; container-owned text is an overlay shaped against
    /// that container's final padded width.
    pub fn add_shape<S: Lower>(&mut self, shape: S) {
        self.forest.add_shape(shape);
    }

    /// Upload an image and get back an owning [`ImageHandle`]. **Hold the
    /// handle** to keep the GPU texture resident — dropping the last
    /// clone frees it; there is no `unregister`. Reference it in
    /// [`Shape::image`](crate::Shape::image) every frame (`clone` it where it
    /// needs to live).
    /// The CPU bytes are dropped right after the upload.
    ///
    /// # Errors
    ///
    /// Returns an error when an image axis exceeds the selected device's 2D
    /// texture limit. A rejected image is never queued for upload. Standalone
    /// CPU recorders have no device limit and retain the original dimensions.
    pub fn register_image(&self, image: Image) -> Result<ImageHandle, RegisterImageError> {
        self.resources.images.register(image)
    }

    /// The largest width or height [`Self::register_image`] accepts — the
    /// selected device's `max_texture_dimension_2d`, and the *only* ceiling on
    /// a registered image, since palantir imposes none of its own. `None` for
    /// a standalone CPU recorder, which has no device to ask.
    ///
    /// Read it when deriving a texture from a larger source, so the downscale
    /// is sized against the device actually in use rather than a constant
    /// picked to stay under every device's limit. Registration *rejects* an
    /// over-limit image rather than shrinking it, so a host that wants the
    /// biggest texture a machine will take has to ask first.
    pub fn max_image_dimension(&self) -> Option<NonZeroU32> {
        self.resources.images.max_texture_dimension_2d()
    }

    /// Record a `GpuView` for widget `id`: upsert it into [`Self::gpu_views`]
    /// — minting the stable backend `TextureId` once (on first sight) and
    /// refreshing the app `paint` callback each frame — then append a
    /// [`ShapeRecord::Image`](crate::scene::shapes::record::ShapeRecord::Image)
    /// sourced from an
    /// [`ImageSource::GpuView`](crate::scene::shapes::paint::ImageSource::GpuView)
    /// carrying the view's `epoch` to the active node
    /// (the encoder recovers id + paint from the map by `id`).
    ///
    /// `repaint` is the widget's per-frame dirty flag. When set, the epoch
    /// bumps to the current render frame id, so the shape hash changes and the view
    /// repaints; when clear, the epoch is held stable, so the damage diff
    /// treats the view as unchanged and the encoder culls it (skipping its GPU
    /// paint and reusing last frame's pixels). First sight always paints (the
    /// texture doesn't exist yet). The entry rides the map's `removed` sweep
    /// when the widget disappears.
    pub(crate) fn gpu_view(
        &mut self,
        id: WidgetId,
        paint: Rc<RefCell<dyn GpuPaint>>,
        repaint: bool,
    ) {
        let epoch = self.frame_runtime.render_frame_id;
        let entry = match self.gpu_views.entry(id) {
            Entry::Occupied(e) => {
                let entry = e.into_mut();
                entry.paint = GpuPaintRef(paint);
                // Bump only on a repaint request; held stable otherwise so a
                // static view stays undamaged (culled, its paint skipped).
                if repaint {
                    entry.epoch = epoch;
                }
                entry
            }
            // First sight always paints — the texture doesn't exist yet.
            // The shared id source is disjoint from `self.gpu_views`.
            Entry::Vacant(e) => e.insert(GpuViewEntry {
                texture_id: self.resources.texture_ids.reserve(),
                paint: GpuPaintRef(paint),
                epoch,
            }),
        };
        self.forest.add_gpu_view(entry.epoch);
    }

    /// Format `args` directly into the record-pass text storage and return
    /// an arena-backed [`InternedStr`]. Pass the returned value to
    /// any text-taking widget. The bytes are already in the destination
    /// buffer, so same-arena lowering is zero-copy and steady-state authoring
    /// of dynamic labels skips per-call `String` allocations.
    ///
    /// **Valid only for the pass that minted it.** Lower it here, in this
    /// window; holding it into a later frame, into the second pass of a
    /// double-layout frame, or into another window panics — the bytes it
    /// spans are gone by then. Persistent application text should stay in
    /// its source `String` and be interned again each frame, which costs
    /// the one `memcpy` the borrowed path pays anyway.
    #[must_use]
    pub fn fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.forest.record_store.intern_fmt(args)
    }

    /// Normalize borrowed, owned, or already-interned text into an
    /// [`InternedStr`]. Borrowed and owned inputs are copied into the
    /// record-pass text arena; an [`InternedStr`] passes through unchanged.
    /// Format-less twin of [`Self::fmt`] with the same retention rules.
    #[must_use]
    pub fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr {
        match text.into() {
            TextInput::Borrowed(text) => self.forest.record_store.intern_str(text),
            TextInput::Owned(text) => self.forest.record_store.intern_str(&text),
            TextInput::Interned(text) => text,
        }
    }

    /// Append `shape` to the active node and register `anim` against
    /// it. The encoder samples `anim` at paint time and folds the
    /// resulting `PaintMod` into the shape's brush; `post_record`
    /// folds the anim's `next_wake` into `repaint_wakes` so the
    /// caller doesn't manage scheduling. Drops silently if the shape
    /// itself was noop-collapsed (zero stroke + transparent fill,
    /// etc.) — `PaintAnim` can't make a zero shape paintable.
    pub(crate) fn add_shape_animated<S: Lower>(&mut self, shape: S, anim: PaintAnim) {
        self.forest.add_shape_animated(shape, anim);
    }

    /// Open a side layer — an arena that paints above the `Main` tree,
    /// escapes ancestor clip, and hit-tests on top of it. Configure the
    /// placement on the returned [`LayerScope`] and terminate with
    /// [`LayerScope::show`]; the defaults anchor at the surface origin
    /// with the whole surface available.
    ///
    /// Recordable from the `Main` baseline or nested inside a
    /// higher-ranked side layer's body (a tooltip raised from a popup or
    /// modal). A nested layer must sit strictly above the current scope
    /// in `Layer::PAINT_ORDER`, else it would paint under its parent.
    pub fn layer(&mut self, layer: Layer) -> LayerScope<'_> {
        LayerScope::new(self, layer)
    }

    /// Withdraw an [`input_scope`](crate::Configure::input_scope) this
    /// pass recorded, so the next resolution does not see it.
    ///
    /// **The pass you call it in is unaffected.** A scope path is
    /// resolved once at pass start against a cascade that is one frame
    /// old, so an overlay that decides it is closing has already
    /// recorded its scope and would go on owning input for the frame
    /// after it is gone — long enough to swallow the click that lands
    /// where it used to be. Call it on the frame the overlay resolves
    /// that it closed; a scope that simply stops recording needs
    /// nothing.
    pub fn close_scope(&mut self, id: WidgetId) {
        self.input.close_scope(id);
    }

    /// Resolve `node`'s stable [`WidgetId`] for this frame and hand
    /// back the [`Widget`] pairing that id with the node. This is
    /// the public entry a widget author calls first: read
    /// [`Self::response_for`] / per-widget [`Self::state_mut`] against
    /// `widget.id()` (theme picking off the prior frame, animation
    /// slots, sub-id derivation), mutate `widget.node` as needed,
    /// then record via [`Widget::record`]. Every built-in widget follows
    /// this resolve-once-then-`node` shape; see
    /// `examples/custom_widget.rs`.
    ///
    /// Resolution is the egui `make_persistent_id` analogue: an
    /// [`crate::Configure::id_salt`] salt *and* a `#[track_caller]`
    /// auto id both resolve to `parent.with(id)` (so identity tracks
    /// tree position, not global record order, keeping per-site state
    /// stable across frames and sibling reorders); only an explicit
    /// `.id(id)` resolves verbatim. Parent context is the
    /// most-recently-opened node in the current layer — `Layer::Main`'s
    /// synthetic viewport counts as a parent with a frame-stable id, so
    /// widgets get stable ids with no layer carve-out. `SeenIds`
    /// **eagerly disambiguates**: a salt colliding with a sibling
    /// already recorded this frame is bumped to a fresh occurrence
    /// slot, so the resolved id matches what the tree, cascade, and
    /// `response_for` will see.
    ///
    /// **Record exactly once**: the resolution reserves this frame's
    /// occurrence slot for the id, and the matching [`Widget::record`]
    /// call claims it. Dropping the `Widget` without recording leaves
    /// the slot dangling (a second same-salt widget this frame would
    /// reuse the id); recording twice panics.
    /// **The one resolver.** A widget whose recorded root is
    /// framework-built — `Modal`'s backdrop, `Scroll`'s outer wrapper,
    /// `TextEdit`'s measured frame — resolves here on the node it was
    /// handed, then overwrites [`Widget::node`] with the root it
    /// actually records once the id has unlocked the state to build it.
    /// The id is the part that must not move; the node is explicitly
    /// open until `record` consumes it.
    #[must_use = "record the widget with Widget::record"]
    pub fn widget(&mut self, node: Node) -> Widget {
        Widget::new(self.forest.widget_id(node.salt), node)
    }

    /// Snapshot of input/cascade state for a widget. `rect` and
    /// `disabled` are from the previous frame's cascade; the interaction
    /// fields (`pressed`, `hovered`, `drag_started`, `drag_delta`, …) are
    /// computed against this frame's input state.
    ///
    /// **Read it during the frame's record** — as every widget does. The
    /// interaction half is gated on a `frame_quiescent` snapshot taken
    /// once at record-pass start, so a read taken *between* frames would
    /// reflect the previous frame's input, not events fed since. Reading
    /// earlier in the same record than the widget's own node is fine —
    /// e.g. baking a drag delta into a widget's position before recording it.
    pub fn response_for(&self, id: WidgetId) -> ResponseState {
        let mut state = self.input.response_for(id, &self.cascade, &self.layout);
        // Cascade lags one frame; OR this frame's ancestor-disabled so
        // a freshly-disabled subtree paints disabled on its first frame.
        state.disabled |= self.forest.current_scratch().ancestor_disabled();
        // The interaction half was routed against the stale cascade, so
        // without this a freshly-disabled widget reports hovered /
        // clicked alongside disabled for one frame — a combination the
        // steady-state hit index can never produce (disabled entries
        // carry `Sense::NONE`), and one that lets a click land on
        // just-disabled UI.
        if state.disabled {
            state = ResponseState {
                rect: state.rect,
                layout_rect: state.layout_rect,
                transform: state.transform,
                disabled: true,
                focused: state.focused,
                ..ResponseState::default()
            };
        }
        state
    }

    /// Cross-frame state row for `id`, `T::default()` on first
    /// access. Rows for `WidgetId`s not recorded this frame are
    /// evicted in `finalize_frame`, once per `Ui::frame` after the
    /// final record pass. Type collisions at one `id` are NOT
    /// detected — each `T` lives in its own store, so two call sites
    /// using different types at the same id silently coexist (see the
    /// `state` module doc).
    pub fn state_mut<S: Default + 'static>(&mut self, id: WidgetId) -> &mut S {
        self.state.get_or_insert_with(id, S::default)
    }

    /// Read-only peek at the cross-frame state row for `id`. `None` if
    /// nothing has been stored for `(id, T)` yet — does not allocate or
    /// mutate. Use this on the `&Ui` side (probes, hit-test helpers,
    /// "is this menu open?" checks) where `state_mut`'s `&mut Ui`
    /// receiver would be a needless borrow upgrade.
    pub fn try_state<S: 'static>(&self, id: WidgetId) -> Option<&S> {
        self.state.try_get::<S>(id)
    }

    /// Mutable peek at an existing cross-frame state row. `None` if
    /// `(id, T)` has never been stored; unlike [`Self::state_mut`], this
    /// does not allocate a typed store or insert a default row.
    pub fn try_state_mut<S: 'static>(&mut self, id: WidgetId) -> Option<&mut S> {
        self.state.try_get_mut::<S>(id)
    }

    /// Advance an animation row keyed by `(id, slot)` and return the
    /// current value. `spec = None` snaps to `target` and drops any
    /// stale row without requesting a repaint — the canonical
    /// "no animation" path.
    // Generic and reached through cross-module widget helpers. Keep the
    // dominant no-map/no-spec return in the widget's block so a static theme
    // doesn't pay an outlined call plus a large `V` return-slot handoff.
    #[inline(always)]
    pub fn animate<V: Animatable>(
        &mut self,
        id: WidgetId,
        slot: impl Into<AnimSlot>,
        target: V,
        spec: Option<AnimSpec>,
    ) -> V {
        // Hottest path: no spec, no typed map for `V` ever allocated.
        // Skip the `slot.into()`, filter closure, and TypeId-keyed
        // HashMap probe — they're per-widget per-frame on a widget
        // that never animates (the dominant case in static UIs).
        if self.anim.by_type.is_empty() && spec.is_none_or(|s| s.is_instant()) {
            return target;
        }
        let slot = slot.into();
        // Merge `None` and instant-degenerate specs (`Duration { secs ≈ 0 }`)
        // into one snap path. `tick` then handles only real motion.
        let Some(spec) = spec.filter(|s| !s.is_instant()) else {
            // Drop stale row so a future `Some(_)` starts fresh from
            // `target`. `try_typed_mut` avoids allocating a typed map
            // just to remove from one that may not exist.
            if let Some(typed) = self.anim.try_typed_mut::<V>() {
                typed.rows.remove(&(id, slot));
            }
            return target;
        };
        let r = self.anim.typed_mut::<V>().tick(
            id,
            slot,
            target,
            spec,
            self.frame_runtime.dt,
            self.frame_runtime.render_frame_id,
        );
        if !r.settled {
            self.frame_runtime.repaint_requested = true;
        }
        r.current
    }

    /// Currently focused widget id, or `None`.
    pub fn focused_id(&self) -> Option<WidgetId> {
        self.input.focused
    }

    /// True when keyboard focus sits on `ancestor` or any widget
    /// recorded inside its subtree — per the most recent cascade run,
    /// i.e. one frame of lag, the same timing as [`Self::response_for`].
    /// `false` when nothing is focused or `ancestor` wasn't recorded.
    /// Layers are separate trees, so focus on a popup never counts as
    /// within the popup's anchor. Lets a caller that skips recording
    /// off-screen subtrees keep the one holding an in-progress edit
    /// alive without enumerating every focusable widget it contains.
    pub fn focus_within(&self, ancestor: WidgetId) -> bool {
        self.input
            .focused
            .is_some_and(|f| self.cascade.is_within(f, ancestor))
    }

    /// True when the pointer's hover target is `ancestor` or any widget
    /// recorded inside its subtree — the hover sibling of
    /// [`Self::focus_within`], same cascade timing and layer caveats.
    /// Prefer this over testing `Self::pointer_pos` against a rect for
    /// "is the pointer on me" styling: it's occlusion-aware (a panel
    /// stacked on top wins the pointer), and because it's a pure
    /// function of the hover *target*, its value can only change when
    /// the target changes — which is exactly when a repaint is already
    /// scheduled, so no `MOVE` watch is needed to stay fresh.
    pub fn hover_within(&self, ancestor: WidgetId) -> bool {
        self.input
            .hovered
            .is_some_and(|h| self.cascade.is_within(h, ancestor))
    }

    /// Active `Display` (physical surface size + scale factor). Read
    /// by example/demo code that wants to inject synthetic input
    /// coordinates without threading window dimensions through itself.
    pub fn display(&self) -> Display {
        self.display
    }

    /// This frame's monotonic index, counting the frames authoring code
    /// actually ran on — the clock retained state stamps to notice it was
    /// *skipped*.
    ///
    /// Code that runs only while its surface is on screen stamps this and
    /// compares on the next run: a gap tells it the surface was away,
    /// without anything having to run while it was. Consecutive values, and
    /// repeats within one frame, both mean "still here" — a settling second
    /// pass observes the same value as the first, and a paint-only frame
    /// (which records nothing, so no such code ran) advances nothing.
    /// [`Self::render_frame_id`] is the peer that counts painted frames.
    pub fn frame_id(&self) -> u64 {
        self.frame_runtime.frame_id
    }

    /// This frame's monotonic index among the frames that reached the
    /// screen, bumped once per `Self::frame` before either record pass —
    /// so both passes of one frame observe the same value.
    ///
    /// For code measuring what the display saw. It counts paint-only frames
    /// too, so a gap in it does *not* mean the reader was skipped: an idle
    /// window painting a caret blink advances this while no record pass runs
    /// at all. Anything asking "was I skipped" wants [`Self::frame_id`].
    pub fn render_frame_id(&self) -> u64 {
        self.frame_runtime.render_frame_id
    }

    /// Shape `run` and return its geometry — caret positions,
    /// click-to-offset, selection rects.
    ///
    /// Only for the byte↔position mapping. **Measuring and painting text
    /// need nothing from here**: record a `Shape::Text` and layout shapes
    /// it, contributing its size like any other content.
    ///
    /// **`&mut self` is a choice, not a requirement.** The shaper carries
    /// its own interior mutability, so `&self` compiles perfectly well —
    /// it just moves where this fails.
    ///
    /// Probing is not read-only: a cache miss shapes the run, and an
    /// evicted buffer is rebuilt on the spot. So the returned probe holds
    /// the shaper's *exclusive* lease until it drops, and borrowing `Ui`
    /// mutably is what makes the compiler's idea of exclusivity agree
    /// with the `RefCell`'s — a second overlapping probe becomes E0499 at
    /// build time instead of a panic in a running app. The cost is that
    /// the borrow is coarse: it locks all of `Ui`, not just the shaper,
    /// because a method signature cannot say "exclusive on this field".
    ///
    /// Sequential probes are fine — end one with a block, or let a
    /// temporary drop at the end of its statement, before taking the
    /// next.
    ///
    /// Cheap to call repeatedly on a stable run: shaped buffers are
    /// cached by the run's own parameters, so a per-frame probe of
    /// unchanged text is a lookup, not a reshape.
    pub fn probe_text<'a>(&'a mut self, run: TextRun<'a>) -> TextProbe<'a> {
        TextProbe::new(self.resources.text.layout(run.request()))
    }

    /// Programmatically set or clear focus. Bypasses [`FocusPolicy`].
    pub fn request_focus(&mut self, id: Option<WidgetId>) {
        self.input.focused = id;
    }

    /// Current pointer position in logical pixels (surface space), or
    /// `None` if the pointer has left the surface.
    ///
    /// `&mut` because reading it auto-asserts a [`PointerWake::MOVE`]
    /// watch: output derived from the raw pointer may change on any
    /// move, so moves must keep triggering repaints even when the hover
    /// target doesn't change — otherwise pointer-derived paint (e.g. a
    /// proximity highlight) goes stale on screen until an unrelated
    /// event forces a frame. Like every watch, it lapses as soon as a
    /// record pass stops reading. Use [`Self::pointer_local`] when the
    /// output should be relative to a widget, and
    /// [`Self::peek_pointer_pos`] when this frame is already awake for
    /// another reason.
    ///
    /// This is the *raw* pointer, so it ignores who owns input. For "is
    /// the pointer on me" styling prefer [`Self::hover_within`], which
    /// routes through the hit index and is therefore occlusion- and
    /// overlay-aware.
    pub fn pointer_pos(&mut self) -> Option<glam::Vec2> {
        self.watch_pointer(PointerWake::MOVE);
        self.input.pointer_pos
    }

    /// Current pointer position in `id`'s pre-transform local logical
    /// coordinates. `None` when the pointer is off-surface or the
    /// widget did not arrange in the previous frame.
    ///
    /// Reading auto-asserts a [`PointerWake::MOVE`] watch, keeping
    /// pointer-local paint reactive while the cursor moves within one
    /// hover target. [`Self::peek_pointer_local`] is the unwatched read.
    pub fn pointer_local(&mut self, id: WidgetId) -> Option<glam::Vec2> {
        self.watch_pointer(PointerWake::MOVE);
        self.input
            .pointer_local_for(id, &self.cascade, &self.layout)
    }

    /// Currently-held modifier keys. State persists across frames; only
    /// `ModifiersChanged` events mutate it.
    ///
    /// Reading auto-asserts a [`KeyboardWake::MODIFIER`] watch, so
    /// modifier-dependent paint updates on both press and release
    /// without another input event. When the read is instead gated on
    /// something that already woke this frame — the overwhelmingly
    /// common `if response.left.clicked() { … }` — use
    /// [`Self::peek_modifiers`] and don't pay for the wake.
    pub fn modifiers(&mut self) -> Modifiers {
        self.watch_keyboard(KeyboardWake::MODIFIER);
        self.input.modifiers
    }

    /// [`Self::pointer_pos`] without the [`PointerWake::MOVE`] watch.
    ///
    /// Correct exactly when something else already guarantees a frame
    /// whenever this value could matter — reading the press position
    /// inside a click branch, say. Wrong when paint is derived from it
    /// continuously: the pointer will move, no frame will run, and the
    /// stale result stays on screen.
    pub fn peek_pointer_pos(&self) -> Option<glam::Vec2> {
        self.input.pointer_pos
    }

    /// [`Self::pointer_local`] without the [`PointerWake::MOVE`] watch.
    /// Same caveat as [`Self::peek_pointer_pos`].
    pub fn peek_pointer_local(&self, id: WidgetId) -> Option<glam::Vec2> {
        self.input
            .pointer_local_for(id, &self.cascade, &self.layout)
    }

    /// [`Self::modifiers`] without the [`KeyboardWake::MODIFIER`] watch
    /// — "which modifiers were held when the thing that woke this frame
    /// happened", the shape almost every modifier read actually wants.
    ///
    /// Use [`Self::modifiers`] instead when a bare modifier press or
    /// release must repaint on its own, with nothing else happening: an
    /// accel-underline overlay that appears on Alt, a modifier-state
    /// debug readout, a drag whose snap targets change under Ctrl while
    /// the pointer holds still.
    pub fn peek_modifiers(&self) -> Modifiers {
        self.input.modifiers
    }

    pub fn focus_policy(&self) -> FocusPolicy {
        self.input.focus_policy
    }

    /// Set the press-on-non-focusable behavior. See [`FocusPolicy`].
    pub fn set_focus_policy(&mut self, p: FocusPolicy) {
        self.input.focus_policy = p;
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod harness;

#[cfg(test)]
pub(crate) mod internals {
    use crate::input::InputState;
    use crate::ui::Ui;

    impl Ui {
        /// The input machine itself, for tests that assert on routing
        /// state the public surface deliberately does not expose —
        /// capture targets, the raw per-layer streams, the action flag.
        ///
        /// Gated so that **production widget code cannot reach past
        /// `Ui`'s public input API**. That is the whole point: every
        /// widget this crate ships authors itself through the same
        /// surface a caller's own widget has, so the surface cannot
        /// quietly become insufficient without a widget in here
        /// noticing first.
        pub(crate) fn input(&self) -> &InputState {
            &self.input
        }

        /// [`Self::input`], mutably — for tests that *drive* routing
        /// state rather than assert on it (planting focus, taking the
        /// action flag).
        pub(crate) fn input_mut(&mut self) -> &mut InputState {
            &mut self.input
        }
    }
}

#[cfg(test)]
mod tests;
