//! The recorder: [`Ui`], the single handle a widget or a host authors a
//! frame through, and the retained state that frame reads and writes.
//!
//! Everything a frame needs that is not an engine lives here — the forest,
//! the theme, per-widget state, input, animation, the display, and the frame
//! clock. The engines that consume a recording (layout, cascade, damage) sit
//! on a [`FrameEngines`] the frame driver owns, so nothing reachable
//! from `Ui` can reach them.

#[cfg(feature = "bench")]
pub(crate) mod bench;
mod frame_cycle;
pub(crate) mod frame_engines;
pub(crate) mod frame_report;
pub(crate) mod frame_runtime;
pub(crate) mod frame_stamp;
pub(crate) mod layer_scope;
pub(crate) mod resources;
pub(crate) mod state;

use crate::animation::AnimMap;
use crate::animation::anim_slot::AnimSlot;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::animatable::Animatable;
use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::diagnostics::frame_stats::FrameStats;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::icons::icon_set::IconSet;
use crate::icons::icon_table::IconTable;
use crate::input::input_event::InputEvent;
use crate::input::input_state::InputState;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::pointer::PointerEvent;
use crate::input::policy::FocusPolicy;
use crate::input::policy::InputPolicy;
use crate::input::response::input_delta::InputDelta;
use crate::input::response::pointer_action::PointerAction;
use crate::input::response::response_state::ResponseState;
use crate::input::shortcut::Shortcut;
use crate::input::watch::{KeyboardWake, PointerWake};
use crate::layout::Layout;
use crate::layout::scrollbars::ScrollbarsDef;
use crate::layout::types::layout_mode::{GridDefId, ScrollbarsDefId};
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::image::Image;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::size::Size;
use crate::primitives::text_input::TextInput;
use crate::primitives::widget_id::WidgetId;
use crate::renderer::frontend::FrameScene;
use crate::renderer::gpu_paint::gpu_paint_ref::GpuPaintRef;
use crate::renderer::gpu_paint::gpu_views::GpuViews;
use crate::renderer::image_registry::image_handle::ImageHandle;
use crate::renderer::texture_limit::RegisterImageError;
use crate::scene::cascade::Cascade;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::ident::Ident;
use crate::scene::record_store::RecordStore;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::paint_anim::PaintAnim;
use crate::shape::Lower;
use crate::text::error::FontLoadError;
use crate::text::font_family::FontFamily;
use crate::text::font_source::FontSource;
use crate::text::probe::TextProbe;
use crate::text::run::TextRun;
use crate::ui::frame_cycle::FrameCycle;
use crate::ui::frame_engines::FrameEngines;
use crate::ui::frame_report::FrameReport;
use crate::ui::frame_runtime::FrameRuntime;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::frame_stamp::FrameInput;
use crate::ui::layer_scope::LayerScope;
use crate::ui::resources::UiResources;
use crate::ui::state::StateMap;
use crate::widgets::theme::Theme;
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
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

/// Recorder + input/response broker. All public coordinates are
/// logical pixels (DIPs); `Display::scale_factor` — the platform's factor
/// times [`Ui::set_user_scale`]'s — converts to physical at the wgpu
/// boundary. Frame scheduling state is retained internally.
///
/// **Every field is private, and that is the boundary.** Everything outside
/// this module — widgets, the host, the encoder — reaches this state through
/// a method, so the surface a caller's own widget has is the surface every
/// widget in this crate is built on, and it cannot quietly become
/// insufficient without a widget in here noticing first. An open field is not
/// a shortcut but a second, unnamed API: it exposes its type's whole surface
/// transitively, and the reach it invites is silent. The rule binds inside
/// this module too, where the fields *are* reachable: a field in scope is
/// shorter than the [`Self::now`] / [`Self::display`] /
/// [`Self::debug_overlay`] that answers the same question, so the bypass
/// happens without anyone deciding on it.
///
/// The in-crate suites reach past this through the test-gated `internals`
/// mod at the end of this file, which does not exist in a shipped build. The
/// layout, cascade and damage engines are not reachable at all: they live on
/// a `FrameEngines`, which the frame driver owns.
#[derive(Debug)]
pub struct Ui {
    forest: Forest,
    /// Refcounted, deliberately: a widget that styles *from the theme*
    /// has to hold the bundle across the `&mut Ui` its own `show` takes,
    /// and a plain `&ui.theme` borrow cannot survive that reborrow. The
    /// `Rc` so that laundering the borrow is a refcount bump:
    /// [`Self::theme`] hands out the handle, [`Self::set_theme`] is the
    /// write side. Owning the bundle directly instead makes every such
    /// site clone one — 640 B for a `ButtonTheme`, 692 B for a
    /// `TextEditTheme`, per widget per frame.
    theme: Rc<Theme>,
    /// Cross-frame widget state: per-type dense stores keyed by
    /// `WidgetId` (see [`StateMap`]).
    state: StateMap,
    /// Live `GpuView`s — the only `GpuView` bookkeeping on the `Ui`. The
    /// shape records only the redraw epoch, and the encoder looks the view
    /// up here by the node's `WidgetId`. Swept by the same `removed` set
    /// as [`StateMap`].
    gpu_views: GpuViews,
    resources: UiResources,
    layout: Layout,
    /// Cascaded clip/disabled/invisible/transform per node + global
    /// hit index. Written by `CascadeEngine::run` in the paint phase
    /// and read by the encoder, input dispatch, and damage compute.
    cascade: Cascade,
    input: InputState,
    display: Display,
    anim: AnimMap,
    /// Retained frame clock, wake queue, repaint/relayout flags, and prior-frame
    /// validity state — the scheduling half of a frame, as against the
    /// authored tables above.
    frame_runtime: FrameRuntime,
    /// Recorder-to-host requests retained across frames.
    window_requests: WindowRequests,
    /// Host-to-recorder facts refreshed before each windowed frame.
    window_frame: WindowFrameState,
}

/// The widget- and host-facing authoring API: input feed, watches,
/// repaint/relayout requests, shape recording, per-widget state, and
/// animation, plus the crate-facing handles a host needs
/// (construction, `Self::frame`, `Self::frame_scene`) and the four-method
/// recorder↔host seam.
///
/// Wide by design. Most of what follows is a one-line delegation, and that is
/// the shape an immediate-mode recorder wants: one namespace a widget author
/// types, over subsystems that stay sealed behind it.
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
            gpu_views: &self.gpu_views,
            display: self.display,
            time: self.frame_runtime.time,
        }
    }

    /// Construct a per-window `Ui` from its app-global capabilities. Each `Ui`
    /// creates its own [`Forest`], whose retained record payloads
    /// remain isolated from other windows' record passes.
    pub(crate) fn new(resources: UiResources) -> Self {
        Self {
            resources,
            forest: Default::default(),
            theme: Default::default(),
            state: Default::default(),
            gpu_views: Default::default(),
            layout: Default::default(),
            cascade: Default::default(),
            input: Default::default(),
            display: Default::default(),
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
    /// borrow, and the alternative is cloning the whole bundle:
    ///
    /// ```ignore
    /// let theme = ui.theme().clone();
    /// Button::new().label("File").style(&theme.button).show(ui);
    /// ```
    #[inline]
    pub fn theme(&self) -> &Rc<Theme> {
        &self.theme
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
    ///
    /// `engines` is the caller's, not this recorder's: measure, cascade and
    /// damage all run incrementally off caches that belong to whoever drives
    /// frames, and keeping them off `Ui` is what puts them out of authoring
    /// code's reach. Pass the same one every frame — a fresh set discards the
    /// measure cache, the previous cascade and the damage baseline, so the
    /// next frame repaints in full.
    pub(crate) fn frame<T: App>(
        &mut self,
        engines: &mut FrameEngines,
        input: FrameInput,
        win: WindowToken,
        app: &mut T,
    ) -> FrameReport {
        FrameCycle::new(self, engines).run(input, win, app)
    }

    /// Feed an palantir-native input event. Returns an [`InputDelta`]
    /// the host reads to decide whether to request a redraw — pointer
    /// moves over inert surfaces leave `requests_repaint` false so the
    /// host can skip the frame entirely. Animation/tooltip-delay wakes
    /// still drive paints independently via `FrameReport::repaint_after`.
    #[inline]
    pub fn on_input(&mut self, event: InputEvent) -> InputDelta {
        self.input.on_input(event, &self.cascade, self.now())
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
    #[inline]
    pub fn watch_pointer(&mut self, flags: PointerWake) {
        self.input.watch_pointer(flags);
    }

    /// Declare interest in off-focus keyboard categories. Hotkey
    /// recorders, accel-underline UIs, command palettes that record
    /// before focus. Specific chords use [`Self::watch_key`].
    #[inline]
    pub fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.input.watch_keyboard(flags);
    }

    /// Declare interest in one specific shortcut (e.g.
    /// `Shortcut::key(Key::Escape)`, `Shortcut::ctrl('K')`).
    /// Duplicate watchers collapse.
    #[inline]
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
    #[inline]
    pub fn pointer_events(&self) -> &[PointerEvent] {
        self.input.pointer_events(self.forest.current_layer())
    }

    /// This frame's presses, in arrival order.
    ///
    /// Layer-gated exactly like [`Self::pointer_events`]: a
    /// An overlay's scope empties the stream for every layer strictly
    /// below, and for no other — so the claiming overlay's own body keeps
    /// reading, which is what lets a `TextEdit` inside a popup be typed
    /// into. The claim owner reads its scoped stream through
    /// its own layer.
    #[inline]
    pub fn keyboard_events(&self) -> &[KeyPress] {
        self.input.keyboard_events(self.forest.current_layer())
    }

    /// `true` if any press this frame matches
    /// `sc`. Iterates [`Self::keyboard_events`]; for repeat or
    /// stateful logic, iterate directly instead.
    ///
    /// Side-effect: auto-watches the chord for wake-up. Without
    /// this, palantir's keyboard wake-gate parks off-focus presses until the
    /// next unrelated frame, and the caller sees the event one gesture late.
    /// Pair with the call-it-every-frame discipline that the
    /// watch system already requires.
    #[inline]
    pub fn key_pressed(&mut self, sc: Shortcut) -> bool {
        let layer = self.forest.current_layer();
        let parent = self.forest.current_parent_id();
        self.input.key_pressed(layer, parent, &self.cascade, sc)
    }

    /// Sugar for `key_pressed(Shortcut::key(Key::Escape))`.
    /// Used by overlays without exclusive keyboard capture, such as
    /// [`crate::widgets::modal::Modal`].
    #[inline]
    pub fn escape_pressed(&mut self) -> bool {
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
        // Record-pass only. `FrameCycle::run` clears the flag before
        // handing the `Ui` to the app, so a call from outside a record —
        // an input handler, or `App::update` — sets a bit the very next
        // line drops, and the retry never happens. The assert is what
        // makes that loud. `App::update` already runs before recording,
        // so code reaching for this from there wants no retry at all.
        assert!(
            self.forest.is_recording(),
            "Ui::request_relayout outside a record pass: it re-runs *this* \
             frame's record, so there is nothing for it to retry — drop the \
             call, or move the work into App::update",
        );
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
    #[inline]
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
    #[inline]
    pub fn set_cursor(&mut self, cursor: CursorIcon) {
        self.window_requests.levels.cursor = cursor;
    }

    /// Set this window's presentation pacing.
    ///
    /// A **level**, retained across frames — [`Self::vsync`] reads it back,
    /// so this is the source of truth for "is vsync on?" rather than
    /// something app code mirrors. Setting the mode already in force costs
    /// nothing: the host diffs against the swapchain it has open and only
    /// then reconfigures, so a control can write its checkbox's value every
    /// frame.
    ///
    /// Applying a real change recreates the swapchain, which waits for the
    /// GPU to go idle. The host does that after the frame and schedules the
    /// repaint that carries it, so an idle window still picks the change up.
    /// Inert on hosts with no swapchain, which is every headless one — the
    /// level is still recorded and still reads back.
    #[inline]
    pub fn set_vsync(&mut self, vsync: Vsync) {
        self.window_requests.levels.vsync = vsync;
    }

    /// This window's presentation pacing, as last set by
    /// [`Self::set_vsync`] or as the host opened the swapchain with.
    ///
    /// Read it as the source of truth instead of mirroring the setting in
    /// app code — the same position [`Self::window_open`] takes for window
    /// liveness. A host launched with an explicit backend present mode
    /// reports whichever of the two states that mode paces like.
    #[inline]
    pub fn vsync(&self) -> Vsync {
        self.window_requests.levels.vsync
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
        self.window_requests.commands.open(token, config);
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
    #[inline]
    pub fn close_window(&mut self, token: WindowToken) {
        self.window_requests.commands.close(token);
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
    #[inline]
    pub fn close_requested(&self) -> bool {
        self.window_frame.close_requested
    }

    /// Veto the auto-close pending from this frame's [`Self::close_requested`].
    /// The window stays open past this frame; close it for real later with
    /// [`Self::close_window`]. A no-op when no close was requested.
    #[inline]
    pub fn keep_open(&mut self) {
        self.window_requests.close_vetoed = true;
    }

    // The recorder↔host seam. Four methods, and between them they are the
    // whole contract: two the host writes before a frame, two the host reads
    // after one. Nothing else on a host may touch recorder state — that is
    // what keeps `window_requests` / `window_frame` / the record arena
    // private, and what stopped the driver reading `display.physical` and
    // the debug-overlay flags off their fields when `Ui::display` and
    // `Ui::debug_overlay` already answered both.

    /// Publish this frame's window-manager facts, refreshed by the host
    /// before it runs the frame.
    ///
    /// Asserts the previous frame's close veto did not survive:
    /// [`Self::drain_window_output`] clears it on the way out and every
    /// windowed frame reaches that drain, occluded ones included, so a veto
    /// still standing here means the drain was skipped. Checked rather than
    /// cleared — a second clear would be a write that can never change
    /// anything, while the check fails if that ever stops holding.
    #[cfg(feature = "winit")]
    pub(crate) fn set_window_facts(&mut self, facts: WindowFrameState) {
        debug_assert!(
            !self.window_requests.close_vetoed,
            "a veto outlived the frame that raised it",
        );
        self.window_frame = facts;
    }

    /// Seed the pacing level from the swapchain the host actually opened, so
    /// [`Self::vsync`] is truthful before any frame runs and a control
    /// writing its own value back does not reconfigure an explicitly
    /// configured present mode out from under the host.
    #[cfg(feature = "winit")]
    #[inline]
    pub(crate) fn seed_vsync(&mut self, vsync: Vsync) {
        self.window_requests.levels.vsync = vsync;
    }

    /// Drain this frame's window scratch into `commands` and return the
    /// levels the host applies afterwards — see
    /// [`WindowRequests::drain`], which owns the close settlement and the
    /// veto's one-frame life. This clears the frame-scoped window state
    /// beside it.
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

    /// This frame's record payloads, for the GPU submit that dereferences the
    /// indices the draw list carries. Valid until the next record pass
    /// refills the arena.
    #[inline]
    pub(crate) fn record_store(&self) -> &RecordStore {
        &self.forest.record_store
    }

    /// This window's live geometry for persist-and-restore across launches.
    /// A computed view, not stored state: the inner size comes from
    /// [`Self::display`] (the single source of truth for surface size), and
    /// the placement from the host-refreshed window-manager facts. Feed it
    /// back through [`WindowConfig::placement`](crate::WindowConfig) and
    /// `inner_size` on the next launch to reopen where the user left off.
    /// The placement's position is `None` on platforms that don't report
    /// one (Wayland). All-zero / `None` in headless contexts.
    ///
    /// **In the window manager's logical pixels**, not the UI's — see
    /// [`Display::system_logical_size`]. This size is going back to the
    /// platform, which never hears about [`Self::set_user_scale`], so
    /// dividing by the product instead would shrink the window by the user
    /// scale on every launch.
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

    /// This app's debug-overlay flags, by value.
    ///
    /// Read-modify-write through [`Self::set_debug_overlay`] rather than
    /// through a borrow guard: the config is a handful of `Copy` flags, and
    /// a guard could not survive the widget calls that sit between reading
    /// a toggle's current value and writing back what the user chose.
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

    /// Replace this app's debug-overlay flags. The overlay is app-global:
    /// the write is visible to every window at once, and the host repaints
    /// idle windows so it shows everywhere — not just the window that
    /// handled the key.
    #[inline]
    pub fn set_debug_overlay(&mut self, overlay: DebugOverlayConfig) {
        self.resources.diagnostics().overlay.set(overlay);
    }

    /// This frame's diagnostic counters, for the [`frame_stats`] overlay.
    ///
    /// The overlay is an ordinary widget: it reaches `Layer::Debug` through
    /// the same `&mut Ui` every other widget takes, and so has no path of its
    /// own to the frame clock or the GPU handle these come off.
    ///
    /// [`frame_stats`]: crate::diagnostics::frame_stats
    pub(crate) fn frame_stats(&self) -> FrameStats {
        FrameStats {
            frame_id: self.frame_runtime.frame_id,
            render_frame_id: self.frame_runtime.render_frame_id,
            fps: self.frame_runtime.fps_ema,
            settle_frames: self.frame_runtime.settle_frames,
            gpu_ms: self.resources.diagnostics().gpu_pass_stats.last_pass_ms(),
        }
    }

    /// Whether a window addressed by `token` is currently live. Reflects
    /// the set as of this frame's *start*, so a window opened or closed
    /// earlier *this* frame isn't reflected until the next one (the host
    /// drains [`Self::open_window`] / [`Self::close_window`] between
    /// frames). Use it as the source of truth for "is this window up?"
    /// instead of mirroring the state in app code — a window the user
    /// closed via its titlebar drops out of this set automatically.
    #[inline]
    pub fn window_open(&self, token: WindowToken) -> bool {
        self.resources.windows().contains(token)
    }

    /// The app-global live-window set this recorder answers
    /// [`Self::window_open`] from.
    ///
    /// For `WindowDriver`'s `Drop`, which retires its own token: the
    /// driver owns this `Ui` and the directory is the only thing it needs
    /// off the shared resources, so it asks by name rather than reaching
    /// through the field.
    #[inline]
    pub(crate) fn window_directory(&self) -> &WindowDirectory {
        self.resources.windows()
    }

    /// Attach a paint primitive to the active node. Direct text contributes to
    /// layout only on a leaf; container-owned text is an overlay shaped against
    /// that container's final padded width.
    pub fn add_shape<S: Lower>(&mut self, shape: S) {
        self.forest.add_shape(shape);
    }

    /// Load an icon set and get back an owning [`IconSet`] to draw from.
    ///
    /// **Hold the set.** It owns everything the host caches for those icons —
    /// the data, the SVG parses, the atlas rasters — and dropping the last
    /// clone unloads all three at the next submit. Park it in your state and
    /// clone it where it needs to live.
    ///
    /// Loading is cheap and idempotent *while a set is held*: registering an
    /// `Rc` a live `IconSet` already covers hands back a clone of that set
    /// rather than a second entry, so an immediate-mode caller may re-load on
    /// every frame if that reads better than threading the handle through —
    /// as long as what comes back outlives the frame. No parsing and no GPU
    /// work happen here.
    ///
    /// Each icon's SVG is parsed the first time that icon is rasterized, and
    /// each raster happens at the exact physical pixel size the icon is drawn
    /// at — so a set the session never draws from costs nothing beyond its
    /// bytes.
    #[inline]
    pub fn load_icons(&self, table: Rc<IconTable>) -> IconSet {
        self.resources.icons().register(table)
    }

    /// Upload an image now and get back an owning [`ImageHandle`]. **Hold
    /// the handle** to keep the GPU texture resident — dropping the last
    /// clone frees it; there is no `unregister`. Reference it in
    /// [`Shape::image`](crate::Shape::image) every frame (`clone` it where it
    /// needs to live).
    ///
    /// The bytes are copied into wgpu staging before this returns; the next
    /// queue submission uploads them before drawing. No CPU copy is retained
    /// by the registry. Keep `image` to refill it for [`ImageHandle::update`].
    ///
    /// # Errors
    ///
    /// Returns an error when an image axis exceeds the selected device's 2D
    /// texture limit. A rejected image never reaches the GPU. Standalone
    /// CPU recorders have no device limit and retain the original dimensions.
    #[inline]
    pub fn register_image(&self, image: &Image) -> Result<ImageHandle, RegisterImageError> {
        self.resources.register_image(image)
    }

    /// Register a font and get back the [`FontFamily`] its first face
    /// belongs to.
    ///
    /// Additive and permanent: there is no unload, because the glyph
    /// atlas keys on the database's face id and fontdb never reuses one.
    /// A collection registers every face it holds, and each of their
    /// families becomes reachable through
    /// [`FontFamily::named`](crate::FontFamily::named). The returned
    /// family is `Copy` and always valid, so — unlike
    /// [`Self::load_icons`] and [`Self::register_image`] — nothing has to
    /// be held to keep it alive.
    ///
    /// A load invalidates every shaped buffer, every reuse row, the
    /// layout measure snapshot and every encoded glyph template — and
    /// **this frame** re-shapes the text on screen, not the next one.
    ///
    /// So it schedules nothing, and needs no `&mut self` to do it. An app
    /// holds a `Ui` only inside [`App::update`](crate::App::update) and
    /// [`App::record`](crate::App::record), both of which run on a
    /// recorded frame *before* that frame measures and before it submits
    /// — and measuring and submitting are the two steps that re-read the
    /// font database. Loads are cold events; one remeasured frame is the
    /// whole cost.
    ///
    /// ```ignore
    /// let mono = ui.load_font(include_bytes!("../assets/Iosevka.ttf"))?;
    /// let sans = ui.load_font("/usr/share/fonts/TTF/Charter.ttf")?;
    /// ```
    ///
    /// A `&str` is a **path**, never a family name — naming an installed
    /// family is [`FontFamily::named`](crate::FontFamily::named), which
    /// needs no load at all when the host asked for
    /// [`FontScope::System`](crate::FontScope::System).
    ///
    /// # Errors
    ///
    /// [`FontLoadError::Io`](crate::FontLoadError::Io) when the file
    /// cannot be read, and
    /// [`FontLoadError::NoFaces`](crate::FontLoadError::NoFaces) when the
    /// bytes hold no face fontdb can parse.
    #[inline]
    pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError> {
        self.resources.text().load_font(source)
    }

    /// Whether a face answers to `family`.
    ///
    /// A family with no face resolves to
    /// [`FontFamily::SANS`](crate::FontFamily::SANS) at shaping time and
    /// warns once — never to whatever the machine happens to have
    /// installed. Ask here to choose deterministically instead.
    #[inline]
    pub fn font_available(&self, family: FontFamily) -> bool {
        self.resources.text().font_available(family)
    }

    /// Every family the shaper's database knows, system fonts included —
    /// what a preferences picker lists.
    ///
    /// Cold: it walks every face and interns every name it has not seen.
    #[inline]
    pub fn font_families(&self) -> Vec<FontFamily> {
        self.resources.text().font_families()
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
    #[inline]
    pub fn max_image_dimension(&self) -> Option<NonZeroU32> {
        self.resources.texture_limit().max_dimension()
    }

    /// A handle on the host's clipboard.
    ///
    /// One clipboard per host, shared by every window that host opens and
    /// isolated from every other host's.
    ///
    /// Hands back a clone rather than a borrow because that is what the
    /// caller shape needs: a widget reads the clipboard from inside a
    /// keyboard-event walk that already holds `&mut Ui`, so a borrow out of
    /// `self` could not survive to the paste. The handle is an `Rc` inside,
    /// so the clone is a refcount bump — take one before the walk and pass
    /// it to whatever handles the action.
    ///
    /// `&self` on [`Self::load_font`]'s terms: the clipboard is a
    /// host-global resource and touching it schedules no frame work.
    ///
    /// # What is behind it
    ///
    /// The OS clipboard, plus an in-process fallback that takes over when
    /// the system backend refuses a write. A copy is therefore never
    /// silently lost, and a paste after such a refusal returns the last
    /// text the user actually copied rather than stale system content.
    ///
    /// [`WinitHost`](crate::WinitHost) always reaches for the OS
    /// clipboard. [`OffscreenHost`](crate::OffscreenHost) does not until
    /// asked, through
    /// [`OffscreenHostBuilder::system_clipboard`](crate::OffscreenHostBuilder::system_clipboard),
    /// because a thumbnailer should not touch the desktop's clipboard on
    /// its caller's behalf.
    ///
    /// Without the `system-clipboard` feature the in-process buffer is
    /// all there is. [`Clipboard::set_text`] then [`Clipboard::text`]
    /// round-trips, and nothing reaches the OS.
    ///
    /// On Linux the copied text is served by the running process, so it
    /// goes when the app exits unless a desktop clipboard manager took a
    /// copy of its own. That is how X11 and Wayland selections work.
    #[inline]
    pub fn clipboard(&self) -> Clipboard {
        self.resources.clipboard().clone()
    }

    /// Record a `GpuView` for widget `id`: refresh its row in
    /// [`Self::gpu_views`], then append a
    /// [`ShapeRecord::Image`](crate::scene::shapes::record::ShapeRecord::Image)
    /// sourced from an
    /// [`ImageSource::GpuView`](crate::scene::shapes::paint::ImageSource::GpuView)
    /// carrying the row's `epoch` to the active node — the encoder
    /// recovers id and paint from the store by `id`.
    ///
    /// The mint / epoch / sweep policy is [`GpuViews`]' business,
    /// `repaint` included.
    pub(crate) fn gpu_view(&mut self, id: WidgetId, paint: GpuPaintRef, repaint: bool) {
        let frame = self.frame_runtime.render_frame_id;
        let epoch = self.gpu_views.record(id, paint, repaint, frame);
        self.forest.add_gpu_view(epoch);
    }

    /// Format `args` directly into the record-pass text storage and return
    /// an arena-backed [`InternedStr`]. Pass the returned value to
    /// any text-taking widget. The bytes are already in the destination
    /// buffer, so same-arena lowering is zero-copy and steady-state authoring
    /// of dynamic labels skips per-call `String` allocations.
    ///
    /// Call sites normally reach for [`fmt!`](crate::fmt), which is this over
    /// `format_args!` — `fmt!(ui, "clicks: {n}")`.
    ///
    /// **Valid only for the pass that minted it.** Lower it here, in this
    /// window; holding it into a later frame, into the second pass of a
    /// double-layout frame, or into another window panics — the bytes it
    /// spans are gone by then. Persistent application text should stay in
    /// its source `String` and be interned again each frame, which costs
    /// the one `memcpy` the borrowed path pays anyway.
    #[must_use]
    #[inline]
    pub fn fmt(&mut self, args: std::fmt::Arguments<'_>) -> InternedStr {
        self.forest.record_store.intern_fmt(args)
    }

    /// Normalize borrowed, owned, or already-interned text into an
    /// [`InternedStr`]. Borrowed and owned inputs are copied into the
    /// record-pass text arena; an [`InternedStr`] this pass minted passes
    /// through unchanged, and one from an earlier pass or another window
    /// panics here rather than resolving against bytes that are gone.
    /// Format-less twin of [`Self::fmt`] with the same retention rules.
    #[must_use]
    pub fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr {
        self.forest.record_store.intern(text.into())
    }

    /// Append `shape` to the active node and animate it at **paint**
    /// time.
    ///
    /// The recorded shape is byte-identical every frame — the encoder
    /// samples `anim` one pass later and folds the result into the
    /// brush — so the widget never re-records and its layout cache entry
    /// survives. [`Self::animate`] plus [`Self::request_repaint`] paint
    /// the same pixels at the cost of a record pass per frame.
    ///
    /// `post_record` folds the animation's next wake into the repaint
    /// queue, so a caller schedules nothing of its own.
    ///
    /// Drops silently if the shape itself was noop-collapsed — zero
    /// stroke and transparent fill, say. An animation cannot make a zero
    /// shape paintable.
    ///
    /// ```
    /// # use palantir::{PaintAnim, PaintRepeat, Rect, RgbaF32, Shape, Ui, curves};
    /// # use std::time::Duration;
    /// # fn demo(ui: &mut Ui) {
    /// ui.add_shape_animated(
    ///     Shape::rect(Rect::new(0.0, 0.0, 8.0, 8.0)).fill(RgbaF32::WHITE),
    ///     PaintAnim::alpha(0.4, 1.0)
    ///         .period(Duration::from_secs(2))
    ///         .repeat(PaintRepeat::Forever)
    ///         .curve(curves::sine),
    /// );
    /// # }
    /// ```
    pub fn add_shape_animated<S: Lower>(&mut self, shape: S, anim: PaintAnim) {
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
    /// modal).
    ///
    /// # Panics
    ///
    /// Panics unless a nested layer sits strictly above the current scope
    /// in `Layer::PAINT_ORDER`. A lower-or-equal nest records fine and
    /// then paints under the parent it was raised from, un-hittable, so
    /// it is rejected where it is asked for rather than left to show up
    /// as a scope that quietly stopped working.
    #[inline]
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
    #[inline]
    pub fn close_scope(&mut self, id: WidgetId) {
        self.input.close_scope(id);
    }

    /// Resolve a widget's identity recipe against the currently-open
    /// parent into the id it records under. [`Widget::resolve`] is the
    /// one caller, and it keeps the answer on the widget.
    ///
    /// [`Widget::resolve`]: crate::Widget::resolve
    #[inline]
    pub(crate) fn resolve_ident(&mut self, ident: Ident) -> WidgetId {
        self.forest.widget_id(ident)
    }

    /// Open `node` under `id`, painting `chrome` behind it. Pairs with
    /// [`Self::close_node`].
    ///
    /// Two callers, and no third: [`Widget::record`], which is how every
    /// widget in the crate reaches the tree, and `FrameCycle`'s synthetic
    /// `Layer::Main` viewport, which has no `Widget` to record through.
    /// Widget code calls `Widget::record`, never this.
    ///
    /// [`Widget::record`]: crate::Widget::record
    #[inline]
    pub(crate) fn open_node(&mut self, id: WidgetId, node: &Node, chrome: Option<&Background>) {
        self.forest.open_node(id, node, chrome);
    }

    #[inline]
    pub(crate) fn close_node(&mut self) {
        self.forest.close_node();
    }

    /// Intern a grid's track definition into the current layer and hand back
    /// the id a `Node` carries it by. Called by [`Widget::grid_tracks`],
    /// which is the whole reason it is on `Ui`: a widget that becomes a
    /// grid references a definition the tree owns, and should not have to
    /// name the tree to do it.
    ///
    /// [`Widget::grid_tracks`]: crate::Widget::grid_tracks
    #[inline]
    pub(crate) fn push_grid_def(&mut self, rows: &[Track], cols: &[Track]) -> GridDefId {
        self.forest.push_grid_def(rows, cols)
    }

    #[inline]
    pub(crate) fn push_scrollbars_def(&mut self, def: ScrollbarsDef) -> ScrollbarsDefId {
        self.forest.push_scrollbars_def(def)
    }

    /// The node `id` was recorded as **this pass**, for a caller that has
    /// just opened it and needs the handle a downstream driver keys off.
    /// Panics if `id` has not been recorded yet this pass.
    #[inline]
    pub(crate) fn current_node(&self, id: WidgetId) -> NodeId {
        self.forest.current_node(id)
    }

    /// Last frame's measured content extent for the scroll viewport `id`,
    /// `Size::ZERO` for any widget that is not one or has not yet arranged.
    ///
    /// **A bridge, and that is why it lives here.** The extent is keyed by
    /// `(layer, node)` in `Layout` while the caller holds a `WidgetId`, and
    /// `Cascade` is what maps between them — so answering the question needs
    /// both tables, and `Ui` is the only thing holding both. Cascade timing
    /// applies: like [`Self::response_for`] this answers for the previous
    /// frame, which is the lag `Scroll` wants — the bars describe the content
    /// the user is looking at.
    #[inline]
    pub(crate) fn scroll_content(&self, id: WidgetId) -> Size {
        self.cascade
            .endpoint(id)
            .map_or(Size::ZERO, |endpoint| self.layout.scroll_content(endpoint))
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
    /// The widget's own `Node::disabled` is **not** folded in here — only
    /// `Widget::response` can see it. Both fold through
    /// `ResponseState::merge_disabled`, which is idempotent, so the
    /// interaction half is gone by the time either of them returns.
    pub fn response_for(&self, id: WidgetId) -> ResponseState {
        let mut state = self.input.response_for(id, &self.cascade, &self.layout);
        // Cascade lags one frame; fold this frame's ancestor-disabled so
        // a freshly-disabled subtree paints disabled on its first frame.
        state.merge_disabled(self.forest.current_scratch().ancestor_disabled());
        state
    }

    /// Cross-frame state row for `id`, `T::default()` on first
    /// access. Rows for `WidgetId`s not recorded this frame are
    /// evicted in `finalize_frame`, once per `Ui::frame` after the
    /// final record pass. Type collisions at one `id` are NOT
    /// detected — each `T` lives in its own store, so two call sites
    /// using different types at the same id silently coexist (see the
    /// `state` module doc).
    ///
    /// The returned borrow is out of `&mut Ui`, so it ends at the next
    /// widget call — fine for a single read or write, useless for state a
    /// whole subtree edits. Use [`Self::with_state`] for that.
    pub fn state_mut<S: Default + 'static>(&mut self, id: WidgetId) -> &mut S {
        self.state.get_or_insert_with(id, S::default)
    }

    /// Lend the cross-frame state row for `id` to `body`, **alongside** the
    /// `Ui` — the scope in which a page, a panel, or any other subtree
    /// larger than one widget owns state.
    ///
    /// [`Self::state_mut`] hands back a borrow of the `Ui`, which the first
    /// widget call inside the scope invalidates; the row is instead moved
    /// out for the duration of the call and moved back after, so both are
    /// live at once:
    ///
    /// ```
    /// # use palantir::{Button, Configure, Text, Ui, WidgetId};
    /// # #[derive(Default)]
    /// # struct Page { clicks: u32, note: String }
    /// # fn demo(ui: &mut Ui, page_id: WidgetId) {
    /// ui.with_state::<Page, _>(page_id, |ui, page| {
    ///     if Button::new().label("click").show(ui).left.clicked() {
    ///         page.clicks += 1;
    ///     }
    ///     Text::new(&page.note).show(ui);
    /// });
    /// # }
    /// ```
    ///
    /// The row is `S::default()` on first access and follows the same
    /// eviction rule as [`Self::state_mut`], so a subtree that stops being
    /// recorded drops its state — key it off a [`WidgetId`] that lives as
    /// long as the state should.
    ///
    /// Re-entering the same `(id, S)` from within `body` is a caller bug:
    /// the inner scope sees a default row and its writes are overwritten
    /// when the outer one restores. Nesting *different* rows is fine, which
    /// is what makes this compose down a tree.
    pub fn with_state<S: Default + 'static, R>(
        &mut self,
        id: WidgetId,
        body: impl FnOnce(&mut Self, &mut S) -> R,
    ) -> R {
        let mut value = std::mem::take(self.state_mut::<S>(id));
        let out = body(self, &mut value);
        // Re-probed rather than held: `body` may have inserted rows of the
        // same `S` at other ids, which can reallocate the store's data vec.
        *self.state_mut::<S>(id) = value;
        out
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
        let r = self.anim.animate(
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
    #[inline]
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
    #[inline]
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
    #[inline]
    pub fn hover_within(&self, ancestor: WidgetId) -> bool {
        self.input
            .hovered
            .is_some_and(|h| self.cascade.is_within(h, ancestor))
    }

    /// Active `Display` (physical surface size + both scale factors). Read
    /// by example/demo code that wants to inject synthetic input
    /// coordinates without threading window dimensions through itself.
    #[inline]
    pub fn display(&self) -> Display {
        self.display
    }

    /// The application's own scale, multiplied onto the scale the platform
    /// reported. `UserScale::ONE` until something sets it.
    ///
    /// The source of truth, rather than something app code mirrors — the
    /// same position [`Self::vsync`] takes. Reads back what
    /// [`Self::set_user_scale`] last wrote, from the first frame, in every
    /// window.
    #[inline]
    pub fn user_scale(&self) -> UserScale {
        self.resources.user_scale().get()
    }

    /// Scale the whole UI by `scale` on top of the platform's own factor.
    ///
    /// **App-global**, like [`Self::set_debug_overlay`] and for the same
    /// reason: the per-monitor case is already answered by the platform
    /// factor, so what is left is a preference, and the host repaints idle
    /// windows so a change from one shows everywhere.
    ///
    /// Takes effect on the next frame, because the host mints each frame's
    /// [`Display`] before running it. This frame's [`Self::display`] still
    /// reports the old value, and the frame after relayouts and repaints in
    /// full — the same path a monitor move takes.
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
    /// Writing the value already in force costs nothing, so a control may
    /// write its own state back every frame.
    ///
    /// **Restore a persisted preference from the app factory** — the
    /// closure [`WinitHostBuilder::build`](crate::WinitHostBuilder::build)
    /// hands a `&mut Ui`, and it runs before the first frame, so the first
    /// window opens at the right size. That is the same place a
    /// [`Theme`](crate::Theme) is restored, and there is no host setting
    /// for either.
    ///
    /// Each distinct scale re-rasterizes every glyph on screen — see
    /// [`UserScale`], which is also where the ladder the steppers walk
    /// lives.
    #[inline]
    pub fn set_user_scale(&mut self, scale: UserScale) {
        self.resources.user_scale().set(scale);
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
    #[inline]
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
    #[inline]
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
    #[inline]
    pub fn probe_text<'a>(&'a mut self, run: TextRun<'a>) -> TextProbe<'a> {
        self.resources.text().layout(&run)
    }

    /// Every edge the pointer produced this frame, widget by widget.
    ///
    /// **The collation half of the input API**, against
    /// [`Self::response_for`]'s polling half. Poll when a widget wants its own
    /// state — that is what every widget in this crate does, and what a
    /// `Response` is for. Reach for this when the *application* wants to know
    /// what the pointer did without naming everything it could have done it to:
    /// a list whose ids come from data rather than from call sites, a router
    /// that dispatches by what was hit, a log.
    ///
    /// **Edges, not levels.** A drag's travel is a level, and this reports only
    /// that a drag started and on what — take the id and poll that one widget
    /// for as long as the gesture lasts. See
    /// [`PointerEdge`](crate::PointerEdge).
    ///
    /// Read it during the frame's record, for the reason
    /// [`Self::response_for`] gives: the edges are one-frame state cleared
    /// between frames, and routed against a snapshot taken when the pass opened.
    /// Order is by button, then press before release; two widgets touched by
    /// two buttons in one frame both appear.
    #[inline]
    pub fn pointer_actions(&self) -> impl Iterator<Item = PointerAction> + '_ {
        self.input.pointer_actions()
    }

    /// Programmatically set or clear focus. Bypasses [`FocusPolicy`].
    ///
    /// `focused` reads back immediately, but key-class routing does not
    /// move until the next record pass: this pass's keystrokes were
    /// already routed by the scope path resolved at its start. A widget
    /// that blurs itself on Escape therefore does not also hand that
    /// Escape to the overlay around it.
    #[inline]
    pub fn request_focus(&mut self, id: Option<WidgetId>) {
        self.input.set_focus(id);
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
    #[inline]
    pub fn pointer_pos(&mut self) -> Option<Vec2> {
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
    #[inline]
    pub fn pointer_local(&mut self, id: WidgetId) -> Option<Vec2> {
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
    #[inline]
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
    #[inline]
    pub fn peek_pointer_pos(&self) -> Option<Vec2> {
        self.input.pointer_pos
    }

    /// [`Self::pointer_local`] without the [`PointerWake::MOVE`] watch.
    /// Same caveat as [`Self::peek_pointer_pos`].
    #[inline]
    pub fn peek_pointer_local(&self, id: WidgetId) -> Option<Vec2> {
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
    #[inline]
    pub fn peek_modifiers(&self) -> Modifiers {
        self.input.modifiers
    }

    #[inline]
    pub fn focus_policy(&self) -> FocusPolicy {
        self.input.focus_policy
    }

    /// Set the press-on-non-focusable behavior. See [`FocusPolicy`].
    #[inline]
    pub fn set_focus_policy(&mut self, p: FocusPolicy) {
        self.input.focus_policy = p;
    }

    /// Which "did input arrive?" signal the frame gate consults before
    /// it commits to a full record pass. See [`InputPolicy`].
    #[inline]
    pub fn input_policy(&self) -> InputPolicy {
        self.input.input_policy
    }

    /// Set the record gate's input signal. Default
    /// [`InputPolicy::OnDelta`] skips record on inert pointer moves and
    /// scroll-over-nothing; [`InputPolicy::Always`] is for telemetry /
    /// custom canvases that need every event.
    #[inline]
    pub fn set_input_policy(&mut self, p: InputPolicy) {
        self.input.input_policy = p;
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod harness;

/// The doors past [`Ui`]'s private fields, and the only ones.
///
/// Every field is private so that production code outside this module reaches
/// the state through a named method — see the type's own doc for why. The
/// white-box suites need more than that surface: they assert on tree
/// contents, measure-cache descriptors, cascade rows and routing state that
/// no widget has any business reading. They reach it here, in a module that
/// does not exist in a shipped build.
///
/// Two gates, and the narrower one says something the wider cannot — that the
/// benches, which compile under `internals` without `cfg(test)`, do not use
/// what it holds.
///
/// None of these carry `#[inline]`, unlike the one-line façade above. Nothing
/// here reaches an optimized build that would want it: `cfg(test)` compiles
/// under `dev` at `opt-level = 0`, where the attribute changes nothing, and
/// the bench profile is fat-LTO with one codegen unit, which inlines across
/// the crate regardless.
#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    #[cfg(test)]
    use crate::input::input_state::InputState;
    #[cfg(test)]
    use crate::layout::LayerLayout;
    #[cfg(any(test, feature = "bench"))]
    use crate::layout::Layout;
    #[cfg(test)]
    use crate::primitives::rect::Rect;
    #[cfg(test)]
    use crate::scene::cascade::Cascade;
    #[cfg(test)]
    use crate::scene::endpoint::Endpoint;
    #[cfg(any(test, feature = "bench"))]
    use crate::scene::forest::Forest;
    #[cfg(test)]
    use crate::scene::layer::Layer;
    #[cfg(test)]
    use crate::scene::tree::Tree;
    #[cfg(test)]
    use crate::scene::tree::node_id::NodeId;
    #[cfg(test)]
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    #[cfg(test)]
    use crate::ui::frame_runtime::FrameRuntime;
    use crate::widgets::theme::Theme;
    #[cfg(all(test, feature = "winit"))]
    use crate::window::window_frame_state::WindowFrameState;
    #[cfg(test)]
    use crate::window::window_requests::WindowRequests;
    use std::rc::Rc;

    impl Ui {
        /// The active theme, for in-place edits
        /// (`ui.theme_mut().button.anim = …`).
        ///
        /// Gated, because in-place mutation is a fixture affordance
        /// rather than how an app dresses a `Ui`: build the [`Theme`]
        /// you want and hand it over with
        /// [`set_theme`](Ui::set_theme) — which is what every caller in
        /// this workspace does, and what the `winit` module's example
        /// shows. This exists so a test can nudge one axis of an
        /// already-running harness without rebuilding the bundle.
        ///
        /// Copy-on-write, so a handle taken from [`Ui::theme`] keeps the
        /// values it was taken with and the `Ui` moves on alone.
        #[inline]
        pub fn theme_mut(&mut self) -> &mut Theme {
            Rc::make_mut(&mut self.theme)
        }
    }

    /// The two authored tables the benches read as well as the tests: the
    /// tree walkers and measure-cache cases take `Self::forest`, and the
    /// cascade bench runs its engine over both.
    ///
    /// `bench` rather than the mod's own `internals`, which is wider than
    /// either consumer: a build that reaches past the published surface
    /// without compiling the bench drivers has no caller for these, and
    /// `-W dead_code` says so.
    #[cfg(any(test, feature = "bench"))]
    impl Ui {
        /// The whole forest, for the callers that re-run a pass over it —
        /// the cascade engine and the measure cache both walk every layer.
        pub(crate) fn forest(&self) -> &Forest {
            &self.forest
        }

        /// The whole layout table, for the handful of callers that re-run a
        /// pass over it — the cascade engine and `InputState::response_for`
        /// both walk every layer, so neither can take one layer's columns.
        pub(crate) fn layout_tables(&self) -> &Layout {
            &self.layout
        }
    }

    /// Narrower than the mod's own gate: these are `pub(crate)` and only
    /// this crate's own tests call them, so under `internals` alone they
    /// would be dead code.
    #[cfg(test)]
    impl Ui {
        /// The input machine itself, for tests that assert on routing
        /// state the public surface deliberately does not expose —
        /// capture targets, the raw per-layer streams, the action flag.
        pub(crate) fn input(&self) -> &InputState {
            &self.input
        }

        /// [`Self::input`], mutably — for tests that *drive* routing
        /// state rather than assert on it (planting focus, taking the
        /// action flag).
        pub(crate) fn input_mut(&mut self) -> &mut InputState {
            &mut self.input
        }

        pub(crate) fn cascade(&self) -> &Cascade {
            &self.cascade
        }

        /// One layer's recorded tree — its `records` columns, `rollups`,
        /// `shapes`, `paint_anims` and `roots`.
        ///
        /// The [`Self::layout`] of the scene side, and for the same reason:
        /// indexing `forest().trees[layer]` was what all but a handful of
        /// assertions did with a [`Forest`].
        pub(crate) fn tree(&self, layer: Layer) -> &Tree {
            &self.forest.trees[layer]
        }

        /// One layer's arranged columns — `text_shapes`, `text_spans`,
        /// `rect_hash`, and the `rect` column [`Self::arranged_rect`] reads
        /// one cell of.
        ///
        /// Takes the layer rather than handing the whole table back for a
        /// caller to index, because one layer's columns is what all but a
        /// handful of assertions want, and a call reads better there than a
        /// call followed by an index. The six callers that want the table
        /// itself take [`Self::layout_tables`].
        pub(crate) fn layout(&self, layer: Layer) -> &LayerLayout {
            &self.layout[layer]
        }

        /// `node`'s arranged rect on `layer` — pre-transform, unclipped,
        /// world coords.
        ///
        /// The commonest thing a layout assertion asks, and it routes through
        /// [`Layout::arranged_rect`] so a test reads the rect the same way
        /// `ResponseState::layout_rect` does. Takes the pair rather than an
        /// [`Endpoint`] because a test holding a `NodeId` off a record
        /// closure has no endpoint to hand over.
        pub(crate) fn arranged_rect(&self, layer: Layer, node: NodeId) -> Rect {
            self.layout.arranged_rect(Endpoint { layer, node })
        }

        /// The shaper the recorder measures and paints text with — the one
        /// piece of [`UiResources`](crate::ui::resources::UiResources) any
        /// test asks for, and all it gets: the cache-population and
        /// measure-count probes the text suites assert on.
        pub(crate) fn shaper(&self) -> &TextShaper {
            self.resources.text()
        }

        pub(crate) fn frame_runtime(&self) -> &FrameRuntime {
            &self.frame_runtime
        }

        /// Narrower again: only the winit host's own tests write here.
        #[cfg(feature = "winit")]
        pub(crate) fn frame_runtime_mut(&mut self) -> &mut FrameRuntime {
            &mut self.frame_runtime
        }

        pub(crate) fn window_requests(&self) -> &WindowRequests {
            &self.window_requests
        }

        /// Narrower again: only the winit host's own tests write here.
        #[cfg(feature = "winit")]
        pub(crate) fn window_frame_mut(&mut self) -> &mut WindowFrameState {
            &mut self.window_frame
        }
    }
}

#[cfg(test)]
mod tests;
