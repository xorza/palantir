// Re-import `aperture` as a self-alias so proc-macros that emit
// `::aperture::Animatable` paths (from `aperture-anim-derive`) resolve
// when the derive is used *inside* the crate (e.g. on `Stroke`,
// `Background`). Outside the crate this path resolves naturally.

extern crate self as aperture;

pub(crate) mod animation;
pub(crate) mod app;
#[cfg(feature = "internals")]
pub mod bench;
pub(crate) mod common;
pub(crate) mod debug_overlay;
/// Per-output display state (physical size, DPR, pixel-snap, refresh) —
/// cross-cutting host/render vocabulary, read by `ui`, the renderer, and
/// the host layer; not owned by any one subsystem.
pub(crate) mod display;
pub(crate) mod forest;
pub(crate) mod host;
pub(crate) mod input;
pub(crate) mod layout;
pub(crate) mod primitives;
pub(crate) mod record_store;
pub(crate) mod renderer;
pub(crate) mod shape;
pub(crate) mod text;
pub(crate) mod ui;
pub(crate) mod widgets;
pub(crate) mod window;

#[cfg(feature = "internals")]
pub use host::offscreen::test_support::{
    OffscreenWindowScratch, TwoWindowOffscreenHost, offscreen_window_scratch,
};
#[cfg(feature = "internals")]
pub use host::test_gpu::{HeadlessTestGpuLease, headless_test_gpu};
/// GPU pass-timing + pipeline-statistics handles, refreshed each frame by
/// the backend (timestamp-query + pipeline-statistics readback).
/// Consumers (debug overlay, benches) hold a `Clone` of the same
/// `GpuPassStats` the backend writes into — no global state;
/// `OffscreenHost::gpu_pass_stats` is the canonical handle.
pub use renderer::backend::gpu_pass_stats::{BatchKind, GpuPassStats, PipelineStats};

pub use animation::animatable::Animatable;
pub use animation::easing::Easing;
pub use animation::{AnimSlot, AnimSpec};
pub use app::App;
// Same-name re-export: the derive lives in the macro namespace,
// the trait in the type namespace — `use aperture::Animatable;` pulls
// both, and `#[derive(Animatable)]` works alongside `T: Animatable`.
pub use aperture_anim_derive::Animatable;
pub use debug_overlay::DebugOverlayConfig;
pub use display::Display;
pub use forest::element::{Configure, Element};
pub use forest::layer::Layer;
pub use forest::visibility::Visibility;
pub use host::clock::{Clock, FixedClock, RealtimeClock};
/// The headless render-to-texture host — the offscreen peer of
/// [`WinitHost`]. Renders a `Ui` to a caller-supplied `wgpu::Texture`
/// instead of a swapchain (screenshots, thumbnails, server-side
/// compositing); also backs the visual harness + GPU benches.
pub use host::offscreen::{OffscreenHost, OffscreenHostBuilder};
pub use host::winit::config::WinitHostConfig;
pub use host::winit::handle::{HostHandle, UserEvent};
pub use host::winit::{WinitHost, WinitHostBuilder};
pub use input::InputEvent;
pub use input::keyboard::{Key, KeyPress, KeyboardEvent, Modifiers, TextChunk};
pub use input::pointer::{PointerButton, PointerEvent};
pub use input::policy::{FocusPolicy, InputPolicy};
pub use input::response::{ButtonPhase, ButtonState, Drag, InputDelta, ResponseState, ScrollDelta};
pub use input::sense::Sense;
pub use input::shortcut::{Mods, Shortcut};
pub use input::subscriptions::{KeyboardSense, PointerSense};
pub use layout::types::align::{Align, HAlign, VAlign};
pub use layout::types::clip_mode::ClipMode;
pub use layout::types::grid_cell::GridCell;
pub use layout::types::justify::Justify;
pub use layout::types::sizing::{Sizes, Sizing};
pub use layout::types::track::Track;
pub use primitives::background::Background;
pub use primitives::brush::{
    Brush, ConicGradient, CurveBrush, GradientStops, Interp, LinearGradient, RadialGradient,
    Spread, Stop,
};
pub use primitives::color::Color;
pub use primitives::color::ColorU8;
pub use primitives::corners::Corners;
pub use primitives::image::{Image, ImageFilter, ImageFit};
pub use primitives::interned_str::{InternedStr, TextInput};
pub use primitives::mesh::{Mesh, MeshVertex};
pub use primitives::rect::Rect;
pub use primitives::shadow::Shadow;
pub use primitives::size::Size;
pub use primitives::spacing::{Spacing, Sums};
// Re-exported (not an aperture type) because it's the canonical integer
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
pub use renderer::image_registry::ImageHandle;
pub use shape::{LineCap, LineJoin, PolylineColors, Shape, TextWrap};
pub use text::cosmic::CosmicMeasure;
pub use text::{
    FontFamily, FontWeight, ShapeParams, ShapeParamsError, TextMeasurement, TextShaper,
};
pub use ui::Ui;
pub use ui::frame::FrameStamp;
pub use ui::frame_report::{FramePaint, FrameProcessing, FrameReport};
pub use widgets::button::Button;
pub use widgets::checkbox::Checkbox;
pub use widgets::combo_box::ComboBox;
pub use widgets::context_menu::{ContextMenu, ContextMenuResponse, MenuItem};
pub use widgets::drag_value::{DragNum, DragValue, DragValueResponse};
pub use widgets::frame::Frame;
pub use widgets::gpu_view::GpuView;
pub use widgets::grid::Grid;
pub use widgets::modal::{Modal, ModalResponse};
pub use widgets::panel::Panel;
pub use widgets::popup::{ClickOutside, Popup, PopupHandle, PopupResponse};
pub use widgets::progress_bar::ProgressBar;
pub use widgets::radio::RadioButton;
pub use widgets::scroll::{BarMode, Scroll, ZoomConfig, ZoomModifier, ZoomPivot};
pub use widgets::separator::Separator;
pub use widgets::slider::Slider;
pub use widgets::spinner::Spinner;
pub use widgets::splitter::{SplitHalf, Splitter};
pub use widgets::switch::Switch;
pub use widgets::text::Text;
pub use widgets::text_edit::{TextEdit, TextEditResponse};
pub use widgets::theme::Theme;
pub use widgets::theme::button::ButtonTheme;
pub use widgets::theme::context_menu::{ContextMenuTheme, MenuItemTheme};
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
pub use widgets::theme::widget_look::{AnimatedLook, StatefulLook, WidgetLook};
pub use widgets::tooltip::Tooltip;
pub use widgets::{InnerResponse, Response, ResponseSnapshot};
pub use window::{CursorIcon, WindowConfig, WindowGeometry, WindowToken};

#[cfg(test)]
mod hot_struct_sizes {
    use crate::animation::AnimRow;
    use crate::common::content_hash::ContentHash;
    use crate::forest::element::Element;
    use crate::forest::element::columns::{BoundsExtras, LayoutCore, NodeFlags, PanelExtras};
    use crate::forest::shapes::paint::{ChromeRow, LoweredShadow, ShapeStroke};
    use crate::forest::shapes::record::ShapeRecord;
    use crate::forest::tree::extras::ExtrasIdx;
    use crate::forest::tree::node::NodeRecord;
    use crate::input::TargetDeltas;
    use crate::layout::ShapedText;
    use crate::primitives::background::Background;
    use crate::primitives::brush::Brush;
    use crate::primitives::interned_str::RecordedText;
    use crate::primitives::mesh::MeshVertex;
    use crate::primitives::span::Span;
    use crate::record_store::RecordedGradient;
    use crate::renderer::backend::text::GlyphInstance;
    use crate::renderer::frontend::cmd_buffer::payload::{
        DrawArcPayload, DrawCurvePayload, DrawImagePayload, DrawMeshPayload, DrawPolylinePayload,
        DrawRectPayload, DrawShadowPayload, DrawTextPayload, DrawTrianglePayload, PushClipPayload,
        ResolvedGradient,
    };
    use crate::renderer::quad::Quad;
    use crate::renderer::render_buffer::curve::CurveInstance;
    use crate::renderer::render_buffer::image::ImageInstance;
    use crate::renderer::render_buffer::mesh::MeshInstance;
    use crate::renderer::render_buffer::text::TextRun;
    use crate::text::TextCacheKey;
    use crate::ui::cascade::CascadeInputHash;
    use crate::ui::cascade::EntryRow;
    use crate::ui::cascade::Paint;
    use crate::ui::damage::region::DamageRegion;
    use crate::ui::damage::snapshot::NodeSnapshot;
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
    use crate::widgets::theme::widget_look::AnimatedLook;

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
    /// encoder↔composer wire payloads, and the GPU instance types.
    macro_rules! hot_structs {
        ($($ty:ty => $name:literal : $size:literal / $align:literal),+ $(,)?) => {
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

    hot_structs! {
        // Per-node SoA columns (touched every node, every frame).
        NodeRecord => "forest::NodeRecord": 56 / 8,
        LayoutCore => "forest::LayoutCore": 28 / 4,
        NodeFlags => "forest::NodeFlags": 2 / 2,
        ExtrasIdx => "forest::ExtrasIdx": 6 / 2,
        BoundsExtras => "forest::BoundsExtras": 32 / 4,
        PanelExtras => "forest::PanelExtras": 20 / 4,
        Element => "forest::Element": 104 / 8,
        // Per-shape / per-chrome paint records + lowered fill forms.
        ShapeRecord => "forest::ShapeRecord": 88 / 8,
        RecordedText => "shapes::RecordedText": 16 / 8,
        ChromeRow => "forest::ChromeRow": 56 / 8,
        ShapeStroke => "shapes::ShapeStroke": 10 / 2,
        LoweredShadow => "shapes::LoweredShadow": 18 / 2,
        RecordedGradient => "shapes::RecordedGradient": 56 / 4,
        ResolvedGradient => "cmd::ResolvedGradient": 16 / 4,
        // Authoring paint primitives.
        Background => "primitives::Background": 124 / 4,
        Brush => "primitives::Brush": 60 / 4,
        Span => "layout::Span": 8 / 4,
        Button<'static> => "widgets::Button": 144 / 8,
        Checkbox<'static> => "widgets::Checkbox": 144 / 8,
        Switch<'static> => "widgets::Switch": 144 / 8,
        ComboBox<'static> => "widgets::ComboBox": 136 / 8,
        DragValue<'static> => "widgets::DragValue": 184 / 8,
        RadioButton<'static, u8> => "widgets::RadioButton<u8>": 152 / 8,
        TextEdit<'static> => "widgets::TextEdit": 168 / 8,
        Text<'static> => "widgets::Text": 144 / 8,
        Slider<'static> => "widgets::Slider": 136 / 8,
        ProgressBar<'static> => "widgets::ProgressBar": 120 / 8,
        Splitter<'static> => "widgets::Splitter": 128 / 8,
        // Layout / text outputs.
        ShapedText => "layout::ShapedText": 32 / 8,
        TextCacheKey => "text::TextCacheKey": 24 / 8,
        // Cross-frame animation rows.
        AnimRow<AnimatedLook> => "animation::AnimRow<AnimatedLook>": 472 / 8,
        // Cross-frame hash keys.
        ContentHash => "rollups::ContentHash": 8 / 8,
        CascadeInputHash => "cascade::CascadeInputHash": 8 / 8,
        // Cascade per-node and input per-target rows.
        EntryRow => "cascade::EntryRow": 56 / 8,
        Paint => "cascade::Paint": 24 / 8,
        TargetDeltas => "input::TargetDeltas": 32 / 8,
        // Damage.
        DamageRegion => "damage::DamageRegion": 140 / 4,
        NodeSnapshot => "damage::snapshot::NodeSnapshot": 40 / 8,
        // Encoder↔composer wire payloads.
        PushClipPayload => "cmd::PushClipPayload": 24 / 4,
        DrawRectPayload => "cmd::DrawRectPayload": 60 / 4,
        DrawShadowPayload => "cmd::DrawShadowPayload": 44 / 4,
        DrawTextPayload => "cmd::DrawTextPayload": 48 / 8,
        DrawPolylinePayload => "cmd::DrawPolylinePayload": 52 / 4,
        DrawMeshPayload => "cmd::DrawMeshPayload": 48 / 4,
        DrawImagePayload => "cmd::DrawImagePayload": 56 / 8,
        DrawTrianglePayload => "cmd::DrawTrianglePayload": 56 / 4,
        DrawCurvePayload => "cmd::DrawCurvePayload": 84 / 4,
        DrawArcPayload => "cmd::DrawArcPayload": 72 / 4,
        // GPU instance / vertex types.
        Quad => "renderer::Quad": 60 / 4,
        CurveInstance => "renderer::CurveInstance": 68 / 4,
        MeshInstance => "renderer::MeshInstance": 16 / 4,
        ImageInstance => "renderer::ImageInstance": 40 / 4,
        MeshVertex => "primitives::MeshVertex": 12 / 4,
        GlyphInstance => "text::GlyphInstance": 20 / 4,
        TextRun => "renderer::TextRun": 56 / 8,
    }
}
