// The README's showcase recording is a bare GitHub attachment URL, which is
// the only form GitHub expands into an inline video player — wrapping it for
// rustdoc's sake would turn it back into a dead link on the repo page.
#![allow(rustdoc::bare_urls)]
// The README's counter example builds a `WinitHost`, so it only compiles as a
// doctest when that feature is on. Without it the crate docs open at the
// orientation section below instead.
#![cfg_attr(feature = "winit", doc = include_str!("../README.md"))]
// `WinitHost`, `WinitHostConfig` and `HostHandle` are the windowed host's own
// types, and the docs on the backend-agnostic items around them — `Ui`'s
// window commands, `WindowConfig`, `WindowToken` — say what that host does
// with each. Those sentences are worth as much to a reader building without
// the feature, so they stay whole and their links go unresolved in that build
// rather than every one of them carrying a second, link-free copy of itself.
// The price is that a link broken for any other reason also passes there —
// the default build is the one that still denies them.
#![cfg_attr(not(feature = "winit"), allow(rustdoc::broken_intra_doc_links))]
//!
//! # Where to start
//!
//! - [`App`] is the lifecycle trait your application implements. Its
//!   [`record`](App::record) runs every frame and describes the whole UI from
//!   scratch — there is no retained widget tree to mutate.
//! - [`WinitHost`] owns the event loop, windows, and GPU device, and calls
//!   `record` for you. [`OffscreenHost`] is its headless peer: same `Ui`, same
//!   frame lifecycle, rendering into a `wgpu::Texture` you supply.
//! - [`Ui`] is the recorder handed to `record`. Widgets are appended to it,
//!   cross-frame widget state hangs off it, and [`Ui::layer`] switches which
//!   of the [`Layer`] arenas (main / popup / modal / tooltip / debug) receives
//!   subsequent records.
//! - Widgets are builders terminated by `show(ui)`: [`Button`], [`Text`],
//!   [`TextEdit`], [`Slider`], [`Checkbox`], [`ComboBox`], [`Scroll`],
//!   [`Popup`], [`Modal`] and the rest. Layout containers are [`Panel`]
//!   (h/v/z-stack and canvas) and [`Grid`].
//! - [`Configure`] carries the settings every node shares — identity, size,
//!   padding, margin, alignment, visibility — so the same builder methods work
//!   on any widget.
//! - [`Theme`] is the one serializable style tree; per-widget sub-themes hang
//!   off it.
//! - [`GpuView`] hands a widget-sized `wgpu` render target to your own
//!   [`GpuPaint`] implementation and composites the result like any other
//!   image, so it clips, rounds, and z-orders with everything else.
//!
//! # Sizing
//!
//! Layout is a WPF-style two pass (measure, then arrange), and [`Sizing`] is
//! the vocabulary both passes speak:
//!
//! - [`Sizing::fixed`] is an exact extent, and is allowed to exceed the parent.
//! - [`Sizing::HUG`] is `min(content, available)`, floored at the largest
//!   non-shrinkable thing inside (a fixed descendant, an explicit minimum, the
//!   longest unbreakable word).
//! - [`Sizing::fill`] takes the leftover, split between fill siblings by
//!   weight; a sibling whose floor exceeds its share freezes at the floor and
//!   the rest re-divide.
//!
//! Children clamp down to fit their parent — a parent never grows to fit a
//! child. Overflow happens only when rigid descendants genuinely do not fit.
//!
//! # Feature flags
//!
//! | flag | default | what it does |
//! | --- | --- | --- |
//! | `winit` | yes | The winit-backed [`WinitHost`] — real windows, a real event loop, and [`TextEdit`] cut/copy/paste through the OS clipboard. Without it only [`OffscreenHost`] exists, and clipboard traffic stays in an in-process buffer. |
//! | `showcase` | no | Builds the bundled `showcase` binary, a tour of every widget. |
//! | `gpu-debug-markers` | no | Emits GPU debug groups around every draw step for RenderDoc / Xcode captures. Costs two recorded commands and a label copy per step even with no capture tool attached, so it is off unless you intend to capture. |
//! | `profile-with-tracy` | no | Opens a Tracy zone over each frame pass, and marks a frame set per window. Needs the external Tracy viewer. |
//! | `internals` | no | Test reach-ins — adds the `internals` module. **Not a supported API**: it exists so the integration tests under `tests/` can reach crate privates, and it breaks without notice. |
//! | `bench` | no | The source-level benchmark drivers, and the function-only facade the thin targets under `benches/` call. Implies `internals`, and adds the harness crates on top. Not a supported API either. |
//! | `golden` | no | Adds the `golden` module — golden-image regression testing for suites that draw through Palantir. Its own flag because it is the only part of the surface that costs an image codec. |
//!
//! # Colour
//!
//! [`Color`] holds **straight-alpha linear RGB**. The convenience constructors
//! ([`Color::rgb`], [`Color::hex`], [`Color::rgb_u8`]) read their input as
//! sRGB-perceptual and linearise it for you; [`Color::linear_rgb`] and
//! [`Color::linear_rgba`] take values that are already linear. Everything
//! downstream — blending, anti-aliasing, animation — runs in linear, and the
//! sRGB encode happens on the GPU when writing the swapchain. Writing
//! already-sRGB-encoded values into [`Color`] skips the linearisation and will
//! come out wrong.

// Re-import `palantir` as a self-alias so proc-macros that emit
// `::palantir::Animatable` paths (from `palantir-anim-derive`) resolve
// when the derive is used *inside* the crate (e.g. on `Stroke`,
// `Background`). Outside the crate this path resolves naturally.

extern crate self as palantir;

// Crate-internal macros are declared at the top of the `mod.rs` that owns
// them, above its module list. `macro_rules!` without `#[macro_export]` is
// scoped textually, so that placement — and only that placement — hands the
// macro to every file in that subtree and to nothing outside it. `widgets`,
// `widgets::theme`, `shape`, `primitives`, and `primitives::brush::gradient`
// each carry their own set on those terms. Collecting them into one shared
// module would take `#[macro_use]` and would widen every macro's reach to the
// whole crate, which is the property this arrangement exists to deny.

pub(crate) mod animation;
pub(crate) mod app;
#[cfg(feature = "bench")]
pub mod bench;
pub(crate) mod common;
/// Accent swatches shared by the two bundled demo surfaces. Public only
/// because the `showcase` binary is a separate crate from this library
/// and cannot reach a `pub(crate)` one; not part of the supported API.
#[cfg(any(feature = "internals", feature = "showcase"))]
pub mod demo_swatches;
pub(crate) mod diagnostics;
/// Per-output display state (physical size, DPR, pixel-snap, refresh) —
/// cross-cutting host/render vocabulary, read by `ui`, the renderer, and
/// the host layer; not owned by any one subsystem.
pub(crate) mod display;
/// The shared benchmark workload — one designed app screen recorded by the
/// frame, allocation, and cascade benches alike. Owned by none of them, so
/// it lives here rather than under whichever driver happened to need it
/// first.
///
/// Gated on `internals` and `showcase`: the allocation gates in
/// `tests/alloc` clear against this tree, and the showcase carries it as a
/// page — the only way to look at the workload the numbers come from. It is
/// pure scene code with no harness dependency, so reaching it either way
/// costs nothing.
#[cfg(any(feature = "internals", feature = "showcase"))]
pub(crate) mod frame_fixture;
pub(crate) mod host;
pub(crate) mod icons;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod primitives;
pub(crate) mod renderer;
pub(crate) mod scene;
pub(crate) mod shape;
pub(crate) mod text;
pub(crate) mod ui;
pub(crate) mod widgets;
pub(crate) mod window;

/// Golden-image regression testing, for suites that draw through Palantir and
/// want to know when the drawing changes. Behind its own feature: it is the
/// only thing here that costs an image codec.
#[cfg(feature = "golden")]
pub mod golden;

/// Test reach-ins the supported surface deliberately excludes, gathered here
/// rather than scattered through it so the published API stays exactly the
/// list below. Each item is re-exported from the gated module that owns it —
/// which lives beside the code whose privates it exposes. Benchmark entry
/// points have their own gated facade in the `bench` module, behind that
/// feature.
///
/// The two bundled demo surfaces — [`FrameFixture`] and [`demo_swatches`] —
/// are gated at the crate root instead, because `showcase` builds them
/// without `internals` and so cannot see this module at all.
#[cfg(any(test, feature = "internals"))]
pub mod internals {
    pub use crate::app::internals::RecordApp;
    /// Needs a real GPU device, so unlike its neighbours this one exists
    /// only under the feature — never in a plain `cargo test` build.
    #[cfg(feature = "internals")]
    pub use crate::host::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
    #[cfg(feature = "internals")]
    pub use crate::text::internals::TEXT_SCALE_STEP;
    pub use crate::ui::harness::UiHarness;
}

/// GPU pass-timing + pipeline-statistics handles, refreshed each frame by
/// the backend (timestamp-query + pipeline-statistics readback).
/// Consumers (debug overlay, benches) hold a `Clone` of the same
/// `GpuPassStats` the backend writes into — no global state;
/// `OffscreenHost::gpu_pass_stats` is the canonical handle.
pub use diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats, PipelineStats};

/// The `wgpu` Palantir was built against.
///
/// Re-exported because Palantir's surface is not wgpu-free: [`GpuPaint`] hands
/// out a `Device` and a `CommandEncoder`, [`OffscreenHost`] is handed a
/// `Device` and a `Queue` and renders into a `Texture`. A consumer naming
/// those from its own `wgpu` dependency has to keep that dependency
/// semver-identical to this one by hand, and a mismatch turns every one of
/// those types foreign at the call site. Going through this one cannot skew.
pub use wgpu;

/// Format text straight into the frame's record store, with no `String` in
/// between.
///
/// `fmt!(ui, "…", args…)` is [`Ui::fmt`] over `format_args!` — the
/// allocation-free way to author a dynamic label. Widget text setters also
/// accept a `String`, so `format!` compiles and reads the same; this is the
/// form to reach for, because the bytes land directly in the arena the
/// widget was going to copy them into anyway.
///
/// ```
/// # use palantir::{Button, Configure, Text, Ui, fmt};
/// # fn demo(ui: &mut Ui, clicks: u32, total: usize) {
/// Text::new(fmt!(ui, "clicks: {clicks}")).show(ui);
/// Button::new().label(fmt!(ui, "{total} items")).show(ui);
/// # }
/// ```
///
/// The result is an [`InternedStr`] valid only for the pass that minted it —
/// hand it to a widget in the same breath, as above. See [`Ui::fmt`] for the
/// retention rules and [`Ui::intern`] for the format-less twin.
#[macro_export]
macro_rules! fmt {
    ($ui:expr, $($args:tt)*) => {
        $ui.fmt(::core::format_args!($($args)*))
    };
}

pub use animation::anim_slot::AnimSlot;
pub use animation::anim_spec::AnimSpec;
pub use animation::animatable::Animatable;
pub use animation::easing::Easing;
pub use app::App;
pub use diagnostics::DebugOverlayConfig;
pub use display::Display;
/// The benchmark workload as a recordable scene. Not part of the supported
/// surface — it exists so the bench target, the allocation gates and the
/// showcase page record the same tree, rather than each keeping a smaller
/// stand-in of its own.
#[cfg(any(feature = "internals", feature = "showcase"))]
pub use frame_fixture::FrameFixture;
/// The surface, scale and dpr the benchmark workload is timed at. The
/// bench target and the allocation gates share them so their numbers stay
/// comparable.
#[cfg(feature = "internals")]
pub use frame_fixture::{BENCH_DPR, BENCH_SCALE, BENCH_SURFACE};
pub use host::clock::{Clock, FixedClock, RealtimeClock};
/// What to ask an adapter for so the device it returns can run Palantir.
pub use host::device_requirements::DeviceRequirements;
pub use host::error::{GpuRequestError, UnmetRequirements};
/// An adapter and the device opened on it. `RequestedGpu::headless` is the
/// short way to one when there is no window to get it from.
pub use host::gpu_request::RequestedGpu;
/// The headless render-to-texture host — the offscreen peer of
/// [`WinitHost`]. Renders a `Ui` to a caller-supplied `wgpu::Texture`
/// instead of a swapchain (screenshots, thumbnails, server-side
/// compositing); also backs the visual harness + GPU benches.
pub use host::offscreen::{OffscreenHost, OffscreenHostBuilder};
#[cfg(feature = "winit")]
pub use host::winit::{
    WinitHost, WinitHostBuilder,
    config::WinitHostConfig,
    error::{HostDisconnected, WinitHostError},
    handle::{HostHandle, UserEvent},
};
pub use input::input_event::InputEvent;
pub use input::key_class::{KeyClass, KeyFilter};
pub use input::keyboard::key::Key;
pub use input::keyboard::key_press::KeyPress;
pub use input::keyboard::modifiers::Modifiers;
pub use input::pointer::{PointerButton, PointerEvent};
pub use input::policy::{FocusPolicy, InputPolicy};
pub use input::response::button_phase::ButtonPhase;
pub use input::response::button_state::ButtonState;
pub use input::response::drag::Drag;
pub use input::response::input_delta::InputDelta;
pub use input::response::pointer_action::PointerAction;
pub use input::response::pointer_edge::PointerEdge;
pub use input::response::response_state::ResponseState;
pub use input::response::scroll_delta::ScrollDelta;
pub use input::sense::Sense;
pub use input::shortcut::{Mods, Shortcut};
pub use input::watch::{KeyboardWake, PointerWake};
pub use layout::types::align::{Align, HAlign, VAlign};
pub use layout::types::clip_mode::ClipMode;
pub use layout::types::grid_cell::GridCell;
pub use layout::types::justify::Justify;
pub use layout::types::sizing::{Sizes, Sizing};
pub use layout::types::track::Track;
pub use palantir_anim_derive::Animatable;
pub use primitives::background::Background;
pub use primitives::brush::gradient::conic_geometry::{
    ConicGeometry, ConicGradient, ConicGradientBuilder,
};
pub use primitives::brush::gradient::gradient_builder::GradientBuilder;
pub use primitives::brush::gradient::linear_geometry::{
    LinearGeometry, LinearGradient, LinearGradientBuilder,
};
pub use primitives::brush::gradient::radial_geometry::{
    RadialGeometry, RadialGradient, RadialGradientBuilder,
};
pub use primitives::brush::gradient::stops::{GradientStops, Stop};
pub use primitives::brush::gradient::{Gradient, GradientGeometry, Interp, Spread};
pub use primitives::brush::{Brush, CurveBrush};
pub use primitives::color::Color;
pub use primitives::color::ColorU8;
pub use primitives::corners::Corners;
pub use primitives::image::{Image, ImageDownsample, ImageFilter, ImageFit};
pub use primitives::interned_str::InternedStr;
pub use primitives::mesh::{Mesh, MeshVertex};
pub use primitives::rect::Rect;
pub use primitives::shadow::Shadow;
pub use primitives::size::Size;
pub use primitives::spacing::{Spacing, Sums};
pub use primitives::text_input::TextInput;
pub use scene::layer::Layer;
pub use scene::node::Node;
pub use scene::node::configure::Configure;
pub use scene::node::configure::ConfigureNode;
pub use scene::visibility::Visibility;
// Re-exported (not an palantir type) because it's the canonical integer
// pixel-extent across the public surface — `Display.physical`,
// `Display::from_physical`, and `WindowConfig`'s sizes all speak `UVec2`
// (`.x` = width, `.y` = height). Saves consumers a direct `glam` dep.
pub use glam::UVec2;
// `Vec2` is in the public surface (Shape polyline points, `Configure::position`,
// `Canvas` placement); re-export so widget authors don't need a direct `glam` dep.
pub use glam::Vec2;
pub use icons::icon_set::{IconHandle, IconSet};
pub use icons::icon_table::{IconDef, IconId, IconTable};
pub use primitives::span::Span;
pub use primitives::stroke::Stroke;
pub use primitives::translate_scale::TranslateScale;
pub use primitives::widget_id::WidgetId;
pub use renderer::gpu_paint::GpuPaint;
pub use renderer::gpu_paint::gpu_frame_ctx::GpuFrameCtx;
pub use renderer::gpu_paint::gpu_init_ctx::GpuInitCtx;
pub use renderer::image_registry::ImageHandle;
pub use renderer::texture_limit::RegisterImageError;
/// The bound on [`Ui::add_shape`] — sealed, so it names the shape kinds
/// the crate ships and nothing else.
pub use shape::Lower;
pub use shape::Shape;
pub use shape::curve::CurveShape;
pub use shape::icon::{IconFit, IconShape};
pub use shape::image::ImageShape;
pub use shape::mesh::MeshShape;
pub use shape::polyline::{PolylineColors, PolylineShape};
pub use shape::rect::RectShape;
pub use shape::shadow::ShadowShape;
pub use shape::style::{LineCap, LineJoin};
pub use shape::text::TextShape;
pub use shape::triangle::TriangleShape;
// Shaping and rasterization for a caller that draws its own text — see
// [`TextShaper::glyphs`]. The atlas and the pipeline stay the caller's; what is
// shared is the font stack.
pub use renderer::backend::raster_atlas::content_type::ContentType;
pub use text::glyph_font::GlyphFont;
pub use text::glyphs::TextGlyphs;
pub use text::probe::Caret;
pub use text::probe::TextProbe;
pub use text::render::{GlyphImage, GlyphPlacement, GlyphRasterKey, PlacedGlyph};
pub use text::run::TextRun;
pub use text::shaper::TextShaper;
pub use text::wrap::TextWrap;
pub use text::{FontFamily, FontWeight};
pub use ui::Ui;
pub use ui::frame_report::{FramePaint, FrameReport};
pub use ui::layer_scope::LayerScope;
pub use widgets::button::Button;
pub use widgets::checkbox::Checkbox;
pub use widgets::close_handle::CloseHandle;
pub use widgets::combo_box::ComboBox;
pub use widgets::context_menu::ContextMenu;
pub use widgets::context_menu::menu_item::MenuItem;
pub use widgets::context_menu::menu_separator::MenuSeparator;
pub use widgets::drag_num::DragNum;
pub use widgets::drag_value::DragValue;
pub use widgets::frame::Frame;
pub use widgets::gpu_view::GpuView;
pub use widgets::grid::Grid;
pub use widgets::modal::Modal;
pub use widgets::overlay_response::OverlayResponse;
pub use widgets::panel::Panel;
pub use widgets::popup::{ClickOutside, Popup};
pub use widgets::progress_bar::ProgressBar;
pub use widgets::radio::RadioButton;
pub use widgets::response::{InnerResponse, Response, ResponseSnapshot};
pub use widgets::scroll::Scroll;
pub use widgets::scroll::bars::BarMode;
pub use widgets::scroll::zoom_config::{ZoomConfig, ZoomModifier, ZoomPivot};
pub use widgets::select_response::SelectResponse;
pub use widgets::separator::Separator;
pub use widgets::slider::Slider;
pub use widgets::spinner::Spinner;
pub use widgets::splitter::{SplitHalf, Splitter};
pub use widgets::switch::Switch;
pub use widgets::text::Text;
pub use widgets::text_edit::{TextEdit, TextEditResponse};
pub use widgets::theme::Theme;
pub use widgets::theme::button::ButtonTheme;
pub use widgets::theme::combo_box::ComboBoxTheme;
pub use widgets::theme::context_menu::ContextMenuTheme;
pub use widgets::theme::context_menu::menu_item::MenuItemTheme;
pub use widgets::theme::drag_value::DragValueTheme;
pub use widgets::theme::modal::ModalTheme;
pub use widgets::theme::palette::Palette;
pub use widgets::theme::progress_bar::ProgressBarTheme;
pub use widgets::theme::scrollbar::ScrollbarTheme;
pub use widgets::theme::separator::SeparatorTheme;
pub use widgets::theme::slider::SliderTheme;
pub use widgets::theme::spinner::SpinnerTheme;
pub use widgets::theme::splitter::SplitterTheme;
pub use widgets::theme::text_edit::TextEditTheme;
pub use widgets::theme::text_style::TextStyle;
pub use widgets::theme::toggle::ToggleTheme;
pub use widgets::theme::tooltip::TooltipTheme;
pub use widgets::theme::widget_look::WidgetLook;
pub use widgets::theme::widget_look::animated_look::AnimatedLook;
pub use widgets::theme::widget_look::stateful_look::StatefulLook;
pub use widgets::theme::widget_look::theme_slot::SlotDefaults;
pub use widgets::tooltip::Tooltip;
pub use widgets::value_response::ValueResponse;
pub use widgets::widget::Widget;
pub use window::cursor_icon::CursorIcon;
pub use window::vsync::Vsync;
pub use window::window_config::WindowConfig;
pub use window::window_geometry::WindowGeometry;
pub use window::window_placement::WindowPlacement;
pub use window::window_token::WindowToken;

#[cfg(test)]
mod hot_struct_sizes;
