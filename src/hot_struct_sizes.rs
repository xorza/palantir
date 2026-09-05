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

/// One inventory row: `T`'s live size and alignment beside the pinned ones.
#[derive(Debug)]
struct Pin {
    name: &'static str,
    size: usize,
    align: usize,
    want_size: usize,
    want_align: usize,
}

const fn pin<T>(name: &'static str, want_size: usize, want_align: usize) -> Pin {
    Pin {
        name,
        size: size_of::<T>(),
        align: align_of::<T>(),
        want_size,
        want_align,
    }
}

/// Expected `size_of::<Ui>()`, as `cfg(test)` sees it. `FrameRuntime`
/// carries a probe cell, so a release `Ui` can be smaller — see
/// [`FRAME_ENGINES_SIZE`], where the same gate is worth ~90 B.
const UI_SIZE: usize = 6016;

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

/// Single source of truth for the per-frame hot-struct inventory.
/// Each entry is `pin::<Type>("name", expected_size, expected_align)`.
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
/// A row's expected size is any `usize` expression, so a type whose
/// number needs an explanation of its own names a const in the same
/// column as everything else's number.
const PINS: &[Pin] = &[
    // One instance per window, not per frame — pinned because every
    // pass walks `&mut Ui` to reach `forest` / `layout` / `anim` /
    // `input` / `cascade`, so anything parked inline between them
    // costs locality on all of them. `Theme` is the measured case:
    // inline at 8904 B it makes `Ui` 15656 B, and holding it behind
    // an `Rc` (halving `Ui`) measured -6% on `frame/cached_cpu` and -6% on
    // `frame/partial_cpu`. That is the regression this number
    // exists to catch; a new field is fine, a new multi-KB blob is
    // the thing to argue about.
    pin::<Ui>("ui::Ui", UI_SIZE, 8),
    // The other per-window instance, holding the retained caches the
    // passes run on. Pinned for the same locality reason, and split
    // from `Ui` so a cache growing here cannot be mistaken for the
    // recorder growing.
    pin::<FrameEngines>("ui::FrameEngines", FRAME_ENGINES_SIZE, 8),
    pin::<NodeRecord>("scene::NodeRecord", 64, 8),
    pin::<LayoutCore>("scene::LayoutCore", 28, 4),
    pin::<NodeFlags>("scene::NodeFlags", 2, 2),
    pin::<ExtrasIdx>("scene::ExtrasIdx", 6, 2),
    pin::<BoundsExtras>("scene::BoundsExtras", 32, 4),
    pin::<PanelExtras>("scene::PanelExtras", 20, 4),
    pin::<Node>("scene::Node", 100, 4),
    pin::<ShapeRecord>("scene::ShapeRecord", 88, 8),
    pin::<RecordedText>("shapes::RecordedText", 16, 8),
    pin::<ChromeRow>("scene::ChromeRow", 64, 8),
    pin::<ShapeStroke>("shapes::ShapeStroke", 12, 4),
    pin::<LoweredShadow>("shapes::LoweredShadow", 18, 2),
    pin::<RecordedGradient>("shapes::RecordedGradient", 56, 4),
    pin::<ResolvedGradient>("payload::ResolvedGradient", 16, 4),
    pin::<Background>("primitives::Background", 124, 4),
    pin::<Brush>("primitives::Brush", 60, 4),
    pin::<Span>("layout::Span", 8, 4),
    pin::<Button<'static>>("widgets::Button", 160, 8),
    pin::<Checkbox<'static>>("widgets::Checkbox", 160, 8),
    pin::<Switch<'static>>("widgets::Switch", 160, 8),
    pin::<ComboBox<'static, &'static str>>("widgets::ComboBox", 160, 8),
    pin::<DragValue<'static>>("widgets::DragValue", 200, 8),
    pin::<RadioButton<'static, u8>>("widgets::RadioButton<u8>", 168, 8),
    pin::<TextEdit<'static>>("widgets::TextEdit", 184, 8),
    pin::<Text<'static>>("widgets::Text", 160, 8),
    pin::<Slider<'static>>("widgets::Slider", 184, 8),
    pin::<ProgressBar<'static>>("widgets::ProgressBar", 136, 8),
    pin::<Splitter<'static>>("widgets::Splitter", 144, 8),
    pin::<Panel>("widgets::Panel", 248, 8),
    pin::<Frame>("widgets::Frame", 248, 8),
    pin::<Grid>("widgets::Grid", 248, 8),
    pin::<Scroll<'static>>("widgets::Scroll", 288, 8),
    pin::<Separator<'static>>("widgets::Separator", 160, 8),
    pin::<Spinner<'static>>("widgets::Spinner", 168, 8),
    pin::<Popup>("widgets::Popup", 272, 8),
    pin::<Modal<'static>>("widgets::Modal", 272, 8),
    pin::<Tooltip<'static>>("widgets::Tooltip", 304, 8),
    pin::<GpuView>("widgets::GpuView", 144, 8),
    pin::<ContextMenu<'static>>("widgets::ContextMenu", 288, 8),
    pin::<MenuItem<'static>>("widgets::MenuItem", 168, 8),
    pin::<ShapedText>("layout::ShapedText", 32, 8),
    pin::<TextShapeKey>("text::TextShapeKey", 24, 8),
    pin::<MeasureSnapshot>("layout::MeasureSnapshot", 312, 8),
    pin::<AnimRow<AnimatedLook>>("animation::AnimRow<AnimatedLook>", 488, 8),
    pin::<ContentHash>("common::ContentHash", 8, 8),
    pin::<CascadeInputHash>("cascade::CascadeInputHash", 8, 8),
    pin::<EntryRow>("cascade::EntryRow", 32, 4),
    pin::<HitRow>("cascade::HitRow", 32, 8),
    pin::<Paint>("cascade::Paint", 24, 8),
    pin::<ResponseState>("input::ResponseState", 136, 4),
    pin::<Widget>("widgets::Widget", 120, 8),
    pin::<TargetScrollDelta>("input::TargetScrollDelta", 32, 8),
    pin::<DamageRegion>("damage::DamageRegion", 132, 4),
    pin::<CollapsedDamage>("damage::CollapsedDamage", 136, 4),
    pin::<NodeSnapshot>("damage::node_snapshot::NodeSnapshot", 40, 8),
    pin::<PushClipPayload>("payload::PushClipPayload", 24, 4),
    pin::<DrawQuadPayload>("payload::DrawQuadPayload", 76, 4),
    pin::<DrawTextPayload>("payload::DrawTextPayload", 56, 8),
    pin::<DrawPolylinePayload>("payload::DrawPolylinePayload", 52, 4),
    pin::<DrawMeshPayload>("payload::DrawMeshPayload", 48, 4),
    pin::<DrawImagePayload>("payload::DrawImagePayload", 56, 8),
    pin::<DrawCurvePayload>("payload::DrawCurvePayload", 88, 4),
    pin::<DrawIconPayload>("payload::DrawIconPayload", 32, 4),
    pin::<Quad>("renderer::Quad", 60, 4),
    pin::<CurveInstance>("renderer::CurveInstance", 68, 4),
    pin::<MeshInstance>("renderer::MeshInstance", 16, 4),
    pin::<ImageInstance>("renderer::ImageInstance", 40, 4),
    pin::<MeshVertex>("primitives::MeshVertex", 12, 4),
    pin::<RasterQuad>("atlas::RasterQuad", 20, 4),
    pin::<PlacedGlyph>("text::PlacedGlyph", 32, 4),
    pin::<ShapedTextRef>("text::ShapedTextRef", 32, 8),
    pin::<TextDrawRow>("renderer::TextDrawRow", 64, 8),
    pin::<IconDrawRow>("renderer::IconDrawRow", 24, 4),
    pin::<ImageDrawRow>("renderer::ImageDrawRow", 48, 8),
    pin::<MeshDrawRow>("renderer::MeshDrawRow", 32, 4),
];

#[test]
#[ignore = "print-only"]
fn print_hot_struct_sizes() {
    let name_w = PINS.iter().map(|p| p.name.len()).max().unwrap_or(0);
    println!();
    println!(
        "{:<w$}  {:>5}  {:>5}",
        "struct",
        "size",
        "align",
        w = name_w
    );
    println!("{:-<w$}  {:->5}  {:->5}", "", "", "", w = name_w);
    for p in PINS {
        println!("{:<w$}  {:>5}  {:>5}", p.name, p.size, p.align, w = name_w);
    }
    println!();
}

#[test]
fn hot_struct_sizes_are_pinned() {
    for p in PINS {
        assert_eq!(
            (p.size, p.align),
            (p.want_size, p.want_align),
            "size/align of {} drifted from the pin — update it here if the change is intentional",
            p.name,
        );
    }
}
