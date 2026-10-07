#![cfg_attr(
    feature = "winit",
    expect(
        rustdoc::bare_urls,
        reason = "the README's showcase recording is a bare GitHub attachment URL, the only form GitHub expands into an inline player"
    )
)]
// Scoped to the library: a `[lints]` table would reach the examples too.
#![warn(missing_docs)]
// The README's counter example builds a `WinitHost`, so it is a doctest only
// with that feature.
#![cfg_attr(feature = "winit", doc = include_str!("../README.md"))]
// Winit-host types stay linked from backend-agnostic docs, so those links go
// unresolved without the feature; other broken links still show in the default build.
#![cfg_attr(
    not(feature = "winit"),
    expect(
        rustdoc::broken_intra_doc_links,
        reason = "the winit-only docs stay whole in this build, so their links go unresolved"
    )
)]
//!
//! # Where to start
//!
//! - [`prelude`] is the one import an application screen needs.
//! - [`App`] is the lifecycle trait; its [`record`](App::record) runs every frame
//!   and describes the whole UI from scratch.
//! - [`WinitHost`] owns the event loop, windows and GPU device; [`OffscreenHost`]
//!   renders into a `wgpu::Texture` you supply.
//! - [`Ui`] is the recorder handed to `record`; [`Ui::layer`] switches which
//!   [`Layer`] arena (main / popup / modal / tooltip / debug) receives records.
//! - Widgets are builders ended by `show(ui)`: [`Button`], [`Text`], [`TextEdit`],
//!   [`Slider`], [`Checkbox`], [`ComboBox`], [`Scroll`], [`Popup`], [`Modal`] and
//!   more. Containers are [`Panel`] (h/v/z-stack and canvas) and [`Grid`].
//! - [`Configure`] carries the settings every node shares (identity, size,
//!   padding, margin, alignment, visibility). It is a trait: it must be in scope,
//!   which the [`prelude`] does.
//! - [`Theme`] is the one serializable style tree.
//! - [`GpuView`] hands a widget-sized `wgpu` render target to your [`GpuPaint`].
//! - [`widget`] is the other half of the surface: what a widget of your own is
//!   built from.
//!
//! # Sizing
//!
//! Layout is a WPF-style two pass (measure, then arrange); [`Sizing`] is its
//! vocabulary:
//!
//! - [`Sizing::fixed`] is an exact extent, allowed to exceed the parent.
//! - [`Sizing::HUG`] is `min(content, available)`, floored at the largest
//!   non-shrinkable thing inside (a fixed descendant, an explicit minimum, the
//!   longest unbreakable word).
//! - [`Sizing::fill`] takes the leftover, split by weight; a sibling whose floor
//!   exceeds its share freezes at the floor and the rest re-divide.
//!
//! Children clamp to their parent; a parent never grows to fit a child.
//!
//! # Styling
//!
//! Three doors, widest first.
//!
//! - **[`Theme`]** is the whole style tree ([`Ui::set_theme`](Ui::set_theme)
//!   swaps it), with per-widget bundles ([`Theme::button`], [`Theme::slider`]).
//! - **`style(&…Theme)`** overrides the bundle for one call site; it takes an
//!   `Option`, so `.style(overrides.as_ref())` works.
//! - **A per-axis setter** ([`Text::color`], [`Separator::thickness`],
//!   [`Spinner::diameter`], [`Modal::backdrop`]) overrides one field.
//!
//! The third door exists only where an axis has one meaning: [`Separator`] draws
//! one rule, but [`Slider`] has a track, fill and knob and [`Button`] a colour per
//! state, so those take the bundle. [`Background`] follows the same rule: widgets
//! with one panel behind them have `background()` ([`Panel`], [`Grid`], [`Block`],
//! [`Scroll`], [`Popup`], [`Modal`], [`Tooltip`], [`ContextMenu`]).
//!
//! # Feature flags
//!
//! | flag | default | what it does |
//! | --- | --- | --- |
//! | `winit` | yes | The winit-backed [`WinitHost`]; without it only [`OffscreenHost`] exists. Implies `system-clipboard`. |
//! | `system-clipboard` | via `winit` | Backs [`Clipboard`] with the OS clipboard; [`OffscreenHost`] opts in via [`OffscreenHostBuilder::system_clipboard`]. Otherwise an in-process buffer. |
//! | `gpu-debug-markers` | no | GPU debug groups around every draw step for RenderDoc / Xcode captures; costs two commands and a label copy per step. |
//! | `profile-with-tracy` | no | A Tracy zone per frame pass and a frame set per window. Needs the Tracy viewer. |
//! | `internals` | no | The `internals` module (frame harness, fixtures, headless test GPU). **Not a supported API.** |
//! | `bench` | no | Benchmark drivers and the facade `benches/` calls. Implies `internals`. Not a supported API. |
//! | `golden` | no | The `golden` module for golden-image tests of suites that draw through Palantir; costs an image codec. |
//!
//! # Colour
//!
//! [`RgbaF32`] holds **straight-alpha linear RGB**. The constructors
//! ([`RgbaF32::srgb`], [`RgbaF32::hex`], [`RgbaF32::from_srgba`]) read sRGB and
//! linearise; [`RgbaF32::new`] takes values already linear. Blending, anti-aliasing
//! and animation run in linear, and the sRGB encode happens on the GPU at swapchain
//! write, so sRGB values put into [`RgbaF32`] directly come out wrong.

// Self-alias so derive-emitted `::palantir::widget::Animatable` paths resolve
// inside the crate.

extern crate self as palantir;

// A macro about one subsystem is declared at the top of the `mod.rs` that owns it:
// `macro_rules!` without `#[macro_export]` is textually scoped to that subtree.
// `common::flag_set` is the exception: `#[macro_use]` and declared first.

#[macro_use]
pub(crate) mod common;

pub(crate) mod animation;
pub(crate) mod app;
#[cfg(feature = "bench")]
pub mod bench;
pub(crate) mod cascade;
pub(crate) mod damage;
pub(crate) mod diagnostics;
/// Per-output display state shared by host and renderer.
pub(crate) mod display;
pub(crate) mod gpu;
pub(crate) mod host;
pub(crate) mod icons;
pub(crate) mod input;
/// Everything that does not ship. **Not a supported API.**
#[cfg(any(test, feature = "internals"))]
pub mod internals;
pub(crate) mod layout;
pub(crate) mod primitives;
pub(crate) mod renderer;
pub(crate) mod scene;
pub(crate) mod shape;
pub(crate) mod text;
pub(crate) mod ui;
pub(crate) mod widget_core;
pub(crate) mod widgets;
pub(crate) mod window;

/// Golden-image regression testing for suites that draw through Palantir.
#[cfg(feature = "golden")]
pub mod golden;

/// GPU pass-timing and pipeline-statistics handles; clones share the backend's
/// `GpuPassStats`.
pub use diagnostics::gpu_pass_stats::{BatchKind, GpuPassStats, PipelineStats};

/// The `wgpu` Palantir was built against. Re-exported because the surface is not
/// wgpu-free: [`GpuPaint`] hands out a `Device` and `CommandEncoder`, [`Gpu`] is
/// built from a `Device` and `Queue`, and [`RenderTarget`] borrows a `Texture`. A
/// consumer's own `wgpu` must match this version exactly or these types go foreign.
pub use wgpu;

/// Format text straight into the frame's record store with no `String`:
/// `fmt!(ui, "…", args…)` is [`Ui::fmt`] over `format_args!`.
///
/// ```
/// # use palantir::{Button, Configure, Text, Ui, fmt};
/// # fn demo(ui: &mut Ui, clicks: u32, total: usize) {
/// Text::new(fmt!(ui, "clicks: {clicks}")).show(ui);
/// Button::new().label(fmt!(ui, "{total} items")).show(ui);
/// # }
/// ```
///
/// The result is an [`InternedStr`] valid only for the pass that minted it. See
/// [`Ui::fmt`] for retention and [`Ui::intern`] for the format-less twin.
#[macro_export]
macro_rules! fmt {
    ($ui:expr, $($args:tt)*) => {
        $ui.fmt(::core::format_args!($($args)*))
    };
}

/// What an application screen types, in one import. Includes [`Configure`], a
/// trait, without which `Button::new().size(..)` does not compile.
///
/// ```
/// use palantir::prelude::*;
///
/// fn screen(ui: &mut Ui) {
///     Panel::vstack().gap(8.0).show(ui, |ui| {
///         Text::new("hello").show(ui);
///         Button::new().label("go").show(ui);
///     });
/// }
/// ```
///
/// Not the whole crate: theme bundles, the docking model, hosts, colour models and
/// gradient builders stay at the root; widget authoring stays in [`widget`].
pub mod prelude {
    pub use crate::{
        Align, App, Axis, Background, Block, Brush, Button, Checkbox, ComboBox, Configure,
        ContextMenu, Corners, DragValue, Expander, Grid, GridCell, HAlign, InnerResponse, Justify,
        Key, KeyPress, MenuItem, Modal, Modifiers, OverlayResponse, Panel, PointerButton, Popup,
        ProgressBar, RadioButton, Rect, Response, RgbaF32, Scroll, Sense, Separator, Shadow,
        Shortcut, Size, SizeSpec, Sizing, Slider, Spacing, Spinner, Splitter, Stroke, Switch,
        TabbedView, Text, TextEdit, TextStyle, Theme, Tooltip, Track, UVec2, Ui, VAlign,
        ValueResponse, Vec2, WidgetId, WindowToken, fmt,
    };
}

/// Authoring a widget: the node, paint primitives, and text and animation plumbing
/// a widget of your own is built from. Nothing here is needed to compose shipped
/// widgets; every widget in this crate uses only this module plus the root (see
/// `examples/showcase/pages/custom_widget.rs`).
pub mod widget {
    pub use crate::animation::animatable::Animatable;
    pub use crate::animation::animation_slot::AnimationSlot;
    pub use crate::common::span::Span;
    pub use crate::primitives::geometry::mesh::{Mesh, MeshVertex};
    pub use crate::primitives::math::domain;
    pub use crate::primitives::paint::content_type::ContentType;
    pub use crate::primitives::paint::raster_image::RasterImage;
    /// The paint-time animation curves the crate ships; a custom curve is any
    /// `fn(f32) -> f32`, see [`PaintCurve`].
    pub use crate::scene::tree::paint_anims::curves;
    pub use crate::scene::tree::paint_anims::paint_animation::{
        PaintAnimation, PaintChannel, PaintCurve, PaintRepeat, PaintSteps, PaintTiming,
    };
    /// The bound on [`Ui::add_shape`](crate::Ui::add_shape); sealed.
    pub use crate::shape::Lower;
    pub use crate::shape::Shape;
    pub use crate::shape::curve::CurveShape;
    pub use crate::shape::icon::{IconFit, IconShape};
    pub use crate::shape::image::ImageShape;
    pub use crate::shape::mesh::MeshShape;
    pub use crate::shape::polyline::PolylineShape;
    pub use crate::shape::rect::RectShape;
    pub use crate::shape::shadow::ShadowShape;
    pub use crate::shape::style::{LineCap, LineJoin};
    pub use crate::shape::text::TextShape;
    pub use crate::shape::triangle::TriangleShape;
    pub use crate::text::glyph_font::GlyphFont;
    pub use crate::text::glyphs::TextGlyphs;
    pub use crate::text::probe::Caret;
    pub use crate::text::probe::TextProbe;
    pub use crate::text::render::{GlyphRasterKey, PlacedGlyph};
    pub use crate::text::run::TextRun;
    pub use crate::widget_core::configure::ConfigureWidget;
    pub use crate::widget_core::configure::ThemeDefaults;
    pub use crate::widget_core::widget::Widget;
    pub use crate::widget_core::widget_look::look_plan::LookPlan;
    pub use crate::widget_core::widget_look::theme_slot::ThemeSlot;
    pub use palantir_anim_derive::Animatable;
}

pub use animation::animation_spec::AnimationSpec;
pub use animation::easing::Easing;
pub use app::App;
pub use common::clipboard::{Clipboard, ClipboardUnavailable};
pub use common::platform::{PLATFORM, Platform};
pub use diagnostics::DebugOverlayConfig;
pub use display::Display;
/// The application's scale factor, multiplied onto the platform's. Written through
/// [`Ui::set_user_scale`](crate::Ui::set_user_scale).
pub use display::user_scale::UserScale;
pub use gpu::device::device_requirements::DeviceRequirements;
pub use gpu::device::power_preference::PowerPreference;
pub use gpu::device::requested_gpu::{Gpu, RequestedGpu};
#[cfg(feature = "winit")]
pub use gpu::error::SurfaceError;
pub use gpu::error::{DriverError, GpuRequestError, UnmetRequirements};
pub use gpu::surface::render_target::{RenderTarget, TargetFormat};
pub use host::clock::{Clock, FixedClock, RealtimeClock};
/// The headless render-to-texture host: renders a `Ui` to a caller-supplied
/// `wgpu::Texture` (screenshots, thumbnails). Also backs the visual harness.
pub use host::offscreen::{OffscreenHost, OffscreenHostBuilder};
#[cfg(feature = "winit")]
pub use host::winit::{
    WinitHost, WinitHostBuilder,
    error::{HostDisconnected, WinitHostError},
    handle::HostHandle,
};
/// The event a host feeds a `Ui`; toolkit-independent (see
/// [`OffscreenHost::on_input`]).
pub use input::ime_preedit::ImePreedit;
pub use input::input_event::InputEvent;
pub use input::interaction::button_phase::ButtonPhase;
pub use input::interaction::button_state::ButtonState;
pub use input::interaction::drag::Drag;
/// The verdict [`OffscreenHost::on_input`] reads back: whether an event asks for a
/// repaint.
pub use input::interaction::input_delta::InputDelta;
pub use input::interaction::pointer_action::PointerAction;
pub use input::interaction::pointer_edge::PointerEdge;
pub use input::interaction::response_state::ResponseState;
pub use input::interaction::scroll_delta::ScrollDelta;
pub use input::key_class::{KeyClass, KeyFilter};
pub use input::keyboard::key::Key;
pub use input::keyboard::key_press::KeyPress;
pub use input::keyboard::key_text::KeyText;
pub use input::keyboard::modifiers::Modifiers;
pub use input::pointer::{PointerButton, PointerEvent};
pub use input::policy::{FocusPolicy, InputPolicy};
pub use input::sense::Sense;
pub use input::shortcut::{Shortcut, ShortcutMods};
pub use input::watch::{KeyboardWake, PointerWake};
pub use input::zoom_factor::ZoomFactor;
pub use primitives::geometry::corners::Corners;
pub use primitives::geometry::rect::Rect;
pub use primitives::geometry::size::Size;
pub use primitives::geometry::spacing::Spacing;
pub use primitives::layout::align::{Align, HAlign, VAlign};
pub use primitives::layout::anchor::{Anchor, AnchorAlign};
pub use primitives::layout::axis::Axis;
pub use primitives::layout::clip_mode::ClipMode;
pub use primitives::layout::grid_cell::GridCell;
pub use primitives::layout::justify::Justify;
pub use primitives::layout::sizing::{SizeSpec, Sizing};
pub use primitives::layout::track::Track;
pub use primitives::layout::visibility::Visibility;
pub use primitives::paint::background::Background;
pub use primitives::paint::brush::Brush;
pub use primitives::paint::brush::gradient::color_ramp::ColorRamp;
pub use primitives::paint::brush::gradient::conic_geometry::{
    ConicGeometry, ConicGradient, ConicGradientBuilder,
};
pub use primitives::paint::brush::gradient::gradient_builder::GradientBuilder;
pub use primitives::paint::brush::gradient::linear_geometry::{
    LinearGeometry, LinearGradient, LinearGradientBuilder,
};
pub use primitives::paint::brush::gradient::radial_geometry::{
    RadialGeometry, RadialGradient, RadialGradientBuilder,
};
pub use primitives::paint::brush::gradient::stops::{GradientStops, Stop};
pub use primitives::paint::brush::gradient::{Gradient, GradientGeometry, Interpolation, Spread};
pub use primitives::paint::color::RgbaF32;
pub use primitives::paint::color::color_coords::ColorCoords;
pub use primitives::paint::color::color_model::{ColorModel, HueSlice};
pub use primitives::paint::color::hsv::Hsv;
pub use primitives::paint::color::okhsv::Okhsv;
pub use primitives::paint::color::srgba_u8::SrgbaU8;
pub use primitives::paint::image::error::ImageDataError;
pub use primitives::paint::image::{Image, ImageDownsample, ImageFilter, ImageFit};
pub use primitives::paint::shadow::Shadow;
pub use primitives::text::interned_str::InternedStr;
pub use primitives::text::text_input::TextInput;
pub use scene::layer::Layer;
// Signed screen coordinates; re-exported so consumers need no matching `glam`.
pub use glam::IVec2;
// Integer pixel extent (`Display.physical`, `WindowConfig` sizes); `.x` is width.
pub use glam::UVec2;
// Used by polyline points, `Configure::position` and `Canvas` placement.
pub use glam::Vec2;
pub use gpu::device::gpu_frame_context::GpuFrameContext;
pub use gpu::device::gpu_init_context::GpuInitContext;
pub use icons::error::IconTableError;
pub use icons::icon_set::{IconHandle, IconSet};
pub use icons::icon_table::{IconDefinition, IconId, IconTable};
pub use primitives::geometry::translate_scale::TranslateScale;
pub use primitives::identity::widget_id::WidgetId;
pub use primitives::paint::stroke::Stroke;
pub use renderer::error::ImageTooLarge;
pub use renderer::gpu_paint::GpuPaint;
pub use renderer::image_registry::image_handle::ImageHandle;
pub use text::error::FontLoadError;
pub use text::font_family::FontFamily;
pub use text::font_scope::FontScope;
pub use text::font_slant::FontSlant;
pub use text::font_source::FontSource;
pub use text::font_weight::FontWeight;
pub use text::shaper::TextShaper;
pub use text::wrap::TextWrap;
pub use ui::Ui;
pub use ui::frame_report::{FramePaint, FrameReport};
pub use ui::layer_scope::LayerScope;
pub use widget_core::configure::Configure;
pub use widget_core::overlay_response::OverlayResponse;
pub use widget_core::response::{InnerResponse, Response, ResponseSnapshot};
pub use widget_core::value_response::ValueResponse;
pub use widget_core::widget_look::WidgetLook;
pub use widget_core::widget_look::animated_look::AnimatedLook;
pub use widget_core::widget_look::stateful_look::StatefulLook;
pub use widget_core::widget_look::theme_slot::SlotDefaults;
pub use widgets::block::Block;
pub use widgets::button::Button;
pub use widgets::checkbox::Checkbox;
pub use widgets::close_handle::CloseHandle;
pub use widgets::color_button::ColorButton;
pub use widgets::color_field::ColorField;
pub use widgets::color_picker::ColorPicker;
pub use widgets::color_strip::ColorStrip;
pub use widgets::color_swatch::ColorSwatch;
pub use widgets::combo_box::ComboBox;
pub use widgets::context_menu::ContextMenu;
pub use widgets::context_menu::menu_item::MenuItem;
pub use widgets::context_menu::menu_separator::MenuSeparator;
pub use widgets::dock::allowed_splits::AllowedSplits;
pub use widgets::dock::dock_node::{DockNode, DockSplit, NodeIndex};
pub use widgets::dock::dock_operation::{DockDrop, DockOperation};
pub use widgets::dock::dock_path::DockPath;
pub use widgets::dock::dock_state::{DockState, TabAddress};
pub use widgets::dock::dock_tab::DockTab;
pub use widgets::dock::dock_tabs::{DockTabMenu, DockTabs};
pub use widgets::dock::dock_view::DockView;
pub use widgets::dock::error::DockError;
pub use widgets::dock::split_side::{SplitDirection, SplitSide};
pub use widgets::dock::tab_group::{TabGroup, TabGroupId};
pub use widgets::drag_num::DragNum;
pub use widgets::drag_value::DragValue;
pub use widgets::expander::Expander;
pub use widgets::expander::ExpanderResponse;
pub use widgets::gpu_view::GpuView;
pub use widgets::grid::Grid;
pub use widgets::modal::Modal;
pub use widgets::panel::Panel;
pub use widgets::popup::Popup;
pub use widgets::popup::click_outside::ClickOutside;
pub use widgets::popup::popup_trigger::PopupTrigger;
pub use widgets::progress_bar::ProgressBar;
pub use widgets::radio::RadioButton;
pub use widgets::scroll::Scroll;
pub use widgets::scroll::bars::BarMode;
pub use widgets::scroll::zoom_config::{ZoomConfig, ZoomModifier, ZoomPivot};
pub use widgets::separator::Separator;
pub use widgets::slider::Slider;
pub use widgets::spinner::Spinner;
pub use widgets::splitter::Splitter;
pub use widgets::splitter::split_half::SplitHalf;
pub use widgets::switch::Switch;
pub use widgets::tabs::tab_item::{TabBadge, TabItem};
pub use widgets::tabs::tab_strip::{TabOverflow, TabStrip, TabStripResponse};
pub use widgets::tabs::tabbed_view::{TabbedView, TabbedViewResponse, TabsAction};
pub use widgets::text::Text;
pub use widgets::text_edit::{TextEdit, TextEditResponse};
pub use widgets::theme::Theme;
pub use widgets::theme::button::ButtonTheme;
pub use widgets::theme::color_picker::ColorPickerTheme;
pub use widgets::theme::combo_box::ComboBoxTheme;
pub use widgets::theme::context_menu::ContextMenuTheme;
pub use widgets::theme::context_menu::menu_item::MenuItemTheme;
pub use widgets::theme::dock::DockTheme;
pub use widgets::theme::drag_value::DragValueTheme;
pub use widgets::theme::expander::ExpanderTheme;
pub use widgets::theme::focus_ring::FocusRingTheme;
pub use widgets::theme::modal::ModalTheme;
pub use widgets::theme::palette::Palette;
pub use widgets::theme::progress_bar::ProgressBarTheme;
pub use widgets::theme::scrollbar::ScrollbarTheme;
pub use widgets::theme::separator::SeparatorTheme;
pub use widgets::theme::slider::SliderTheme;
pub use widgets::theme::spinner::SpinnerTheme;
pub use widgets::theme::splitter::SplitterTheme;
pub use widgets::theme::tabs::TabsTheme;
pub use widgets::theme::text_edit::TextEditTheme;
pub use widgets::theme::text_style::{TextStyle, TextStyleOverrides};
pub use widgets::theme::toggle::ToggleTheme;
pub use widgets::theme::tooltip::TooltipTheme;
pub use widgets::tooltip::Tooltip;
pub use widgets::tooltip::TooltipResponse;
pub use window::cursor_icon::CursorIcon;
pub use window::vsync::Vsync;
pub use window::window_config::WindowConfig;
pub use window::window_geometry::WindowGeometry;
pub use window::window_placement::WindowPlacement;
pub use window::window_token::WindowToken;
