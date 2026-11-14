//! Inventory of the per-frame types whose `size`/`align` must not drift silently.

use crate::animation::animation_row::AnimationRow;
use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::paint::Paint;
use crate::cascade::rows::entry_row::EntryRow;
use crate::cascade::rows::hit_row::HitRow;
use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::damage::node_snapshot::NodeSnapshot;
use crate::damage::region::{CollapsedDamage, DamageRegion};
use crate::gpu::pipeline::raster_pipeline::raster_quad::{RasterQuad, SpunRasterQuad};
use crate::input::target_scroll_delta::TargetScrollDelta;
use crate::layout::cache::measure_snapshot::MeasureSnapshot;
use crate::primitives::effect::effect_kind::BackdropEffect;
use crate::primitives::effect::effect_name::EffectName;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::mesh::MeshVertex;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::interaction::response::Response;
use crate::primitives::input::interaction::response_state::ResponseState;
use crate::primitives::layout::layout_mode::LayoutMode;
use crate::primitives::layout::packed_layout_meta::PackedLayoutMeta;
use crate::primitives::paint::border::Border;
use crate::primitives::paint::brush::Brush;
use crate::primitives::paint::brush::gradient::Gradient;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::stops::{GradientStops, MAX_STOPS};
use crate::primitives::paint::chrome::Chrome;
use crate::primitives::paint::material::Material;
use crate::primitives::paint::material::material_effect::MaterialEffect;
use crate::primitives::paint::material::material_paint::MaterialPaint;
use crate::primitives::paint::paint;
use crate::primitives::paint::shadow::Shadow;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::text::recorded_text::RecordedText;
use crate::renderer::frontend::payload::draw_closed_payload::DrawClosedPayload;
use crate::renderer::frontend::payload::draw_closed_payload::closed_fill::ClosedFill;
use crate::renderer::frontend::payload::draw_closed_payload::closed_image::ClosedImage;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_shadow_payload::DrawShadowPayload;
use crate::renderer::frontend::payload::draw_stroke_payload::DrawStrokePayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::resolved_gradient::ResolvedGradient;
use crate::renderer::render_buffer::curve_instance::CurveInstance;
use crate::renderer::render_buffer::effect_draw_row::EffectInstance;
use crate::renderer::render_buffer::icon_draw_row::IconDrawRow;
use crate::renderer::render_buffer::image_draw_row::ImageDrawRow;
use crate::renderer::render_buffer::image_draw_row::ImageInstance;
use crate::renderer::render_buffer::mesh_draw_row::MeshDrawRow;
use crate::renderer::render_buffer::mesh_draw_row::MeshInstance;
use crate::renderer::render_buffer::quad_draw_row::QuadInstance;
use crate::renderer::render_buffer::text_draw_row::TextDrawRow;
use crate::scene::node::Node;
use crate::scene::node::bounds_extras::BoundsExtras;
use crate::scene::node::layout_core::LayoutCore;
use crate::scene::node::node_flags::NodeFlags;
use crate::scene::node::panel_extras::PanelExtras;
use crate::scene::seen_ids::IdEntry;
use crate::scene::tree::extras_idx::ExtrasIdx;
use crate::scene::tree::node_record::NodeRecord;
use crate::shape::draw::Draw;
use crate::shape::icon_paint::IconPaint;
use crate::shape::image_paint::ImagePaint;
use crate::shape::lower::LoweredPaint;
use crate::shape::paint::chrome_row::ChromeRow;
use crate::shape::paint::lowered_border::LoweredBorder;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::record::ShapeRecord;
use crate::shape::record_store::recorded_gradient::RecordedGradient;
use crate::text::font_family::FontFamily;
use crate::text::key::TextShapeKey;
use crate::text::render::PlacedGlyph;
use crate::text::shaped_ref::ShapedTextRef;
use crate::text::shaped_text::ShapedText;
use crate::theme::look::WidgetLook;
use crate::theme::look::animated_look::AnimatedLook;
use crate::theme::look::look_plan::LookPlan;
use crate::theme::text_style::TextStyle;
use crate::ui::Ui;
use crate::ui::window_frames::WindowFrames;
use crate::widget_core::widget::Widget;
use crate::widgets::block::Block;
use crate::widgets::button::Button;
use crate::widgets::checkbox::Checkbox;
use crate::widgets::combo_box::ComboBox;
use crate::widgets::drag_value::DragValue;
use crate::widgets::gpu_view::GpuView;
use crate::widgets::grid::Grid;
use crate::widgets::menu::Menu;
use crate::widgets::menu::menu_item::MenuItem;
use crate::widgets::modal::Modal;
use crate::widgets::panel::Panel;
use crate::widgets::popup::Popup;
use crate::widgets::progress_bar::ProgressBar;
use crate::widgets::radio_group::RadioGroup;
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

/// Expected `size_of::<Ui>()` under `cfg(test)`. One value for both profiles: the scene's
/// `cfg(debug_assertions)` check fields sit on each plane's `Tree`, which `Forest` keeps on the
/// heap.
const UI_SIZE: usize = 3008;

/// Expected `size_of::<WindowFrames>()` under `cfg(test)`, the only build this
/// module compiles in. Test-only and bench-only cells are zero-sized in
/// release, so read this as a drift tripwire, not the shipped footprint.
const WINDOW_FRAMES_SIZE: usize = 6696;

/// The per-frame hot-struct inventory, one `pin::<Type>(name, size, align)` per
/// row, driving [`print_hot_struct_sizes`] (`#[ignore]`; prints the live table)
/// and [`hot_struct_sizes_are_pinned`] (asserts each row). Update the number
/// when a change is intentional. Sizes are for 64-bit targets.
const PINS: &[Pin] = &[
    // Pinned for locality: every pass walks `&mut Ui`, so inline blobs between
    // its fields cost on all of them. `Theme` inline made `Ui` 15656 B; behind
    // an `Rc` measured -6% on `integration/frame/cpu/cached` and `integration/frame/cpu/partial`.
    pin::<Ui>("ui::Ui", UI_SIZE, 8),
    // Beside `Ui`, so growth here alone is cache growth and not recorder growth.
    pin::<WindowFrames>("ui::WindowFrames", WINDOW_FRAMES_SIZE, 8),
    pin::<NodeRecord>("scene::NodeRecord", 72, 8),
    pin::<IdEntry>("scene::IdEntry", 56, 8),
    pin::<Option<WidgetId>>("primitives::Option<WidgetId>", 8, 8),
    pin::<LayoutCore>("scene::LayoutCore", 28, 4),
    pin::<NodeFlags>("scene::NodeFlags", 4, 4),
    pin::<LayoutMode>("primitives::LayoutMode", 4, 2),
    pin::<PackedLayoutMeta>("primitives::PackedLayoutMeta", 4, 4),
    pin::<ExtrasIdx>("scene::ExtrasIdx", 12, 4),
    pin::<BoundsExtras>("scene::BoundsExtras", 36, 4),
    pin::<PanelExtras>("scene::PanelExtras", 20, 4),
    pin::<Node>("scene::Node", 104, 4),
    // A rect's shadow and material, 18 and 40 of them, set the row's size.
    pin::<ShapeRecord>("shape::ShapeRecord", 144, 8),
    pin::<RecordedText>("shapes::RecordedText", 16, 8),
    // No material: a node wears one as a shape (`MaterialPaint`).
    pin::<ChromeRow>("scene::ChromeRow", 64, 8),
    pin::<LoweredBorder>("shape::LoweredBorder", 12, 4),
    pin::<LoweredShadow>("shape::LoweredShadow", 18, 2),
    pin::<RecordedGradient>("shapes::RecordedGradient", 56, 4),
    pin::<ResolvedGradient>("payload::ResolvedGradient", 16, 4),
    // A paint and its corners. Every look holds one and every themed widget wears one a frame,
    // so its size is copied through recording and animation: a paint 116 B larger measured
    // record +2.8 to +4.6 % and animation +1 to +26 % (`.notes/PERF.md`, "A paint carries a
    // short effect name"). A material sits in a shape's `MaterialPaint`, out of both.
    pin::<Chrome>("primitives::Chrome", 128, 4),
    pin::<paint::Paint>("primitives::Paint", 120, 4),
    pin::<Border>("primitives::Border", 20, 4),
    pin::<Shadow>("primitives::Shadow", 36, 4),
    // Of which 60 the effect with its inline name, `MaterialEffect`, and 64 the fallback, a brush
    // that holds the fallback's tag in its own.
    pin::<Option<Material>>("primitives::Option<Material>", 144, 4),
    // A material box's paint, what `add_shape` takes for one: a paint and a material.
    pin::<MaterialPaint>("primitives::MaterialPaint", 264, 4),
    pin::<EffectName<(), BackdropEffect>>("primitives::EffectName", 24, 1),
    pin::<MaterialEffect>("primitives::MaterialEffect", 60, 4),
    pin::<Stroke>("primitives::Stroke", 64, 4),
    pin::<ImagePaint>("shape::ImagePaint", 48, 8),
    pin::<IconPaint>("shape::IconPaint", 36, 4),
    // What `add_shape` hands the record store, matched once a shape.
    pin::<Draw<'static>>("shape::Draw", 192, 8),
    pin::<LoweredPaint>("shape::LoweredPaint", 88, 8),
    // A look, its plan and what it wears, each built or copied per themed widget a frame.
    pin::<WidgetLook>("theme::WidgetLook", 236, 4),
    pin::<LookPlan>("theme::LookPlan", 360, 8),
    pin::<AnimatedLook>("theme::AnimatedLook", 220, 4),
    pin::<TextStyle>("theme::TextStyle", 92, 4),
    pin::<FontFamily>("text::FontFamily", 64, 1),
    // Every `show` returns one.
    pin::<Response>("input::Response", 160, 8),
    // Align 2, so embedding either in `QuadInstance` keeps its alignment at 4 and
    // `Pod` free of padding.
    pin::<Spacing>("primitives::Spacing", 8, 2),
    pin::<Corners>("primitives::Corners", 8, 2),
    // Inline in `Brush::Gradient`, so it sets the floor for `Brush`, whose tag sits in the
    // geometry's. Stops: 1 (len) + `MAX_STOPS` * 5; ramp adds 1 (interpolation); gradient adds
    // 20 (the radial's 16 behind a tag padded to align 4), 1 (spread) and 1 tail pad.
    pin::<GradientStops>("brush::GradientStops", 1 + 5 * MAX_STOPS, 1),
    pin::<ColorRamp>("brush::ColorRamp", 1 + 5 * MAX_STOPS + 1, 1),
    pin::<Gradient>("brush::Gradient", 64, 4),
    pin::<Brush>("primitives::Brush", 64, 4),
    pin::<Span>("common::Span", 8, 4),
    pin::<Button<'static>>("widgets::Button", 168, 8),
    pin::<Checkbox<'static>>("widgets::Checkbox", 168, 8),
    pin::<Switch<'static>>("widgets::Switch", 168, 8),
    pin::<
        ComboBox<
            'static,
            &'static str,
            std::iter::Copied<std::slice::Iter<'static, &'static str>>,
            for<'a> fn(&'a &'static str) -> &'a str,
        >,
    >("widgets::ComboBox", 184, 8),
    pin::<DragValue<'static>>("widgets::DragValue", 232, 8),
    pin::<RadioGroup<'static, u8>>("widgets::RadioGroup<u8>", 152, 8),
    pin::<TextEdit<'static>>("widgets::TextEdit", 320, 8),
    pin::<Text<'static>>("widgets::Text", 272, 8),
    pin::<Slider<'static>>("widgets::Slider", 192, 8),
    pin::<ProgressBar<'static>>("widgets::ProgressBar", 144, 8),
    pin::<Splitter<'static>>("widgets::Splitter", 160, 8),
    // 116 of them the chrome's material.
    pin::<Panel>("widgets::Panel", 256, 8),
    pin::<Block>("widgets::Block", 256, 8),
    pin::<Grid>("widgets::Grid", 256, 8),
    pin::<Scroll<'static>>("widgets::Scroll", 312, 8),
    pin::<Separator<'static>>("widgets::Separator", 160, 8),
    pin::<Spinner<'static>>("widgets::Spinner", 168, 8),
    pin::<Popup<'static>>("widgets::Popup", 440, 8),
    pin::<Modal<'static>>("widgets::Modal", 400, 8),
    pin::<Tooltip<'static>>("widgets::Tooltip", 472, 8),
    pin::<GpuView>("widgets::GpuView", 152, 8),
    pin::<Menu<'static>>("widgets::Menu", 520, 8),
    pin::<MenuItem<'static>>("widgets::MenuItem", 176, 8),
    pin::<ShapedText>("layout::ShapedText", 40, 8),
    pin::<TextShapeKey>("text::TextShapeKey", 24, 8),
    pin::<MeasureSnapshot>("layout::MeasureSnapshot", 392, 8),
    pin::<AnimationRow<AnimatedLook>>("animation::AnimationRow<AnimatedLook>", 256, 8),
    pin::<ContentHash>("common::ContentHash", 8, 8),
    pin::<CascadeInputHash>("cascade::CascadeInputHash", 8, 8),
    pin::<EntryRow>("cascade::EntryRow", 32, 4),
    pin::<HitRow>("cascade::HitRow", 32, 8),
    pin::<Paint>("cascade::Paint", 24, 8),
    pin::<ResponseState>("input::ResponseState", 152, 4),
    pin::<Widget>("widget_core::Widget", 128, 8),
    pin::<TargetScrollDelta>("input::TargetScrollDelta", 48, 8),
    pin::<DamageRegion>("damage::DamageRegion", 132, 4),
    pin::<CollapsedDamage>("damage::CollapsedDamage", 136, 4),
    pin::<NodeSnapshot>("damage::node_snapshot::NodeSnapshot", 40, 8),
    pin::<PushClipPayload>("payload::PushClipPayload", 24, 4),
    pin::<DrawClosedPayload<ClosedFill>>("payload::DrawClosedPayload<ClosedFill>", 84, 4),
    pin::<DrawShadowPayload>("payload::DrawShadowPayload", 60, 4),
    pin::<DrawTextPayload>("payload::DrawTextPayload", 80, 8),
    pin::<DrawStrokePayload>("payload::DrawStrokePayload", 104, 4),
    pin::<DrawMeshPayload>("payload::DrawMeshPayload", 80, 4),
    pin::<DrawClosedPayload<ClosedImage>>("payload::DrawClosedPayload<ClosedImage>", 120, 8),
    pin::<DrawIconPayload>("payload::DrawIconPayload", 48, 4),
    pin::<QuadInstance>("renderer::QuadInstance", 104, 4),
    pin::<CurveInstance>("renderer::CurveInstance", 80, 4),
    pin::<MeshInstance>("renderer::MeshInstance", 36, 4),
    // 4 of them a halo's reach.
    pin::<EffectInstance>("renderer::EffectInstance", 100, 4),
    pin::<ImageInstance>("renderer::ImageInstance", 112, 4),
    pin::<MeshVertex>("primitives::MeshVertex", 12, 4),
    pin::<RasterQuad>("raster_pipeline::RasterQuad", 36, 4),
    pin::<SpunRasterQuad>("raster_pipeline::SpunRasterQuad", 52, 4),
    pin::<PlacedGlyph>("text::PlacedGlyph", 32, 4),
    pin::<ShapedTextRef>("text::ShapedTextRef", 32, 8),
    pin::<TextDrawRow>("renderer::TextDrawRow", 96, 8),
    pin::<IconDrawRow>("renderer::IconDrawRow", 56, 4),
    pin::<ImageDrawRow>("renderer::ImageDrawRow", 128, 8),
    pin::<MeshDrawRow>("renderer::MeshDrawRow", 52, 4),
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
