// The README's showcase recording is a bare GitHub attachment URL, which is
// the only form GitHub expands into an inline video player — wrapping it for
// rustdoc's sake would turn it back into a dead link on the repo page.
#![allow(rustdoc::bare_urls)]
// The README's counter example builds a `WinitHost`, so it only compiles as a
// doctest when that feature is on. Without it the crate docs open at the
// orientation section below instead.
#![cfg_attr(feature = "winit-host", doc = include_str!("../README.md"))]
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
//! | `winit-host` | yes | The winit-backed [`WinitHost`] — real windows, real event loop. Without it only [`OffscreenHost`] exists. |
//! | `system-clipboard` | yes | Routes [`TextEdit`] cut/copy/paste through the OS clipboard. Without it clipboard traffic stays in an in-process buffer. |
//! | `showcase` | no | Builds the bundled `showcase` binary, a tour of every widget. |
//! | `gpu-debug-markers` | no | Emits GPU debug groups around every draw step for RenderDoc / Xcode captures. Costs two recorded commands and a label copy per step even with no capture tool attached, so it is off unless you intend to capture. |
//! | `profile-with-tracy` | no | Routes the crate's profiling spans to a Tracy client, and marks a frame set per window. Needs the external Tracy viewer. |
//! | `internals` | no | Test and benchmark reach-ins — adds the `internals` and `bench` modules. **Not a supported API**: it exists so the integration tests and benches under `tests/` and `benches/` can reach crate privates, and it breaks without notice. |
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

pub(crate) mod animation;
pub(crate) mod app;
#[cfg(feature = "bench")]
pub mod bench;
pub(crate) mod common;
/// Accent swatches shared by the two bundled demo surfaces. Public only
/// because the `showcase` binary is a separate crate from this library
/// and cannot reach a `pub(crate)` one; not part of the supported API.
#[cfg(any(feature = "bench", feature = "showcase"))]
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
/// Gated on `showcase` as well as `bench` because the showcase carries it as
/// a page — the only way to look at the workload the numbers come from. It is
/// pure scene code with no harness dependency, so reaching it that way costs
/// the showcase nothing.
#[cfg(any(feature = "bench", feature = "showcase"))]
pub(crate) mod frame_fixture;
pub(crate) mod host;
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

/// Test reach-ins the supported surface deliberately excludes, gathered here
/// rather than scattered through it so the published API stays exactly the
/// list below. Everything in scope is the root re-export of a colocated
/// crate-private `internals` module — those live beside the code whose
/// privates they expose, and this is only the door out of the crate for
/// integration tests. Benchmark entry points have their own gated facade in
/// [`mod@bench`].
/// Golden-image regression testing, for suites that draw through Palantir and
/// want to know when the drawing changes. Behind its own feature: it is the
/// only thing here that costs an image codec.
#[cfg(feature = "golden")]
pub mod golden;

#[cfg(any(test, feature = "internals"))]
pub mod internals {
    pub use crate::app::internals::RecordApp;
    /// Needs a real GPU device, so unlike its neighbours this one exists
    /// only under the feature — never in a plain `cargo test` build.
    #[cfg(feature = "internals")]
    pub use crate::host::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
    pub use crate::ui::harness::UiHarness;
}

/// GPU pass-timing + pipeline-statistics handles, refreshed each frame by
/// the backend (timestamp-query + pipeline-statistics readback).
/// Consumers (debug overlay, benches) hold a `Clone` of the same
/// `GpuPassStats` the backend writes into — no global state;
/// `OffscreenHost::gpu_pass_stats` is the canonical handle.
pub use diagnostics::gpu_stats::{BatchKind, GpuPassStats, PipelineStats};

/// The `wgpu` Palantir was built against.
///
/// Re-exported because Palantir's surface is not wgpu-free: [`GpuPaint`] hands
/// out a `Device` and a `CommandEncoder`, [`OffscreenHost`] is handed a
/// `Device` and a `Queue` and renders into a `Texture`. A consumer naming
/// those from its own `wgpu` dependency has to keep that dependency
/// semver-identical to this one by hand, and a mismatch turns every one of
/// those types foreign at the call site. Going through this one cannot skew.
pub use wgpu;

pub use animation::animatable::Animatable;
pub use animation::easing::Easing;
pub use animation::{AnimSlot, AnimSpec};
pub use app::App;
// Same-name re-export: the derive lives in the macro namespace,
// the trait in the type namespace — `use palantir::Animatable;` pulls
// both, and `#[derive(Animatable)]` works alongside `T: Animatable`.
pub use diagnostics::DebugOverlayConfig;
pub use display::Display;
/// The benchmark workload as a recordable scene. Not part of the supported
/// surface — it exists so the `bench` targets and the showcase page can
/// record the same tree.
#[cfg(any(feature = "bench", feature = "showcase"))]
pub use frame_fixture::FrameFixture;
pub use host::clock::{Clock, FixedClock, RealtimeClock};
/// What to ask an adapter for so the device it returns can run Palantir.
pub use host::device_requirements::DeviceRequirements;
pub use host::error::{HeadlessGpuError, UnmetRequirements};
/// The short way to a usable device when there is no window to get one from.
pub use host::headless_gpu::HeadlessGpu;
/// The headless render-to-texture host — the offscreen peer of
/// [`WinitHost`]. Renders a `Ui` to a caller-supplied `wgpu::Texture`
/// instead of a swapchain (screenshots, thumbnails, server-side
/// compositing); also backs the visual harness + GPU benches.
pub use host::offscreen::{OffscreenHost, OffscreenHostBuilder};
#[cfg(feature = "winit-host")]
pub use host::winit::{
    WinitHost, WinitHostBuilder,
    config::WinitHostConfig,
    error::{HostDisconnected, WinitHostError},
    handle::{HostHandle, UserEvent},
};
pub use input::InputEvent;
pub use input::key_class::{KeyClass, KeyFilter};
pub use input::keyboard::{Key, KeyPress, KeyboardEvent, Modifiers, TextChunk};
pub use input::pointer::{PointerButton, PointerEvent};
pub use input::policy::{FocusPolicy, InputPolicy};
pub use input::response::{ButtonPhase, ButtonState, Drag, InputDelta, ResponseState, ScrollDelta};
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
pub use primitives::brush::gradient::conic::{ConicGradient, ConicGradientBuilder};
pub use primitives::brush::gradient::linear::{LinearGradient, LinearGradientBuilder};
pub use primitives::brush::gradient::radial::{RadialGradient, RadialGradientBuilder};
pub use primitives::brush::gradient::stops::{GradientStops, Stop};
pub use primitives::brush::gradient::{Interp, Spread};
pub use primitives::brush::{Brush, CurveBrush};
pub use primitives::color::Color;
pub use primitives::color::ColorU8;
pub use primitives::corners::Corners;
pub use primitives::image::{Image, ImageDownsample, ImageFilter, ImageFit};
pub use primitives::interned_str::{InternedStr, TextInput};
pub use primitives::mesh::{Mesh, MeshVertex};
pub use primitives::rect::Rect;
pub use primitives::shadow::Shadow;
pub use primitives::size::Size;
pub use primitives::spacing::{Spacing, Sums};
pub use scene::layer::Layer;
pub use scene::node::{Configure, ConfigureNode, Node};
pub use scene::visibility::Visibility;
// Re-exported (not an palantir type) because it's the canonical integer
// pixel-extent across the public surface — `Display.physical`,
// `Display::from_physical`, and `WindowConfig`'s sizes all speak `UVec2`
// (`.x` = width, `.y` = height). Saves consumers a direct `glam` dep.
pub use glam::UVec2;
// `Vec2` is in the public surface (Shape polyline points, `Configure::position`,
// `Canvas` placement); re-export so widget authors don't need a direct `glam` dep.
pub use glam::Vec2;
pub use primitives::span::Span;
pub use primitives::stroke::Stroke;
pub use primitives::transform::TranslateScale;
pub use primitives::widget_id::WidgetId;
pub use renderer::gpu_view::{GpuFrameCtx, GpuInitCtx, GpuPaint};
pub use renderer::image_registry::{ImageHandle, RegisterImageError};
/// The bound on [`Ui::add_shape`] — sealed, so it names the shape kinds
/// the crate ships and nothing else.
pub use shape::Lower;
pub use shape::Shape;
pub use shape::curve::CurveShape;
pub use shape::image::ImageShape;
pub use shape::mesh::MeshShape;
pub use shape::polyline::{PolylineColors, PolylineShape};
pub use shape::rect::RectShape;
pub use shape::shadow::ShadowShape;
pub use shape::style::{LineCap, LineJoin};
pub use shape::text::TextShape;
pub use shape::triangle::TriangleShape;
pub use text::probe::TextProbe;
pub use text::probe::layout::Caret;
pub use text::run::TextRun;
pub use text::shaper::TextShaper;
pub use text::wrap::TextWrap;
pub use text::{FontFamily, FontWeight};
pub use ui::Ui;
pub use ui::frame_report::{FramePaint, FrameProcessing, FrameReport};
pub use ui::layer_scope::LayerScope;
pub use widgets::button::Button;
pub use widgets::checkbox::Checkbox;
pub use widgets::combo_box::ComboBox;
pub use widgets::context_menu::{ContextMenu, ContextMenuResponse, MenuItem, MenuSeparator};
pub use widgets::drag_value::{DragNum, DragValue, DragValueResponse};
pub use widgets::frame::Frame;
pub use widgets::gpu_view::GpuView;
pub use widgets::grid::Grid;
pub use widgets::modal::{Modal, ModalResponse};
pub use widgets::panel::Panel;
pub use widgets::popup::{ClickOutside, Popup, PopupHandle, PopupResponse};
pub use widgets::progress_bar::ProgressBar;
pub use widgets::radio::RadioButton;
pub use widgets::response::{InnerResponse, Response, ResponseSnapshot};
pub use widgets::scroll::Scroll;
pub use widgets::scroll::bars::BarMode;
pub use widgets::scroll::zoom_config::{ZoomConfig, ZoomModifier, ZoomPivot};
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
pub use widgets::tooltip::Tooltip;
pub use widgets::widget::Widget;
pub use window::{CursorIcon, Vsync, WindowConfig, WindowGeometry, WindowToken};

#[cfg(test)]
mod hot_struct_sizes {
    use crate::animation::AnimRow;
    use crate::common::content_hash::ContentHash;
    use crate::input::{TargetScrollDelta, response::ResponseState};
    use crate::layout::ShapedText;
    use crate::primitives::background::Background;
    use crate::primitives::brush::Brush;
    use crate::primitives::interned_str::RecordedText;
    use crate::primitives::mesh::MeshVertex;
    use crate::primitives::span::Span;
    use crate::renderer::backend::text::GlyphInstance;
    use crate::renderer::frontend::payload::{
        DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload, DrawQuadPayload,
        DrawTextPayload, PushClipPayload, ResolvedGradient,
    };
    use crate::renderer::quad::Quad;
    use crate::renderer::render_buffer::curve::CurveInstance;
    use crate::renderer::render_buffer::image::ImageInstance;
    use crate::renderer::render_buffer::mesh::MeshInstance;
    use crate::renderer::render_buffer::text::TextDrawRow;
    use crate::scene::cascade::CascadeInputHash;
    use crate::scene::cascade::entry::{EntryRow, HitRow};
    use crate::scene::cascade::paint::Paint;
    use crate::scene::damage::region::DamageRegion;
    use crate::scene::damage::snapshot::NodeSnapshot;
    use crate::scene::node::Node;
    use crate::scene::node::columns::{BoundsExtras, LayoutCore, NodeFlags, PanelExtras};
    use crate::scene::record_store::RecordedGradient;
    use crate::scene::shapes::paint::{ChromeRow, LoweredShadow, ShapeStroke};
    use crate::scene::shapes::record::ShapeRecord;
    use crate::scene::tree::extras::ExtrasIdx;
    use crate::scene::tree::record::NodeRecord;
    use crate::text::key::TextShapeKey;
    use crate::text::render::PlacedGlyph;
    use crate::text::shaped_ref::ShapedTextRef;
    use crate::ui::Ui;
    use crate::widgets::button::Button;
    use crate::widgets::checkbox::Checkbox;
    use crate::widgets::combo_box::ComboBox;
    use crate::widgets::drag_value::DragValue;
    use crate::widgets::progress_bar::ProgressBar;
    use crate::widgets::radio::RadioButton;
    use crate::widgets::slider::Slider;
    use crate::widgets::splitter::Splitter;
    use crate::widgets::switch::Switch;
    use crate::widgets::text::Text;
    use crate::widgets::text_edit::TextEdit;
    use crate::widgets::theme::widget_look::animated_look::AnimatedLook;
    use crate::widgets::widget::Widget;

    /// Single source of truth for the per-frame hot-struct inventory.
    /// Each entry is `Type => "name": expected_size / expected_align`.
    /// Drives two tests from one list:
    ///
    /// - [`print_hot_struct_sizes`] (`#[ignore]`) prints the live
    ///   `size`/`align` table — run it to read off a new number when a
    ///   layout change is intentional.
    /// - [`hot_struct_sizes_are_pinned`] (a real gate) asserts each
    ///   `(size, align)` so a *silent* footprint regression — an added
    ///   field, a stop-cap bump, an enum variant that re-inlines a boxed
    ///   payload — fails `cargo test` instead of diffusing across the
    ///   codebase. When the change is intended, update the number next to
    ///   the type; that one-line edit is the review signal.
    ///
    /// Sizes are for the 64-bit target (the only one). Covers the SoA
    /// per-node columns, per-shape/per-chrome lowered forms, the
    /// encoder↔composer wire payloads, the GPU instance types, and the
    /// one whole-`Ui` entry ([`UI_SIZE`]).
    ///
    /// The expected size is a `tt`, not a literal, so a type whose
    /// footprint is feature-conditional can name a `cfg`'d const in the
    /// same column as everything else's number.
    macro_rules! hot_structs {
        ($($ty:ty => $name:literal : $size:tt / $align:literal),+ $(,)?) => {
            #[test]
            #[ignore = "print-only"]
            fn print_hot_struct_sizes() {
                let rows = [$(($name, size_of::<$ty>(), align_of::<$ty>())),+];
                let name_w = rows.iter().map(|(n, ..): &(&str, _, _)| n.len()).max().unwrap_or(0);
                println!();
                println!("{:<w$}  {:>5}  {:>5}", "struct", "size", "align", w = name_w);
                println!("{:-<w$}  {:->5}  {:->5}", "", "", "", w = name_w);
                for (n, s, a) in &rows {
                    println!("{:<w$}  {:>5}  {:>5}", n, s, a, w = name_w);
                }
                println!();
            }

            #[test]
            fn hot_struct_sizes_are_pinned() {
                $(
                    assert_eq!(
                        (size_of::<$ty>(), align_of::<$ty>()),
                        ($size, $align),
                        concat!(
                            "size/align of ", $name,
                            " drifted from the pin — update it here if the change is intentional",
                        ),
                    );
                )+
            }
        };
    }

    /// Expected `size_of::<Ui>()`. Two numbers because `LayoutCounters`
    /// carries a `PhaseTimings` only under `bench` — the sole
    /// feature-conditional footprint in the table below.
    ///
    /// Both are the **`cfg(test)`** size, since that is what this module
    /// compiles as: `LayoutCounters`' `TestOnly` fields are live here and
    /// zero-sized in a release build, so a shipped `Ui` is ~90 B smaller
    /// than either. Read these as a drift tripwire, not as the production
    /// footprint.
    #[cfg(feature = "bench")]
    const UI_SIZE: usize = 6872;
    #[cfg(not(feature = "bench"))]
    const UI_SIZE: usize = 6848;

    hot_structs! {
        // One instance per window, not per frame — pinned because every
        // pass walks `&mut Ui` to reach `forest` / `layout` / `anim` /
        // `input` / `cascade`, so anything parked inline between them
        // costs locality on all of them. `Theme` used to be: at 8904 B
        // it made `Ui` 15656 B, and moving it behind an `Rc` (halving
        // `Ui`) measured -6% on `frame/cached_cpu` and -6% on
        // `frame/partial_cpu`. That is the regression this number
        // exists to catch; a new field is fine, a new multi-KB blob is
        // the thing to argue about.
        Ui => "ui::Ui": UI_SIZE / 8,
        // Per-node SoA columns (touched every node, every frame).
        NodeRecord => "scene::NodeRecord": 56 / 8,
        LayoutCore => "scene::LayoutCore": 28 / 4,
        NodeFlags => "scene::NodeFlags": 2 / 2,
        ExtrasIdx => "scene::ExtrasIdx": 6 / 2,
        BoundsExtras => "scene::BoundsExtras": 32 / 4,
        PanelExtras => "scene::PanelExtras": 20 / 4,
        Node => "scene::Node": 120 / 8,
        // Per-shape / per-chrome paint records + lowered fill forms.
        ShapeRecord => "scene::ShapeRecord": 88 / 8,
        RecordedText => "shapes::RecordedText": 16 / 8,
        ChromeRow => "scene::ChromeRow": 64 / 8,
        ShapeStroke => "shapes::ShapeStroke": 12 / 4,
        LoweredShadow => "shapes::LoweredShadow": 18 / 2,
        RecordedGradient => "shapes::RecordedGradient": 56 / 4,
        ResolvedGradient => "payload::ResolvedGradient": 16 / 4,
        // Authoring paint primitives.
        Background => "primitives::Background": 124 / 4,
        Brush => "primitives::Brush": 60 / 4,
        Span => "layout::Span": 8 / 4,
        Button<'static> => "widgets::Button": 160 / 8,
        Checkbox<'static> => "widgets::Checkbox": 160 / 8,
        Switch<'static> => "widgets::Switch": 160 / 8,
        ComboBox<'static> => "widgets::ComboBox": 152 / 8,
        DragValue<'static> => "widgets::DragValue": 200 / 8,
        RadioButton<'static, u8> => "widgets::RadioButton<u8>": 168 / 8,
        TextEdit<'static> => "widgets::TextEdit": 184 / 8,
        Text<'static> => "widgets::Text": 160 / 8,
        Slider<'static> => "widgets::Slider": 152 / 8,
        ProgressBar<'static> => "widgets::ProgressBar": 136 / 8,
        Splitter<'static> => "widgets::Splitter": 144 / 8,
        // Layout / text outputs.
        ShapedText => "layout::ShapedText": 32 / 8,
        TextShapeKey => "text::TextShapeKey": 24 / 8,
        // Cross-frame animation rows.
        AnimRow<AnimatedLook> => "animation::AnimRow<AnimatedLook>": 472 / 8,
        // Cross-frame hash keys.
        ContentHash => "rollups::ContentHash": 8 / 8,
        CascadeInputHash => "cascade::CascadeInputHash": 8 / 8,
        // Cascade per-node and input per-target rows.
        EntryRow => "cascade::EntryRow": 32 / 4,
        HitRow => "cascade::HitRow": 32 / 8,
        Paint => "cascade::Paint": 24 / 8,
        ResponseState => "input::ResponseState": 136 / 4,
        Widget => "widgets::Widget": 128 / 8,
        TargetScrollDelta => "input::TargetScrollDelta": 32 / 8,
        // Damage.
        DamageRegion => "damage::DamageRegion": 140 / 4,
        NodeSnapshot => "damage::snapshot::NodeSnapshot": 40 / 8,
        // Encoder↔composer wire payloads.
        PushClipPayload => "payload::PushClipPayload": 24 / 4,
        DrawQuadPayload => "payload::DrawQuadPayload": 76 / 4,
        DrawTextPayload => "payload::DrawTextPayload": 56 / 8,
        DrawPolylinePayload => "payload::DrawPolylinePayload": 52 / 4,
        DrawMeshPayload => "payload::DrawMeshPayload": 48 / 4,
        DrawImagePayload => "payload::DrawImagePayload": 56 / 8,
        DrawCurvePayload => "payload::DrawCurvePayload": 88 / 4,
        // GPU instance / vertex types.
        Quad => "renderer::Quad": 60 / 4,
        CurveInstance => "renderer::CurveInstance": 68 / 4,
        MeshInstance => "renderer::MeshInstance": 16 / 4,
        ImageInstance => "renderer::ImageInstance": 40 / 4,
        MeshVertex => "primitives::MeshVertex": 12 / 4,
        GlyphInstance => "text::GlyphInstance": 20 / 4,
        PlacedGlyph => "text::PlacedGlyph": 32 / 4,
        ShapedTextRef => "text::ShapedTextRef": 32 / 8,
        TextDrawRow => "renderer::TextDrawRow": 64 / 8,
    }
}
