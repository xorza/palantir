//! The per-frame footprint inventory: one list of the types whose
//! `size`/`align` a change must not move silently, driving both a printing
//! run and the test that pins them.

use crate::animation::anim_row::AnimRow;
use crate::common::content_hash::ContentHash;
use crate::input::response::response_state::ResponseState;
use crate::input::target_scroll_delta::TargetScrollDelta;
use crate::layout::ShapedText;
use crate::layout::cache::MeasureSnapshot;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::mesh::MeshVertex;
use crate::primitives::recorded_text::RecordedText;
use crate::primitives::span::Span;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::DrawImagePayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::quad::Quad;
use crate::renderer::render_buffer::curve::CurveInstance;
use crate::renderer::render_buffer::icon::IconDrawRow;
use crate::renderer::render_buffer::image::ImageDrawRow;
use crate::renderer::render_buffer::image::ImageInstance;
use crate::renderer::render_buffer::mesh::MeshDrawRow;
use crate::renderer::render_buffer::mesh::MeshInstance;
use crate::renderer::render_buffer::text::TextDrawRow;
use crate::scene::cascade::CascadeInputHash;
use crate::scene::cascade::entry::{EntryRow, HitRow};
use crate::scene::cascade::paint::Paint;
use crate::scene::damage::node_snapshot::NodeSnapshot;
use crate::scene::damage::region::{CollapsedDamage, DamageRegion};
use crate::scene::node::Node;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::shapes::paint::{ChromeRow, LoweredShadow, ShapeStroke};
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::node_record::NodeRecord;
use crate::text::key::TextShapeKey;
use crate::text::render::PlacedGlyph;
use crate::text::shaped_ref::ShapedTextRef;
use crate::ui::Ui;
use crate::ui::frame_engines::FrameEngines;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::drag_value::DragValue;
use crate::widgets::frame::Frame;
use crate::widgets::gpu_view::GpuView;
use crate::widgets::grid::Grid;
use crate::widgets::modal::Modal;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::progress_bar::ProgressBar;
use crate::widgets::radio::RadioButton;
use crate::widgets::scroll::Scroll;
use crate::widgets::separator::Separator;
use crate::widgets::slider::Slider;
use crate::widgets::spinner::Spinner;
use crate::widgets::splitter::Splitter;
use crate::widgets::switch::Switch;
use crate::widgets::text::Text;
use crate::widgets::text_edit::TextEdit;
use crate::widgets::theme::widget_look::animated_look::AnimatedLook;
use crate::widgets::tooltip::Tooltip;
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
/// The expected size is a `tt`, not a literal, so a type whose number
/// needs an explanation of its own can name a const in the same column
/// as everything else's number.
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

/// Expected `size_of::<Ui>()`, as `cfg(test)` sees it. `FrameRuntime`
/// carries a probe cell, so a release `Ui` can be smaller — see
/// [`FRAME_ENGINES_SIZE`], where the same gate is worth ~90 B.
const UI_SIZE: usize = 6000;

/// Expected `size_of::<FrameEngines>()`, as **`cfg(test)`** sees it —
/// which is the only way this module compiles.
///
/// `LayoutCounters`' `TestOnly` and `BenchOnly` cells are both live
/// under that gate, and `BenchOnly` reads `any(test, feature =
/// "bench")`, so the `bench` feature adds no field a test build did not
/// already carry and one number covers every feature set. Both kinds of
/// cell are zero-sized in a release build, which leaves a shipped
/// `FrameEngines` ~90 B smaller. Read this as a drift tripwire, not as
/// the production footprint.
const FRAME_ENGINES_SIZE: usize = 1480;

hot_structs! {
    // One instance per window, not per frame — pinned because every
    // pass walks `&mut Ui` to reach `forest` / `layout` / `anim` /
    // `input` / `cascade`, so anything parked inline between them
    // costs locality on all of them. `Theme` is the measured case:
    // inline at 8904 B it makes `Ui` 15656 B, and holding it behind
    // an `Rc` (halving `Ui`) measured -6% on `frame/cached_cpu` and -6% on
    // `frame/partial_cpu`. That is the regression this number
    // exists to catch; a new field is fine, a new multi-KB blob is
    // the thing to argue about.
    Ui => "ui::Ui": UI_SIZE / 8,
    // The other per-window instance, holding the retained caches the
    // passes run on. Pinned for the same locality reason, and split
    // from `Ui` so a cache growing here cannot be mistaken for the
    // recorder growing.
    FrameEngines => "ui::FrameEngines": FRAME_ENGINES_SIZE / 8,
    // Per-node SoA columns (touched every node, every frame).
    NodeRecord => "scene::NodeRecord": 64 / 8,
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
    ComboBox<'static, &'static str> => "widgets::ComboBox": 160 / 8,
    DragValue<'static> => "widgets::DragValue": 200 / 8,
    RadioButton<'static, u8> => "widgets::RadioButton<u8>": 168 / 8,
    TextEdit<'static> => "widgets::TextEdit": 184 / 8,
    Text<'static> => "widgets::Text": 160 / 8,
    Slider<'static> => "widgets::Slider": 184 / 8,
    ProgressBar<'static> => "widgets::ProgressBar": 136 / 8,
    Splitter<'static> => "widgets::Splitter": 144 / 8,
    Panel => "widgets::Panel": 248 / 8,
    Frame => "widgets::Frame": 248 / 8,
    Grid => "widgets::Grid": 248 / 8,
    Scroll<'static> => "widgets::Scroll": 288 / 8,
    Separator<'static> => "widgets::Separator": 160 / 8,
    Spinner<'static> => "widgets::Spinner": 168 / 8,
    Popup => "widgets::Popup": 272 / 8,
    Modal<'static> => "widgets::Modal": 272 / 8,
    Tooltip<'static, 'static> => "widgets::Tooltip": 304 / 8,
    GpuView => "widgets::GpuView": 144 / 8,
    ContextMenu<'static> => "widgets::ContextMenu": 288 / 8,
    MenuItem<'static> => "widgets::MenuItem": 168 / 8,
    // Layout / text outputs.
    ShapedText => "layout::ShapedText": 32 / 8,
    TextShapeKey => "text::TextShapeKey": 24 / 8,
    // The measure cache's per-descriptor row, retained across frames.
    MeasureSnapshot => "layout::MeasureSnapshot": 312 / 8,
    // Cross-frame animation rows.
    AnimRow<AnimatedLook> => "animation::AnimRow<AnimatedLook>": 472 / 8,
    // Cross-frame hash keys.
    ContentHash => "common::ContentHash": 8 / 8,
    CascadeInputHash => "cascade::CascadeInputHash": 8 / 8,
    // Cascade per-node and input per-target rows.
    EntryRow => "cascade::EntryRow": 32 / 4,
    HitRow => "cascade::HitRow": 32 / 8,
    Paint => "cascade::Paint": 24 / 8,
    ResponseState => "input::ResponseState": 136 / 4,
    Widget => "widgets::Widget": 128 / 8,
    TargetScrollDelta => "input::TargetScrollDelta": 32 / 8,
    // Damage.
    DamageRegion => "damage::DamageRegion": 132 / 4,
    CollapsedDamage => "damage::CollapsedDamage": 136 / 4,
    NodeSnapshot => "damage::node_snapshot::NodeSnapshot": 40 / 8,
    // Encoder↔composer wire payloads.
    PushClipPayload => "payload::PushClipPayload": 24 / 4,
    DrawQuadPayload => "payload::DrawQuadPayload": 76 / 4,
    DrawTextPayload => "payload::DrawTextPayload": 56 / 8,
    DrawPolylinePayload => "payload::DrawPolylinePayload": 52 / 4,
    DrawMeshPayload => "payload::DrawMeshPayload": 48 / 4,
    DrawImagePayload => "payload::DrawImagePayload": 56 / 8,
    DrawCurvePayload => "payload::DrawCurvePayload": 88 / 4,
    DrawIconPayload => "payload::DrawIconPayload": 32 / 4,
    // GPU instance / vertex types.
    Quad => "renderer::Quad": 60 / 4,
    CurveInstance => "renderer::CurveInstance": 68 / 4,
    MeshInstance => "renderer::MeshInstance": 16 / 4,
    ImageInstance => "renderer::ImageInstance": 40 / 4,
    MeshVertex => "primitives::MeshVertex": 12 / 4,
    RasterQuad => "atlas::RasterQuad": 20 / 4,
    PlacedGlyph => "text::PlacedGlyph": 32 / 4,
    ShapedTextRef => "text::ShapedTextRef": 32 / 8,
    TextDrawRow => "renderer::TextDrawRow": 64 / 8,
    IconDrawRow => "renderer::IconDrawRow": 24 / 4,
    ImageDrawRow => "renderer::ImageDrawRow": 48 / 8,
    MeshDrawRow => "renderer::MeshDrawRow": 32 / 4,
}
