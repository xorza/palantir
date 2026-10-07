//! Inventory of the per-frame types whose `size`/`align` must not drift silently.

use crate::animation::anim_row::AnimRow;
use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::entry::{EntryRow, HitRow};
use crate::cascade::paint::Paint;
use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::damage::node_snapshot::NodeSnapshot;
use crate::damage::region::{CollapsedDamage, DamageRegion};
use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::input::interaction::response_state::ResponseState;
use crate::input::target_scroll_delta::TargetScrollDelta;
use crate::layout::cache::MeasureSnapshot;
use crate::layout::text::shaped_text::ShapedText;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::mesh::MeshVertex;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::packed_layout_meta::PackedLayoutMeta;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, MAX_STOPS};
use crate::primitives::text::recorded_text::RecordedText;
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
use crate::scene::node::Node;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::seen_ids::IdEntry;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::node_record::NodeRecord;
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::record::ShapeRecord;
use crate::text::key::TextShapeKey;
use crate::text::render::PlacedGlyph;
use crate::text::shaped_ref::ShapedTextRef;
use crate::ui::Ui;
use crate::ui::frame_engines::FrameEngines;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::animated_look::AnimatedLook;
use crate::widgets::block::Block;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::drag_value::DragValue;
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
use crate::widgets::tooltip::Tooltip;

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

/// Expected `size_of::<Ui>()` under `cfg(test)`; a release `Ui` can be smaller.
const UI_SIZE: usize = 9352;

/// Expected `size_of::<FrameEngines>()` under `cfg(test)`, the only build this
/// module compiles in. Test-only and bench-only cells are zero-sized in
/// release, so read this as a drift tripwire, not the shipped footprint.
const FRAME_ENGINES_SIZE: usize = 2048;

/// The per-frame hot-struct inventory, one `pin::<Type>(name, size, align)` per
/// row, driving [`print_hot_struct_sizes`] (`#[ignore]`; prints the live table)
/// and [`hot_struct_sizes_are_pinned`] (asserts each row). Update the number
/// when a change is intentional. Sizes are for 64-bit targets.
const PINS: &[Pin] = &[
    // Pinned for locality: every pass walks `&mut Ui`, so inline blobs between
    // its fields cost on all of them. `Theme` inline made `Ui` 15656 B; behind
    // an `Rc` measured -6% on `frame/cached_cpu` and `frame/partial_cpu`.
    pin::<Ui>("ui::Ui", UI_SIZE, 8),
    // Split from `Ui` so cache growth is not mistaken for recorder growth.
    pin::<FrameEngines>("ui::FrameEngines", FRAME_ENGINES_SIZE, 8),
    pin::<NodeRecord>("scene::NodeRecord", 64, 8),
    pin::<IdEntry>("scene::IdEntry", 64, 8),
    pin::<LayoutCore>("scene::LayoutCore", 28, 4),
    pin::<NodeFlags>("scene::NodeFlags", 4, 4),
    pin::<LayoutMode>("primitives::LayoutMode", 4, 2),
    pin::<PackedLayoutMeta>("primitives::PackedLayoutMeta", 4, 4),
    pin::<ExtrasIdx>("scene::ExtrasIdx", 6, 2),
    pin::<BoundsExtras>("scene::BoundsExtras", 36, 4),
    pin::<PanelExtras>("scene::PanelExtras", 20, 4),
    pin::<Node>("scene::Node", 104, 4),
    pin::<ShapeRecord>("shape::ShapeRecord", 88, 8),
    pin::<RecordedText>("shapes::RecordedText", 16, 8),
    pin::<ChromeRow>("scene::ChromeRow", 64, 8),
    pin::<ShapeStroke>("shape::ShapeStroke", 12, 4),
    pin::<LoweredShadow>("shape::LoweredShadow", 18, 2),
    pin::<RecordedGradient>("shapes::RecordedGradient", 56, 4),
    pin::<ResolvedGradient>("payload::ResolvedGradient", 16, 4),
    pin::<Background>("primitives::Background", 124, 4),
    // Align 2, so embedding either in `Quad` keeps its alignment at 4 and
    // `Pod` free of padding.
    pin::<Spacing>("primitives::Spacing", 8, 2),
    pin::<Corners>("primitives::Corners", 8, 2),
    // Inline in every `Brush::Linear`, so it sets the floor for `Brush`. Stops:
    // 1 (len) + `MAX_STOPS` * 5; ramp adds 1 (interpolation); gradient adds 4
    // (angle), 1 (spread) and 1 tail pad to align 4.
    pin::<GradientStops>("brush::GradientStops", 1 + 5 * MAX_STOPS, 1),
    pin::<ColorRamp>("brush::ColorRamp", 1 + 5 * MAX_STOPS + 1, 1),
    pin::<LinearGradient>("brush::LinearGradient", 48, 4),
    pin::<Brush>("primitives::Brush", 60, 4),
    pin::<Span>("common::Span", 8, 4),
    pin::<Button<'static>>("widgets::Button", 160, 8),
    pin::<Checkbox<'static>>("widgets::Checkbox", 160, 8),
    pin::<Switch<'static>>("widgets::Switch", 160, 8),
    pin::<ComboBox<'static, &'static str, for<'a> fn(&'a &'static str) -> &'a str>>(
        "widgets::ComboBox",
        168,
        8,
    ),
    pin::<DragValue<'static>>("widgets::DragValue", 208, 8),
    pin::<RadioButton<'static, u8>>("widgets::RadioButton<u8>", 168, 8),
    pin::<TextEdit<'static>>("widgets::TextEdit", 232, 8),
    pin::<Text<'static>>("widgets::Text", 208, 8),
    pin::<Slider<'static>>("widgets::Slider", 184, 8),
    pin::<ProgressBar<'static>>("widgets::ProgressBar", 136, 8),
    pin::<Splitter<'static>>("widgets::Splitter", 144, 8),
    pin::<Panel>("widgets::Panel", 248, 8),
    pin::<Block>("widgets::Block", 248, 8),
    pin::<Grid>("widgets::Grid", 248, 8),
    pin::<Scroll<'static>>("widgets::Scroll", 296, 8),
    pin::<Separator<'static>>("widgets::Separator", 160, 8),
    pin::<Spinner<'static>>("widgets::Spinner", 168, 8),
    pin::<Popup>("widgets::Popup", 272, 8),
    pin::<Modal<'static>>("widgets::Modal", 272, 8),
    pin::<Tooltip<'static>>("widgets::Tooltip", 304, 8),
    pin::<GpuView>("widgets::GpuView", 144, 8),
    pin::<ContextMenu<'static>>("widgets::ContextMenu", 296, 8),
    pin::<MenuItem<'static>>("widgets::MenuItem", 168, 8),
    pin::<ShapedText>("layout::ShapedText", 40, 8),
    pin::<TextShapeKey>("text::TextShapeKey", 24, 8),
    pin::<MeasureSnapshot>("layout::MeasureSnapshot", 384, 8),
    pin::<AnimRow<AnimatedLook>>("animation::AnimRow<AnimatedLook>", 496, 8),
    pin::<ContentHash>("common::ContentHash", 8, 8),
    pin::<CascadeInputHash>("cascade::CascadeInputHash", 8, 8),
    pin::<EntryRow>("cascade::EntryRow", 32, 4),
    pin::<HitRow>("cascade::HitRow", 32, 8),
    pin::<Paint>("cascade::Paint", 24, 8),
    pin::<ResponseState>("input::ResponseState", 136, 4),
    pin::<Widget>("widget_core::Widget", 120, 8),
    pin::<TargetScrollDelta>("input::TargetScrollDelta", 32, 8),
    pin::<DamageRegion>("damage::DamageRegion", 132, 4),
    pin::<CollapsedDamage>("damage::CollapsedDamage", 136, 4),
    pin::<NodeSnapshot>("damage::node_snapshot::NodeSnapshot", 40, 8),
    pin::<PushClipPayload>("payload::PushClipPayload", 24, 4),
    pin::<DrawQuadPayload>("payload::DrawQuadPayload", 76, 4),
    pin::<DrawTextPayload>("payload::DrawTextPayload", 64, 8),
    pin::<DrawPolylinePayload>("payload::DrawPolylinePayload", 56, 4),
    pin::<DrawMeshPayload>("payload::DrawMeshPayload", 48, 4),
    pin::<DrawImagePayload>("payload::DrawImagePayload", 56, 8),
    pin::<DrawCurvePayload>("payload::DrawCurvePayload", 88, 4),
    pin::<DrawIconPayload>("payload::DrawIconPayload", 32, 4),
    pin::<Quad>("renderer::Quad", 60, 4),
    pin::<CurveInstance>("renderer::CurveInstance", 76, 4),
    pin::<MeshInstance>("renderer::MeshInstance", 20, 4),
    pin::<ImageInstance>("renderer::ImageInstance", 44, 4),
    pin::<MeshVertex>("primitives::MeshVertex", 12, 4),
    pin::<RasterQuad>("atlas::RasterQuad", 28, 4),
    pin::<PlacedGlyph>("text::PlacedGlyph", 32, 4),
    pin::<ShapedTextRef>("text::ShapedTextRef", 32, 8),
    pin::<TextDrawRow>("renderer::TextDrawRow", 72, 8),
    pin::<IconDrawRow>("renderer::IconDrawRow", 32, 4),
    pin::<ImageDrawRow>("renderer::ImageDrawRow", 56, 8),
    pin::<MeshDrawRow>("renderer::MeshDrawRow", 36, 4),
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
