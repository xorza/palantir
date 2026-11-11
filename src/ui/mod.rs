//! The recorder: [`Ui`], the handle a widget authors a frame through, and the retained state that
//! frame reads and writes. `window_frames` holds it, runs the frame and takes it to its target.
//!
//! It records through `Node` and shapes, never through a widget, so the recorder sits below the
//! authoring layer.

pub(crate) mod added_shape;
pub(crate) mod app;
#[cfg(feature = "internals")]
pub(crate) mod bench;
pub(crate) mod frame_report;
pub(crate) mod frame_runtime;
mod frame_stats_readout;
mod host_seam;
pub(crate) mod inspected_widget;
pub(crate) mod layer_scope;
mod memory;
pub(crate) mod overlay_stack;
pub(crate) mod resources;
pub(crate) mod state;
mod theme_pair;
mod theme_transition;
pub(crate) mod ui_preferences;
pub(crate) mod window_frames;
mod window_theme;

use crate::animation::animated::Animated;
use crate::animation::animation_map_typed::AnimationClock;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::motion_role::MotionRole;
use crate::cascade::Cascade;
use crate::cascade::WidgetLocation;
use crate::cascade::rows::tab_stop_row::TabEnd;
use crate::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::diagnostics::frame_stats::FrameStats;
use crate::diagnostics::gpu_pass_stats::GpuPassStats;
use crate::display::Display;
use crate::display::user_scale::UserScale;
use crate::effect::effect_source::EffectSource;
use crate::effect::error::EffectError;
use crate::effect::press_input::PressInput;
use crate::gpu::gpu_paint::gpu_views::RecordedView;
use crate::icons::icon_set::IconSet;
use crate::icons::icon_table::IconTable;
use crate::image_registry::error::ImageTooLarge;
use crate::image_registry::image_handle::ImageHandle;
use crate::input::Input;
use crate::input::key_reader::KeyReader;
use crate::primitives::effect::effect_kind::{
    BackdropEffect, ColorEffect, EffectKind, EffectTypes,
};
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::effect::effect_params::EffectParams;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::ime_preedit::ImePreedit;
use crate::primitives::input::interaction::pointer_action::PointerAction;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::key_class::KeyClass;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::keyboard::key_press::KeyPress;
use crate::primitives::input::keyboard::modifiers::Modifiers;
use crate::primitives::input::pointer::PointerEvent;
use crate::primitives::input::policy::FocusPolicy;
use crate::primitives::input::policy::InputPolicy;
use crate::primitives::input::shortcut::Shortcut;
use crate::primitives::input::watch::{KeyboardWake, PointerWake};
use crate::primitives::layout::layout_mode::{GridDefId, ScrollbarsDefId};
use crate::primitives::layout::track::Track;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::math::domain;
use crate::primitives::math::domain::vec2;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::image::Image;
use crate::primitives::paint::material::material_input::MaterialInput;
use crate::primitives::text::interned_str::InternedStr;
use crate::primitives::text::text_input::TextInput;
use crate::renderer::frontend::FrameScene;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::id_recipe::IdRecipe;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::scene::seen_ids::ResolvedId;
use crate::scene::tree::NodeChrome;
use crate::shape::PaintedBy;
use crate::shape::paint::lowered_material::MaterialRead;
use crate::shape::record_store::Resolve;
use crate::text::error::FontLoadError;
use crate::text::font_family::FontFamily;
use crate::text::font_source::FontSource;
use crate::text::probe::TextProbe;
use crate::text::run::TextRun;
use crate::theme::Theme;
use crate::theme::color_scheme::ColorScheme;
use crate::theme::error::ThemeEffectError;
use crate::ui::added_shape::AddedShape;
use crate::ui::app::RecordPass;
use crate::ui::frame_runtime::FrameRuntime;
use crate::ui::frame_runtime::wake::WakeReasons;
use crate::ui::host_seam::HostSeam;
use crate::ui::inspected_widget::InspectedWidget;
use crate::ui::layer_scope::LayerScope;
use crate::ui::memory::Memory;
use crate::ui::overlay_stack::OverlayStack;
use crate::ui::resources::UiResources;
use crate::ui::ui_preferences::UiPreferences;
use crate::ui::window_theme::{Motion, WindowTheme};
use crate::window::cursor_icon::CursorIcon;
use crate::window::screenshot::{Screenshot, ScreenshotId};
use crate::window::vsync::Vsync;
use crate::window::waker::Waker;
use crate::window::window_config::WindowConfig;
use crate::window::window_geometry::WindowGeometry;
use crate::window::window_token::WindowToken;
use glam::Vec2;
use std::fmt;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Duration;

/// Recorder + input/response broker. Coordinates are logical pixels;
/// `Display::scale_factor` converts to physical at the wgpu boundary. Fields are
/// private so widgets share one method surface.
///
/// It repeats the reads of the crate's `Input`, each sugar over the one there, as call sites read
/// better. Cost: a new read is written twice. Refused: a public `Ui::input()`, as egui's
/// `ui.input(|i| …)` reads, which makes each call longer; a trait for the reads, which each caller
/// imports and which takes `const fn` off them.
#[derive(Debug)]
pub struct Ui {
    forest: Forest,
    /// This window's theme for each scheme, the scheme it shows and the transition between them.
    window_theme: WindowTheme,
    /// What the widgets keep between frames.
    memory: Memory,
    resources: UiResources,
    /// This window's input, and the arranged frame it reads.
    input: Input,
    /// Frame clock, wake queue, repaint/relayout flags and prior-frame validity.
    frame_runtime: FrameRuntime,
    /// What this `Ui` and its host hand each other.
    host: HostSeam,
    /// The overlay bodies recording now, and the close a row in each asked for.
    overlays: OverlayStack,
}

/// Widget-facing authoring API; mostly one-line delegations to sealed subsystems. The host's side
/// is on `WindowFrames`.
impl Ui {
    /// Construct a per-window `Ui`; each owns its own [`Forest`].
    fn new(resources: UiResources) -> Self {
        Self {
            forest: Forest::new(resources.text().family_keys()),
            window_theme: WindowTheme::new(resources.color_scheme()),
            resources,
            memory: Memory::default(),
            input: Input::new(),
            frame_runtime: FrameRuntime::default(),
            host: HostSeam::default(),
            overlays: OverlayStack::default(),
        }
    }

    /// Which pass of its frame this record is. Read it before work that must not replay.
    #[inline]
    pub const fn record_pass(&self) -> RecordPass {
        self.frame_runtime.record_pass
    }

    /// What the frontend builds the last frame's draw list from.
    pub(crate) const fn frame_scene(&self) -> FrameScene<'_> {
        FrameScene {
            forest: &self.forest,
            layout: &self.input.arranged.layout,
            cascade: &self.input.arranged.cascade,
            stamp: self.frame_runtime.stamp(),
        }
    }

    /// The last frame's cascade, which a driver asks what an input reaches.
    pub(crate) const fn cascade(&self) -> &Cascade {
        &self.input.arranged.cascade
    }

    /// The input machine, which a harness asks where the pointer and its widgets are.
    pub(crate) const fn input(&self) -> &Input {
        &self.input
    }

    /// A handle that asks this window for a frame from any thread: see [`Waker`].
    pub fn waker(&self) -> Waker {
        self.host.waker.clone()
    }

    /// The theme this frame shows: the one of the scheme it shows ([`Self::color_scheme`]), or a
    /// blend on its way there under [`Theme::transition`]. `.clone()` is a refcount
    /// bump, so a style can outlive the `show(ui)` reborrow; a handle kept past the frame makes
    /// the next blend copy the theme:
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
        self.window_theme.shown()
    }

    /// This window's theme for `scheme`, shown or not. [`Theme::stock`] of each until set.
    #[inline]
    pub const fn scheme_theme(&self, scheme: ColorScheme) -> &Rc<Theme> {
        self.window_theme.get(scheme)
    }

    /// Replace this window's theme for `scheme`; takes an `Rc` so swapping prebuilt themes copies
    /// nothing, and a theme equal to the one the slot holds changes nothing, the same handle with
    /// no compare. The other scheme's waits until its scheme shows.
    ///
    /// The shown scheme's moves there from the next frame, this one keeping what it shows, under
    /// [`Theme::transition`], as Flutter's `AnimatedTheme` shows each frame's lerp: colors, lengths
    /// and font sizes move with real layout. A turn of the scheme runs the same way, under the
    /// transition of the theme it turns to. A change mid-way starts from the blend shown. A run
    /// keeps its spec until the next change.
    ///
    /// It applies at once, to the widgets recorded after the call, and asks for a frame that records
    /// the rest under it, under [`AnimationSpec::SNAP`], before the window's second frame, in the
    /// first frame after the window was hidden, or while transitions snap, with motion reduced
    /// ([`Self::is_motion_reduced`]) or under reduced power ([`Self::is_power_reduced`]). A theme that differs in its transition alone counts as a
    /// change.
    ///
    /// A live edit, one theme a frame, blends too: under [`AnimationSpec::THEME`] at 60 Hz it trails
    /// by 89 ms and adds 5 µs a frame over a snap on an Apple laptop. Refused: a second method that
    /// snaps one change, which every caller must choose between, and a window-wide spec, a mode that
    /// each caller must restore. Set it on a change: a call each frame with an equal theme starts nothing, but builds and compares a whole
    /// theme.
    ///
    /// ```
    /// # use palantir::{ColorScheme, Preference, Theme, Ui, UiPreferences};
    /// # fn one_look(ui: &mut Ui) {
    /// // An app with one look sets it and pins its scheme.
    /// ui.set_theme(ColorScheme::Dark, Theme::stock(ColorScheme::Dark))
    ///     .expect("a stock theme names no press effect");
    /// ui.set_preferences(UiPreferences {
    ///     scheme: Preference::Fixed(ColorScheme::Dark),
    ///     ..ui.preferences()
    /// });
    /// # }
    /// ```
    ///
    /// Each effect its press slots name ([`PressEffect`](crate::PressEffect)) builds with the
    /// alternatives it names, at the next frame that draws, with the other scheme's theme's, so
    /// no later press, turn of the scheme or switch of a preference stalls on a build: a theme
    /// for the scheme not shown builds at the latest with the frame that turns to it, before any
    /// press can reach it. A theme another set replaces before that frame builds nothing.
    /// Refused: a build at the first press, which stalls that frame; a build at the set, which a
    /// later set before the frame makes for nothing.
    ///
    /// # Errors
    ///
    /// A [`ThemeEffectError`] when a press slot names an effect not loaded, or not one over
    /// [`PressInput`], or a chrome's material one not loaded, or not a backdrop effect over
    /// [`MaterialInput`]; the theme held before stays.
    pub fn set_theme(
        &mut self,
        scheme: ColorScheme,
        theme: impl Into<Rc<Theme>>,
    ) -> Result<(), ThemeEffectError> {
        let theme = theme.into();
        let effects = self.resources.effects();
        let press = EffectTypes::of::<PressInput, ColorEffect>();
        let mut checked = Ok(());
        theme.press_effects(|name| {
            if checked.is_ok() {
                checked = match effects.types_of(name.inline()) {
                    None => Err(ThemeEffectError::UnloadedPress(name)),
                    Some(types) if types != press => Err(ThemeEffectError::NotPress(name)),
                    Some(_) => Ok(()),
                };
            }
        });
        let material = EffectTypes::of::<MaterialInput, BackdropEffect>();
        theme.material_effects(|name| {
            if checked.is_ok() {
                checked = match effects.types_of(name.inline()) {
                    None => Err(ThemeEffectError::UnloadedMaterial(name)),
                    Some(types) if types != material => Err(ThemeEffectError::NotMaterial(name)),
                    Some(_) => Ok(()),
                };
            }
        });
        checked?;
        let motion = self.theme_motion();
        if self.window_theme.set(scheme, theme, motion) {
            self.frame_runtime.request_repaint();
        }
        Ok(())
    }

    /// Whether a change of the shown theme may now animate. Not before the window's second frame:
    /// a change before the first or during it lands on a window that showed nothing, and one
    /// right after it is the system's first answer (`SystemPreferences::read`'s timeout).
    const fn theme_motion(&self) -> Motion {
        if !self.motion_snaps(MotionRole::Decorative) && self.frame_runtime.render_frame_id > 1 {
            Motion::Animate
        } else {
            Motion::Snap
        }
    }

    /// Whether a value transition of `role` lands on its end this frame: a decorative one while
    /// motion is reduced, or power is and the motion preference follows the system
    /// (`AppSettings::transitions`); any one when the window shows again after a hidden span,
    /// where a motion that ran unseen would show its last shown point and finish late.
    #[inline]
    const fn motion_snaps(&self, role: MotionRole) -> bool {
        self.host.window_frame.facts.shown_again
            || (matches!(role, MotionRole::Decorative)
                && self.frame_runtime.transitions.is_reduced())
    }

    /// The person's picks over the system's answers; each
    /// [`Preference::System`](crate::Preference::System) until set. App-global.
    #[inline]
    pub fn preferences(&self) -> UiPreferences {
        self.resources.settings().preferences()
    }

    /// Hold the person's picks, each taking effect as its field says, and ask every window for a
    /// frame when one moved. App-global. Persisting the picks is the app's.
    pub fn set_preferences(&mut self, preferences: UiPreferences) {
        if self.resources.settings().set_preferences(preferences) {
            self.frame_runtime.request_repaint();
        }
    }

    /// The scheme this frame shows: [`UiPreferences::scheme`] resolved against
    /// [`Self::system_color_scheme`], or [`ColorScheme::FALLBACK`] where nothing reports one.
    #[inline]
    pub const fn color_scheme(&self) -> ColorScheme {
        self.window_theme.scheme()
    }

    /// The scheme the system reports, whatever the preference; `None` where nothing reports one,
    /// as in a [`Harness`](crate::Harness) until its test sets one.
    #[inline]
    pub fn system_color_scheme(&self) -> Option<ColorScheme> {
        self.resources.settings().system_color_scheme()
    }

    /// Whether this frame runs with motion reduced: [`UiPreferences::motion`] resolved against
    /// [`Self::system_motion_reduced`] at the frame's start, so no frame mixes two, as transparency
    /// and power. Then every [`Self::animate`] and theme
    /// transition snaps to its target, as CSS `prefers-reduced-motion` asks of transitions, and a
    /// [decorative](crate::widget::MotionRole::Decorative) paint animation holds still; an
    /// [essential](crate::widget::MotionRole::Essential) one, as a spinner's turn or a caret's
    /// blink, still runs, as WCAG 2.3.3 lets essential motion stay
    /// (<https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions>). Under
    /// [reduced power](Self::is_power_reduced) with motion that follows the system, the
    /// transitions snap too, and paint loops run on. App-global.
    #[inline]
    pub const fn is_motion_reduced(&self) -> bool {
        self.frame_runtime.reductions.motion.is_reduced()
    }

    /// Whether the system asks for less motion, whatever the preference; `false` where nothing
    /// reports it, as in a [`Harness`](crate::Harness) until its test sets it.
    #[inline]
    pub fn system_motion_reduced(&self) -> bool {
        self.resources
            .settings()
            .system_reductions()
            .motion
            .is_reduced()
    }

    /// Whether this frame paints with transparency reduced: [`UiPreferences::transparency`]
    /// resolved against
    /// [`Self::system_transparency_reduced`] at the frame's start, so no frame mixes two. Then no
    /// capture, blur or effect runs, as CSS `prefers-reduced-transparency` asks, and every
    /// [`Material`](crate::Material)'s box draws as a plain box of its
    /// [`fallback`](crate::Material::fallback) in place of its fill: by default
    /// [`Theme::material_fallback`](crate::Theme::material_fallback), the theme's float surface,
    /// translucent and readable. Under [reduced power](Self::is_power_reduced)
    /// the system's answer reads as reduced, as Windows' Energy saver turns transparency effects
    /// off, so only a pick by hand keeps transparency whole. App-global.
    #[inline]
    pub const fn is_transparency_reduced(&self) -> bool {
        self.frame_runtime.reductions.transparency.is_reduced()
    }

    /// Whether the system asks for less transparency, whatever the preference; `false` where
    /// nothing reports it, as on Linux, which has no such setting, and in a
    /// [`Harness`](crate::Harness) until its test sets it.
    #[inline]
    pub fn system_transparency_reduced(&self) -> bool {
        self.resources
            .settings()
            .system_reductions()
            .transparency
            .is_reduced()
    }

    /// Whether this frame runs with power use reduced: [`UiPreferences::power`] resolved against
    /// [`Self::system_power_reduced`] at the frame's start, so no frame mixes two, and a move
    /// records every window again. Then the [frame rate](Self::is_frame_rate_reduced) and
    /// transparency reduce and every [`Self::animate`] and theme transition snaps where its
    /// preference follows the system, paint loops run on, and an app
    /// reads it to swap to cheaper paint, as Apple asks an app to reduce its activity in Low Power
    /// Mode (<https://developer.apple.com/documentation/foundation/processinfo/islowpowermodeenabled>).
    /// App-global.
    #[inline]
    pub const fn is_power_reduced(&self) -> bool {
        self.frame_runtime.reductions.power.is_reduced()
    }

    /// Whether this frame runs with the frame rate reduced: [`UiPreferences::frame_rate`] resolved
    /// against [`Self::is_power_reduced`] at the frame's start, so no frame mixes two. Then every
    /// host starts a frame at most at 30 Hz, on whole refresh periods, input frames included
    /// ([`FrameReport::earliest_repaint_at`](crate::FrameReport::earliest_repaint_at)), and paint
    /// loops run at that rate. App-global.
    #[inline]
    pub const fn is_frame_rate_reduced(&self) -> bool {
        self.frame_runtime.frame_rate.is_reduced()
    }

    /// Whether the system's power-saver mode is on, whatever the preference: macOS Low Power Mode,
    /// Windows' Energy saver or battery saver, Linux's power-saver profile. `false` where nothing
    /// reports it, as in a [`Harness`](crate::Harness) until its test sets it.
    #[inline]
    pub fn system_power_reduced(&self) -> bool {
        self.resources
            .settings()
            .system_reductions()
            .power
            .is_reduced()
    }

    /// At a frame's start, after its clock: move a theme transition one `dt` on, then adopt the
    /// scheme the app-global settings resolve to. A change of the shown theme moves the theme
    /// epoch, so the frame records again under it. A frame that snaps motion
    /// (`Self::motion_snaps`) snaps the theme; a scheme change starts its transition at the theme
    /// shown, so the frame that adopts it shows no step yet. The frame a running transition owes is asked
    /// for after the frame's plan takes this one's requests (`WindowFrames::frame`).
    fn advance_theme(&mut self) {
        let motion = self.theme_motion();
        let resolved = self.resources.color_scheme();
        self.window_theme
            .advance(self.frame_runtime.dt, resolved, motion);
    }

    // `peek_*` reads without declaring a wake. `watch_keyboard` and plain reads declare
    // "wake me when this changes"; forgetting that freezes pointer-derived paint,
    // while forgetting a peek only costs frames. Watches are cleared pre-record:
    // re-call each active frame.

    /// Declare interest in off-focus keyboard categories. One key or chord needs no watch: its
    /// query keeps it watched.
    #[inline]
    pub const fn watch_keyboard(&mut self, flags: KeyboardWake) {
        self.input.watch_keyboard(flags);
    }

    /// Keep `shortcut` watched without reading it, for a menu item that shows a chord its parent
    /// reads outside the record. Duplicate watchers collapse.
    #[inline]
    pub(crate) fn watch_shortcut(&mut self, shortcut: impl Into<Shortcut>) {
        self.input.watch_shortcut(shortcut.into());
    }

    /// This frame's pointer events, of every class a reader watched last frame, and a watch of
    /// `wake`: an event of those classes, off-target too, is kept for the next frame and wakes
    /// it. Watches are cleared before each record pass, so a reader reads in every frame it
    /// wants events in. Gated like [`Self::keyboard_events`]: an overlay's scope empties it for
    /// the planes strictly below, since watches bypass hit-testing.
    #[inline]
    pub fn pointer_events(&mut self, wake: PointerWake) -> &[PointerEvent] {
        self.input.watch_pointer(wake);
        self.input.pointer_events_at(self.forest.cur_plane())
    }

    /// This frame's presses, in arrival order, and a watch of `wake`. A press reaches the
    /// stream when a widget is focused, or a reader watched its key, its chord or
    /// [`KeyboardWake::KEY`] in the pass before, so a reader with nothing focused passes `KEY`; a focused
    /// widget passes [`KeyboardWake::NONE`]. Gated like [`Self::pointer_events`].
    #[inline]
    pub fn keyboard_events(&mut self, wake: KeyboardWake) -> &[KeyPress] {
        self.input.watch_keyboard(wake);
        self.input.keyboard_events_at(self.forest.cur_plane())
    }

    /// `true` if `key` went down this frame, OS repeats included, whatever the modifiers,
    /// and the press was granted to this record position's scope. Outside every scope, a press
    /// no scope on the focus path claims: a focused button keeps its Enter from an app that
    /// declares no scope of its own.
    ///
    /// A key reads in the pointer's words, pressed, [held](Self::key_held) and
    /// [released](Self::key_released), each whatever the modifiers, as Dear ImGui's
    /// `IsKeyPressed`, `IsKeyDown` and `IsKeyReleased` and egui's `key_pressed`, `key_down` and
    /// `key_released` read; a chord, whose modifiers match exactly, is a
    /// [shortcut](Self::shortcut_pressed). A press is routed; held and released are facts of
    /// the keyboard, as [`Self::modifiers`] is. Each watches its key, so an off-focus press or
    /// release wakes a frame.
    #[inline]
    pub fn key_pressed(&mut self, key: Key) -> bool {
        let reader = self.key_reader();
        self.input.key_pressed_at(reader, key)
    }

    /// `true` if `key` is down now, whatever the modifiers: from its press to its release, or
    /// to the surface losing the keyboard. Not routed.
    #[inline]
    pub fn key_held(&mut self, key: Key) -> bool {
        self.input.key_held(key)
    }

    /// `true` if `key` came up this frame, or was held when the surface lost the keyboard. A
    /// tap whose press and release share a frame reads pressed and released, not held. Not
    /// routed.
    #[inline]
    pub fn key_released(&mut self, key: Key) -> bool {
        self.input.key_released(key)
    }

    /// `true` if a press this frame matches `shortcut`, its modifiers exactly, and was granted
    /// to this record position's scope, as [`Self::key_pressed`] routes it.
    ///
    /// The read also watches the chord, so an off-focus press of it wakes the next frame. A read
    /// skipped behind a chord that fired, as the right side of `a || b` is, still watches: a
    /// frame that receives a press records again with no input, and that pass reads it.
    #[inline]
    pub fn shortcut_pressed(&mut self, shortcut: impl Into<Shortcut>) -> bool {
        let reader = self.key_reader();
        self.input.shortcut_pressed_at(reader, shortcut.into())
    }

    /// [`Self::key_pressed`] read as `reader` rather than the record position.
    #[inline]
    pub(crate) fn key_pressed_as(&mut self, reader: WidgetId, key: Key) -> bool {
        let reader = self.key_reader_as(reader);
        self.input.key_pressed_at(reader, key)
    }

    /// [`Self::shortcut_pressed`] read inside `reader`, as the scope around it, rather than at
    /// the record position: for a read made outside the node whose scope it speaks for, before
    /// that node opens or after it closes.
    #[inline]
    pub(crate) fn shortcut_pressed_as(
        &mut self,
        reader: WidgetId,
        shortcut: impl Into<Shortcut>,
    ) -> bool {
        let reader = self.key_reader_as(reader);
        self.input.shortcut_pressed_at(reader, shortcut.into())
    }

    /// [`Input::class_pressed_at`] read inside `reader`, as [`Self::shortcut_pressed_as`] is.
    #[inline]
    pub(crate) fn class_pressed_as(&mut self, reader: WidgetId, class: KeyClass) -> bool {
        let reader = self.key_reader_as(reader);
        self.input.class_pressed_at(reader, class)
    }

    /// The key reader at this record position.
    fn key_reader(&self) -> KeyReader {
        KeyReader {
            plane: self.forest.cur_plane(),
            parent: self.forest.cur_parent_id(),
        }
    }

    /// The key reader `reader` is, on this record position's plane.
    fn key_reader_as(&self, reader: WidgetId) -> KeyReader {
        KeyReader {
            plane: self.forest.cur_plane(),
            parent: Some(reader),
        }
    }

    /// A widget's first record has no rect, so what reads it takes this pass, as egui's `Grid` takes
    /// through `request_discard`.
    ///
    /// Re-record this frame after measure, once at most (roughly a 2x frame); prefer a layout
    /// driver. Asked in the frame's second pass, it asks for the next frame. An app that asks in every pass has a relayout loop: after a few frames, it logs an
    /// error and asks no more frames.
    ///
    /// # Panics
    ///
    /// Panics outside a record pass.
    #[track_caller]
    pub fn request_relayout(&mut self) {
        // `WindowFrames::frame` clears the flag before the app runs, so a call outside a
        // record would be dropped silently.
        assert!(
            self.forest.is_recording(),
            "Ui::request_relayout outside a record pass: it re-runs *this* \
             frame's record, so there is nothing for it to retry",
        );
        self.frame_runtime.request_relayout();
    }

    /// Monotonic time of the current frame, accumulated from host `dt`s. Pair with
    /// [`Self::request_repaint`] for continuous animation.
    #[inline]
    pub const fn now(&self) -> Duration {
        self.frame_runtime.stamp().time
    }

    /// Request the mouse cursor for this frame. Last writer wins; reset to
    /// [`CursorIcon::Default`] each record pass, so request it again every frame. Ignored
    /// headless.
    #[inline]
    pub const fn request_cursor(&mut self, cursor: CursorIcon) {
        self.host.window_requests.levels.cursor = cursor;
    }

    /// The cursor requested so far this record pass. Frame-scoped, unlike
    /// [`Self::vsync`].
    #[inline]
    pub const fn cursor(&self) -> CursorIcon {
        self.host.window_requests.levels.cursor
    }

    /// Set this window's presentation pacing, a retained level. Setting the mode in
    /// force is free; a real change recreates the swapchain after the frame.
    #[inline]
    pub const fn set_vsync(&mut self, vsync: Vsync) {
        self.host.window_requests.levels.vsync = vsync;
    }

    /// This window's presentation pacing, as last set or as the swapchain was opened.
    #[inline]
    pub const fn vsync(&self) -> Vsync {
        self.host.window_requests.levels.vsync
    }

    /// Ask the host for another frame after this one. Cleared at the top of every `frame`.
    pub fn request_repaint(&mut self) {
        tracing::trace!(
            target: "palantir.repaint",
            render_frame = self.frame_runtime.render_frame_id,
            "request_repaint",
        );
        self.frame_runtime.request_repaint();
    }

    /// Ask for an image of this frame: the whole window for `None`, else `region` in logical
    /// pixels, clamped to the window, and get the id to read the image by. The host copies the
    /// frame as it paints it, and the image arrives through [`Self::screenshot_taken`] in a frame
    /// after the device finishes the copy; on an [`OffscreenHost`](crate::OffscreenHost),
    /// [`wait_for_gpu`](crate::OffscreenHost::wait_for_gpu) and then one frame lands it. A frame
    /// paints in full when it asks. Each region a frame asks for is its own image; asking a
    /// region again in the same frame, as a replayed record pass does, returns the same id.
    ///
    /// An empty region, or one wholly outside the window, holds no pixel and asks nothing:
    /// `None`, with no image to wait for. An ask made between frames whose region the window
    /// shrinks past before the next frame lands nothing either.
    ///
    /// # Panics
    ///
    /// Panics if `region` is not finite: its corner and its size are *offsets*. A negative size
    /// is an empty region, as it paints nothing.
    #[track_caller]
    pub fn request_screenshot(&mut self, region: Option<Rect>) -> Option<ScreenshotId> {
        let window = self.frame_runtime.stamp().display.logical_rect();
        let region = region.map_or(window, |region| {
            vec2::offset(region.min);
            vec2::offset(Vec2::new(region.size.w, region.size.h));
            region.clamp_to(window)
        });
        if region.is_paint_empty() {
            return None;
        }
        self.frame_runtime.request_repaint();
        Some(self.host.window_requests.ask_screenshot(region))
    }

    /// The image the ask `id` of [`Self::request_screenshot`] got, during the one frame it lands
    /// in, as a response's `clicked` holds for the frame of the click; `None` in every other
    /// frame. Images land in the order they were asked for, one per frame, and the frame one
    /// lands in records.
    pub fn screenshot_taken(&self, id: ScreenshotId) -> Option<&Screenshot> {
        self.host
            .window_frame
            .screenshot
            .as_deref()
            .filter(|screenshot| screenshot.id() == id)
    }

    /// Schedule a one-shot wake at `now + after`. Duplicate deadlines collapse.
    pub fn request_repaint_after(&mut self, after: Duration) {
        tracing::trace!(
            target: "palantir.repaint",
            ?after,
            render_frame = self.frame_runtime.render_frame_id,
            "request_repaint_after",
        );
        let deadline = self.frame_runtime.stamp().time.saturating_add(after);
        self.frame_runtime.schedule_wake(
            deadline,
            WakeReasons::REAL,
            self.frame_runtime.stamp().display.refresh_millihertz,
        );
    }

    /// Open a top-level OS window addressed by `token`, unique among live windows.
    /// Deferred to the host after this frame, which applies every window's opens and
    /// closes in the order they were called, the last call per token winning: a later
    /// `config` replaces an earlier one, a later [`Self::close_window`] cancels the open,
    /// and an open after a close of a live window recreates it. An open of a live
    /// `token` is ignored with a warning. A window the platform or GPU cannot open is
    /// logged and stays closed, so [`Self::is_window_open`] answers `false`. A host with one
    /// window, as [`OffscreenHost`](crate::OffscreenHost) and [`Harness`](crate::Harness), opens
    /// none and records the command in its `window_commands`.
    ///
    /// # Panics
    ///
    /// Panics unless each size `config` holds is positive on both axes, as
    /// [`WindowConfig::with_inner_size`] asks.
    #[track_caller]
    pub fn open_window(&mut self, token: WindowToken, config: WindowConfig) {
        config.validate();
        self.host.window_requests.commands.open(token, config);
    }

    /// Request that the window addressed by `token` close, deferred and ordered like
    /// [`Self::open_window`]. No-op for an unknown `token`. A host with one window records the
    /// command, as [`Self::open_window`] says.
    #[inline]
    pub fn close_window(&mut self, token: WindowToken) {
        self.host.window_requests.commands.close(token);
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
        self.host.window_frame.facts.close_requested
    }

    /// Veto the auto-close pending from [`Self::close_requested`].
    #[inline]
    pub const fn keep_open(&mut self) {
        self.host.window_requests.close_vetoed = true;
    }

    /// This window's geometry for persist-and-restore: where it sits, and the size it returns
    /// to from maximized, its live size while it is not maximized. Placement position is
    /// `None` where the platform reports none (Wayland). In the window manager's
    /// logical pixels (see [`Display::system_logical_size`]), since the platform never
    /// hears about [`Self::set_user_scale`].
    pub fn window_geometry(&self) -> WindowGeometry {
        let display = self.frame_runtime.stamp().display;
        let facts = &self.host.window_frame.facts;
        let physical = facts.restored_size.unwrap_or(display.physical);
        // A window skips its frames while hidden, minimized included, so one that records has an
        // extent, as `WindowConfig::with_geometry` asks.
        debug_assert!(
            physical.min_element() > 0,
            "a recording window has a size: {physical}"
        );
        let scale = display.system_scale.get();
        WindowGeometry {
            inner_size: Size::new(physical.x as f32 / scale, physical.y as f32 / scale),
            placement: facts.placement,
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
        self.resources.settings().debug_overlay()
    }

    /// Replace this app's debug-overlay flags, visible in every window.
    #[inline]
    pub fn set_debug_overlay(&mut self, overlay: DebugOverlayConfig) {
        if self.resources.settings().set_debug_overlay(overlay) {
            self.frame_runtime.request_repaint();
        }
    }

    /// This window's most recent GPU instrumentation sample, which the backend publishes and the
    /// debug overlay reads; empty unless the host collects GPU stats.
    pub fn gpu_pass_stats(&self) -> &GpuPassStats {
        &self.host.gpu_pass_stats
    }

    /// This frame's diagnostic counters, for the [`frame_stats`] overlay.
    ///
    /// [`frame_stats`]: crate::diagnostics::frame_stats
    fn frame_stats(&self) -> FrameStats {
        FrameStats {
            record_frame_id: self.frame_runtime.record_frame_id,
            render_frame_id: self.frame_runtime.render_frame_id,
            fps: self.frame_runtime.fps_ema,
            settle_frames: self.frame_runtime.settle_frames(),
            gpu: self.host.gpu_pass_stats.last_pass(),
        }
    }

    /// Whether the window addressed by `token` is live, as of this frame's start;
    /// changes made this frame show next frame.
    #[inline]
    pub fn is_window_open(&self, token: WindowToken) -> bool {
        self.resources.windows().contains(token)
    }

    /// Attach `shape` to the active node, covered by `paint`: each shape takes the paints its
    /// pipeline draws, listed as [`PaintedBy`]'s implementors. Direct text counts toward layout
    /// only on a leaf; container-owned text is an overlay shaped against the container's padded
    /// width. The added shape, `None` where the pair paints nothing, can still be animated at paint
    /// time ([`AddedShape::animate`]).
    ///
    /// The shape checks `paint`, the two become one draw, and the frame's record store records it
    /// into the node, or drops it where it paints nothing: a transparent paint, a rect of no size,
    /// empty text.
    ///
    /// # Panics
    ///
    /// When `paint` holds a value outside its kind, as [`Paint`](crate::Paint) lists, or draws
    /// what the shape's pipeline cannot, as [`PaintedBy`] lists; when it is an
    /// [`EffectPaint`](crate::widget::EffectPaint) whose effect is not loaded, or loaded with
    /// other parameters or kind; while power is reduced, the same for the alternative it paints
    /// in place of it.
    ///
    /// ```
    /// # use palantir::widget::{PaintAnimation, PaintRepeat, Shape, curves};
    /// # use palantir::{Rect, RgbaF32, Ui};
    /// # use std::time::Duration;
    /// # fn demo(ui: &mut Ui) {
    /// let rect = Shape::rect().at(Rect::new(0.0, 0.0, 8.0, 8.0));
    /// if let Some(shape) = ui.add_shape(rect, RgbaF32::WHITE) {
    ///     shape.animate(
    ///         PaintAnimation::alpha(0.4, 1.0)
    ///             .with_period(Duration::from_secs(2))
    ///             .with_repeat(PaintRepeat::Forever)
    ///             .with_curve(curves::sine),
    ///     );
    /// }
    /// # }
    /// ```
    #[track_caller]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "a shape and a paint are builder values, taken by value as a builder is; the draw borrows them for the call"
    )]
    pub fn add_shape<S: PaintedBy<P>, P>(&mut self, shape: S, paint: P) -> Option<AddedShape<'_>> {
        let paint = S::prepare(paint);
        shape.check(&paint);
        let draw = shape.draw(&paint);
        let reads_time = draw.reads_time();
        let resolve = Resolve {
            effects: self.resources.effects(),
            reductions: self.frame_runtime.reductions,
            material_fallback: self.theme().material_fallback,
        };
        let slot = self.forest.add_draw(draw, resolve)?;
        Some(AddedShape::new(self, slot, reads_time))
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
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Bytes only: a font on disk is read by the caller, as `std::fs::read(path)?` gives a
    /// `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// [`FontLoadError::NoFaces`] when the bytes hold no parsable face,
    /// [`FontLoadError::FamilyNameTooLong`] when every family it names is longer than a
    /// [`FontFamily`] holds.
    #[inline]
    pub fn load_font(&self, source: impl Into<FontSource>) -> Result<FontFamily, FontLoadError> {
        self.resources.text().load_font(source)
    }

    /// Load the effect `source` under `name`, for the app's life, as a font loads; the same name
    /// again replaces it, so a reload is this call with the new source. The WGSL is parsed with
    /// palantir's prelude and validated, `fn effect` is checked against the contract
    /// ([`EffectSource`]), and its parameter struct against `U`, member for member. On a host
    /// with a device, its pipelines build when its source's [`EffectBuild`](crate::EffectBuild)
    /// says, and a wanted effect reloaded builds again before its next frame. Names that start
    /// with `palantir.` are palantir's stock effects ([`EffectName::RIPPLE`]). App-global.
    /// Refused: a handle that unloads an effect when it drops, a second route to one effect beside
    /// its name.
    ///
    /// # Errors
    ///
    /// An [`EffectError`] case; the effect loaded under `name` before, if any, stays.
    ///
    /// # Panics
    ///
    /// When `name`, or an alternative `source` names, was loaded before with another
    /// parameter type or kind, or the alternative with other image slots.
    #[inline]
    #[track_caller]
    pub fn load_effect<U: EffectParams, K: EffectKind>(
        &self,
        name: EffectName<U, K>,
        source: EffectSource<U, K>,
    ) -> Result<(), EffectError> {
        self.resources.effects().load(name, source)
    }

    /// Whether an effect is loaded under `name`. App-global.
    ///
    /// # Panics
    ///
    /// When it was loaded with another parameter type or kind.
    #[inline]
    #[track_caller]
    pub fn has_effect<U: EffectParams, K: EffectKind>(&self, name: EffectName<U, K>) -> bool {
        self.resources.effects().contains(name)
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
    pub fn max_image_dimension(&self) -> Option<NonZeroU32> {
        self.resources.max_image_dimension()
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

    /// Record `view`: refresh its row in the window's `GpuViews` and append an image
    /// shape carrying the row's target and `epoch` to the active node.
    pub(crate) fn gpu_view(&mut self, view: RecordedView) {
        let frame = self.frame_runtime.render_frame_id;
        let target = self.memory.gpu_views.record(view, frame);
        self.forest.add_gpu_view(target.id, target.epoch);
    }

    /// Format `args` into the record-pass text storage and return an arena-backed
    /// [`InternedStr`]. Usually reached through [`fmt!`](crate::fmt).
    ///
    /// Valid only for the pass that minted it, in this window; holding it into a later
    /// pass or another window panics.
    #[must_use]
    #[inline]
    pub fn fmt(&mut self, args: fmt::Arguments<'_>) -> InternedStr {
        self.forest.record_store.text.intern_fmt(args)
    }

    /// Normalize borrowed, owned or interned text into an [`InternedStr`]. An
    /// [`InternedStr`] from an earlier pass or another window panics.
    #[must_use]
    pub fn intern<'a>(&mut self, text: impl Into<TextInput<'a>>) -> InternedStr {
        self.forest.record_store.text.intern(text.into())
    }

    /// The characters of `text`, a handle this pass interned. The borrow holds
    /// `self`; copy to a scratch `String` first if you also need `&mut Ui`.
    ///
    /// # Panics
    ///
    /// Panics when `text` was interned by an earlier pass or another window.
    #[must_use]
    pub fn text(&self, text: InternedStr) -> &str {
        self.forest.record_store.text.text(text)
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

    /// The overlay bodies recording now.
    pub(crate) const fn overlays(&mut self) -> &mut OverlayStack {
        &mut self.overlays
    }

    /// Close the innermost overlay recording now: the [`Popup`](crate::Popup),
    /// [`Modal`](crate::Modal) or menu whose body this call runs in, which reports
    /// [`OverlayClose::Requested`](crate::OverlayClose::Requested) this frame. A dialog's button
    /// and an activated [`MenuItem`](crate::MenuItem) close their overlay so, as egui's
    /// `Ui::close` closes the innermost closable container
    /// (<https://docs.rs/egui/latest/egui/struct.Ui.html#method.close>). A no-op outside every
    /// overlay, where a row shown in the main tree has nothing to close. Refused: a close handle
    /// passed into each body, a second route to the same close.
    #[inline]
    pub fn close_overlay(&mut self) {
        self.overlays.close_innermost();
    }

    /// Withdraw an [`input_scope`](crate::Configure::input_scope) this pass recorded.
    /// Holds through the end of the next frame; the current pass is unaffected, since
    /// scope paths resolve at pass start against a one-frame-old cascade.
    #[inline]
    pub fn withdraw_input_scope(&mut self, id: WidgetId) {
        self.input.withdraw_input_scope(id);
    }

    /// Resolve a widget's identity recipe against the open parent into its record
    /// id. [`Widget::resolve`] is the one caller.
    ///
    /// [`Widget::resolve`]: crate::widget::Widget::resolve
    #[inline]
    pub(crate) fn resolve_ident(&mut self, recipe: IdRecipe) -> ResolvedId {
        self.forest.widget_id(recipe)
    }

    /// Open `node` under `resolved`, painting `chrome` and, for keyboard-sourced
    /// focus, the theme's focus ring. Pairs with [`Self::close_node`]. Widget code
    /// calls `Widget::record`, never this.
    #[inline]
    #[track_caller]
    pub(crate) fn open_node(&mut self, resolved: ResolvedId, node: &Node, chrome: Option<&Chrome>) {
        let ring = if self.input.focus() == Some(resolved.id()) && self.input.focus_visible() {
            let theme = &self.theme().focus_ring;
            Border::new(domain::color(theme.color), domain::length(theme.width))
        } else {
            Border::NONE
        };
        let resolve = Resolve {
            effects: self.resources.effects(),
            reductions: self.frame_runtime.reductions,
            material_fallback: self.theme().material_fallback,
        };
        let material = chrome
            .and_then(|chrome| chrome.paint.material.as_ref())
            .and_then(|material| MaterialRead::resolve(material, resolve));
        self.forest.open_node(
            resolved,
            node,
            NodeChrome {
                chrome,
                ring,
                material,
            },
        );
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
        self.input.arranged.scroll_content(id)
    }

    /// The [`Response`] of the widget `id`, probed now. `rect` and `disabled` are last
    /// frame's; interaction fields are computed against this frame's input, so read it
    /// during record. The widget's own `NodeFlags::is_disabled` is folded in by
    /// `Widget::response`.
    pub fn response_for(&self, id: WidgetId) -> Response {
        let loc = self.last_frame_location(id);
        let mut state = self.input.response_state(id, loc);
        // Cascade lags a frame; fold in this frame's ancestor-disabled so a newly
        // disabled subtree paints disabled at once.
        state.merge_disabled(self.forest.ancestor_disabled());
        Response { id, state }
    }

    /// Where the most recent cascade run put `id`, which a widget reads as it resolves: the
    /// newest id of this pass, so its position is the last one.
    #[inline]
    fn last_frame_location(&self, id: WidgetId) -> Option<WidgetLocation> {
        let newest = self.forest.ids.resolved_count().wrapping_sub(1);
        self.input.arranged.cascade.locate_at(id, newest)
    }

    /// The cross-frame state row for `id`, or `None` if `(id, S)` was never stored. Creates
    /// nothing.
    ///
    /// # Panics
    ///
    /// While the row is lent to a [`Self::with_state`] body, which holds it as `&mut S`.
    #[track_caller]
    pub fn state<S: 'static>(&self, id: WidgetId) -> Option<&S> {
        self.memory.state.try_get::<S>(id)
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
    /// not; a row under an id no node records lives as long as the `Ui`. The row is lent, not
    /// copied: `S::default()` is built only for a new row.
    ///
    /// # Panics
    ///
    /// While the row is lent already: inside `body`, the same `(id, S)` is `body`'s `&mut S`, and
    /// a second lend or a [`Self::state`] read of it panics, as `RefCell::borrow_mut` does.
    #[track_caller]
    pub fn with_state<S: Default + 'static, R>(
        &mut self,
        id: WidgetId,
        body: impl FnOnce(&mut Self, &mut S) -> R,
    ) -> R {
        let mut value = self.memory.state.lend::<S>(id).unwrap_or_default();
        let out = body(self, &mut value);
        // Re-probed: `body` may have inserted rows of the same `S`, reallocating the store.
        self.memory.state.put(id, value);
        out
    }

    /// Store `value` as `id`'s row of `S`, in place of any it had: for a row that holds a value
    /// only while something is under way, as an open menu's target, so the map, not an `Option`
    /// in the row, says when there is none.
    ///
    /// # Panics
    ///
    /// While the row is lent to a [`Self::with_state`] body.
    #[track_caller]
    pub fn set_state<S: 'static>(&mut self, id: WidgetId, value: S) {
        self.memory.state.set(id, value);
    }

    /// Drop `id`'s row of `S`, returning its value; `None` when it has none.
    ///
    /// # Panics
    ///
    /// While the row is lent to a [`Self::with_state`] body.
    #[track_caller]
    pub fn remove_state<S: 'static>(&mut self, id: WidgetId) -> Option<S> {
        self.memory.state.remove::<S>(id)
    }

    /// The one `S` this `Ui` holds, or `None`: the state row of `S` under an id of its type, which
    /// no node records, so it is never swept. For state a widget kind shares across instances.
    /// Sugar over [`Self::state`], as egui's `IdTypeMap` keeps a global under `Id::NULL`.
    ///
    /// # Panics
    ///
    /// While `S` is lent to a [`Self::with_singleton`] body.
    #[track_caller]
    pub fn singleton<S: 'static>(&self) -> Option<&S> {
        self.state::<S>(singleton_id::<S>())
    }

    /// Lend the one `S` to `body` beside the `Ui`, `S::default()` on first use; sugar over
    /// [`Self::with_state`] under the id of `S`.
    ///
    /// # Panics
    ///
    /// While `S` is lent already, as [`Self::with_state`] panics.
    #[track_caller]
    pub fn with_singleton<S: Default + 'static, R>(
        &mut self,
        body: impl FnOnce(&mut Self, &mut S) -> R,
    ) -> R {
        self.with_state::<S, R>(singleton_id::<S>(), body)
    }

    /// Advance the animation row `request` names toward its target and return the current
    /// value, and whether the motion is at rest there. [`AnimationSpec::SNAP`] lands on the
    /// target now, settled, and requests no repaint, as every spec of a decorative request does
    /// with motion reduced and under reduced power where motion follows the system
    /// ([`AnimationRequest::with_motion`](crate::widget::AnimationRequest::with_motion)), and every
    /// spec on the first frame after the window was hidden; a row it leaves goes at the end of the
    /// frame.
    ///
    /// # Panics
    ///
    /// When an animated target is not finite, which would never settle.
    // Keep the snap's return in the widget's block so a static theme does not pay an
    // outlined call plus a large `Animated<V>` return-slot handoff.
    #[inline(always)]
    #[track_caller]
    pub fn animate<V: Animatable>(
        &mut self,
        request: AnimationRequest<V>,
        spec: AnimationSpec,
    ) -> Animated<V> {
        let mut request = request;
        let settled = self.animate_in_place(&mut request, spec);
        Animated {
            value: request.target,
            settled,
        }
    }

    /// [`Self::animate`] into `request.target`, returning whether the motion is at rest: a look
    /// worn each frame moves through no copies (`.notes/PERF.md`, "A look animates in place").
    #[track_caller]
    pub(crate) fn animate_in_place<V: Animatable>(
        &mut self,
        request: &mut AnimationRequest<V>,
        spec: AnimationSpec,
    ) -> bool {
        let spec = if self.motion_snaps(request.motion) {
            AnimationSpec::SNAP
        } else {
            spec
        };
        let clock = AnimationClock {
            dt: self.frame_runtime.dt,
            render_frame_id: self.frame_runtime.render_frame_id,
        };
        let settled = self.memory.animation.animate(request, spec, clock);
        if !settled {
            self.frame_runtime.request_repaint();
        }
        settled
    }

    /// Currently focused widget id, or `None`.
    #[inline]
    pub const fn focus(&self) -> Option<WidgetId> {
        self.input.focus()
    }

    /// True when focus sits on `ancestor` or in its subtree, per the most recent
    /// cascade (one frame of lag). Layers are separate trees, so popup focus is not
    /// within its anchor.
    #[inline]
    pub fn is_focus_within(&self, ancestor: WidgetId) -> bool {
        self.input.is_focus_within(ancestor)
    }

    /// True when the pointer's hover target is `ancestor` or in its subtree.
    /// Occlusion-aware, empty while another widget holds a press of any button, as
    /// [`ResponseState::hovered`](crate::ResponseState::hovered) reads, and changes only when a repaint
    /// is already scheduled.
    #[inline]
    pub fn is_hover_within(&self, ancestor: WidgetId) -> bool {
        self.input.is_hover_within(ancestor)
    }

    /// Active `Display` (physical surface size and both scale factors).
    #[inline]
    pub const fn display(&self) -> Display {
        self.frame_runtime.stamp().display
    }

    /// The application's own scale, multiplied onto the platform's. `UserScale::ONE`
    /// until set.
    #[inline]
    pub fn user_scale(&self) -> UserScale {
        self.resources.settings().user_scale()
    }

    /// Scale the whole UI by `scale` on top of the platform factor. App-global;
    /// takes effect next frame, which a change asks for, so this frame's [`Self::display`]
    /// reports the old value.
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
        if self.resources.settings().set_user_scale(scale) {
            self.frame_runtime.request_repaint();
        }
    }

    /// This frame's monotonic index over frames authoring code ran on. Retained
    /// state stamps it to notice a gap (it was skipped); paint-only frames advance
    /// nothing. See [`Self::render_frame_id`].
    #[inline]
    pub const fn record_frame_id(&self) -> u64 {
        self.frame_runtime.record_frame_id
    }

    /// The last frame's interactive widgets, in paint order, into `out`: what a tool that
    /// drives the `Ui` from outside finds a widget by.
    pub fn inspect(&self, out: &mut Vec<InspectedWidget>) {
        InspectedWidget::collect(&self.forest, &self.input.arranged.cascade, out);
    }

    /// How soon a second press on the same target makes a double click: what the winit host sets
    /// for the platform, else 500 ms. A driver that sends separate clicks spaces them past it.
    #[inline]
    pub fn double_click_interval(&self) -> Duration {
        self.resources.settings().double_click_interval()
    }

    /// This frame's monotonic index among frames that reached the screen. Counts
    /// paint-only frames, so use [`Self::record_frame_id`] to detect a skipped reader.
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
    ///
    /// A disabled widget, itself or by an ancestor as the last cascade holds it, has no edges,
    /// as its `response_for` read outside a disabled subtree reports none.
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
        self.input.set_focus(id);
    }

    /// Leave nothing focused; same routing lag as [`Self::set_focus`].
    #[inline]
    pub const fn clear_focus(&mut self) {
        self.input.clear_focus();
    }

    /// Ask for IME text this frame, with `area` (logical px) the caret's rect, beside which the
    /// platform places its candidate list. A level asked for every frame it is wanted; a frame
    /// with no call turns IME off.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `area` is an
    /// [offset](crate::widget::domain::offset).
    #[inline]
    #[track_caller]
    pub const fn request_ime_area(&mut self, area: Rect) {
        area.validate();
        self.host.window_requests.levels.ime_area = Some(area);
    }

    /// The IME area requested so far this record pass, `None` for off. Frame-scoped, like
    /// [`Self::cursor`]: a host that embeds the UI enables its input method while it is `Some`
    /// after a frame; the winit host does it itself.
    #[inline]
    pub const fn ime_area(&self) -> Option<Rect> {
        self.host.window_requests.levels.ime_area
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
        self.focus_within(ancestor, TabEnd::First);
    }

    /// Move focus at the end of this frame to the Tab stop at `end` under `ancestor`: the first,
    /// as [`Self::focus_first_within`] does, or the last, as a list End opens lands on its last
    /// row.
    pub const fn focus_within(&mut self, ancestor: WidgetId, end: TabEnd) {
        self.input.focus_within(ancestor, end);
    }

    /// Pointer position in logical pixels (surface space), or `None` off-surface.
    /// `&mut` because reading auto-asserts a [`PointerWake::MOVE`] watch so
    /// pointer-derived paint keeps repainting. For hover styling prefer
    /// [`Self::is_hover_within`].
    #[inline]
    pub const fn pointer_pos(&mut self) -> Option<Vec2> {
        self.input.pointer_pos()
    }

    /// Currently held modifier keys. Auto-watches [`KeyboardWake::MODIFIER`]; when
    /// gated on something that already woke the frame, use [`Self::peek_modifiers`].
    #[inline]
    pub const fn modifiers(&mut self) -> Modifiers {
        self.input.modifiers()
    }

    /// [`Self::pointer_pos`] without the [`PointerWake::MOVE`] watch. Right only
    /// when another event guarantees the frame; stale for continuously derived paint.
    #[inline]
    pub const fn peek_pointer_pos(&self) -> Option<Vec2> {
        self.input.peek_pointer_pos()
    }

    /// [`Self::modifiers`] without the [`KeyboardWake::MODIFIER`] watch. Use the
    /// watched read when a bare modifier press must repaint on its own.
    #[inline]
    pub const fn peek_modifiers(&self) -> Modifiers {
        self.input.peek_modifiers()
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

/// The state-map id the one `S` of a `Ui` lives under: of its type, so no widget id meets it.
fn singleton_id<S: 'static>() -> WidgetId {
    WidgetId::from_hash(("palantir.singleton", std::any::TypeId::of::<S>()))
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::theme::Theme;

    #[cfg(test)]
    use crate::animation::AnimationMap;
    #[cfg(test)]
    use crate::input::Input;
    use crate::layout::Layout;
    #[cfg(test)]
    use crate::layout::plane_layout::PlaneLayout;
    #[cfg(test)]
    use crate::primitives::geometry::rect::Rect;
    #[cfg(test)]
    use crate::primitives::input::pointer::PointerEvent;
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
    use crate::shape::record_store::RecordStore;
    #[cfg(test)]
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    #[cfg(test)]
    use crate::ui::frame_runtime::FrameRuntime;
    #[cfg(test)]
    use crate::window::window_frame_state::WindowFrameState;
    #[cfg(test)]
    use crate::window::window_requests::WindowRequests;

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
        pub(crate) const fn layout(&self) -> &Layout {
            &self.input.arranged.layout
        }

        /// Close the input frame as the end of a frame does (dropping its edges, then applying
        /// held events) and return whether input owes another. For a caller streaming input with
        /// no frame recorded between.
        pub(crate) fn end_input_frame(&mut self) -> bool {
            self.input.end_frame();
            self.input.next_frame(self.double_click_interval())
        }
    }

    /// Only this crate's own tests call these.
    #[cfg(test)]
    impl Ui {
        /// This frame's record payloads, valid until the next record pass refills the arena.
        pub(crate) const fn record_store(&self) -> &RecordStore {
            &self.forest.record_store
        }

        /// The input machine, mutably, for tests that drive routing state.
        pub(crate) const fn input_mut(&mut self) -> &mut Input {
            &mut self.input
        }

        pub(crate) const fn frame_runtime(&self) -> &FrameRuntime {
            &self.frame_runtime
        }

        /// One plane's recorded tree; a `Layer` names its base plane.
        pub(crate) fn tree(&self, plane: impl Into<Plane>) -> &Tree {
            &self.forest.trees[plane.into()]
        }

        /// One plane's arranged columns; a `Layer` names its base plane. Callers wanting the whole
        /// table use [`Self::layout`].
        pub(crate) fn plane_layout(&self, plane: impl Into<Plane>) -> &PlaneLayout {
            &self.input.arranged.layout[plane.into()]
        }

        /// `node`'s arranged rect on `plane`: pre-transform, unclipped, world coords.
        pub(crate) fn arranged_rect(&self, plane: impl Into<Plane>, node: NodeId) -> Rect {
            self.input.arranged.layout.arranged_rect(Endpoint {
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
            &mut self.memory.animation
        }

        pub(crate) fn window_requests(&self) -> &WindowRequests {
            &self.host.window_requests
        }

        /// The facts a host publishes before a frame, for tests that act as the host.
        pub(crate) fn window_frame_mut(&mut self) -> &mut WindowFrameState {
            &mut self.host.window_frame
        }

        /// [`Ui::pointer_events`] read between frames, where a read watches nothing.
        pub(crate) fn captured_pointer_events(&self) -> &[PointerEvent] {
            self.input.pointer_events_at(self.forest.cur_plane())
        }
    }

    impl Ui {
        /// How many explicit-id collisions this frame outlined in magenta, for a scene's own test.
        pub const fn explicit_id_collision_count(&self) -> usize {
            self.forest.collisions.len()
        }

        /// The shown theme, for in-place edits. Gated: apps build a [`Theme`] and call
        /// [`set_theme`](Ui::set_theme). Copy-on-write, so a handle from [`Ui::theme`]
        /// keeps its values.
        pub fn theme_mut(&mut self) -> &mut Theme {
            self.window_theme.edit()
        }
    }
}

#[cfg(test)]
mod tests;
