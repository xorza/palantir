//! Several runs on one leaf: shaped apart, emitted apart, and cached apart.

use crate::Ui;
use crate::WidgetId;
use crate::layout::cross_driver_tests::support::chat_message;
use crate::layout::cross_driver_tests::text_wrap::support::PARAGRAPH;
use crate::layout::types::align::Align;
use crate::primitives::color::RgbaF32;
use crate::renderer::frontend::capture::PaintCall;
use crate::scene::layer::Layer;
use crate::scene::node::Node;
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::text::font_family::FontFamily;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;
use glam::UVec2;

/// Pin: a custom widget that pushes two `ShapeRecord::Text` to the same
/// node has both runs shaped (`text_spans[node].len == 2`) at distinct
/// `TextShapeKey`s (no identity-reuse collision). Replaces the
/// old "one ShapeRecord::Text per leaf" hard assert.
#[test]
fn multi_shape_text_per_leaf_shapes_each_run_independently() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let leaf = h.frame_value(build_multi_text_leaf);
    let span = h.ui.layout(Layer::Main).text_spans[leaf.idx()];
    assert_eq!(
        span.len, 2,
        "leaf with two ShapeRecord::Text should record two text-shape entries"
    );
    let first = h.ui.layout(Layer::Main).text_shapes[span.start as usize];
    let second = h.ui.layout(Layer::Main).text_shapes[(span.start + 1) as usize];
    assert!(
        first.measured.w > 0.0 && second.measured.w > 0.0,
        "both runs must have measured nonzero width: first={:?} second={:?}",
        first.measured,
        second.measured,
    );
    assert!(
        second.measured.w > first.measured.w,
        "second run is longer text and should measure wider; first={} second={}",
        first.measured.w,
        second.measured.w,
    );
    assert_ne!(
        first.key, second.key,
        "different text inputs must produce distinct TextShapeKeys — \
       a collision would mean the second shape clobbered the first's cache slot",
    );
}

/// Pin: encoder emits one `DrawText` per `ShapeRecord::Text` in record
/// order, and `local_rect: Some(lr)` shifts the emitted rect by
/// `lr.min` (relative to the owner). Without per-shape `text_ordinal`
/// indexing or the `local_rect` branch, the second run would either
/// re-paint the first's shaped buffer or sit on top of the first.
#[test]
fn multi_shape_text_per_leaf_emits_one_drawtext_per_run_at_local_rect() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let leaf = h.frame_value(build_multi_text_leaf);
    let owner_min = h.ui.arranged_rect(Layer::Main, leaf).min;
    let cmds = h.encode_paint();
    let mut drawn: Vec<glam::Vec2> = cmds
        .calls
        .iter()
        .filter_map(|command| match command {
            PaintCall::Text(payload) => Some(payload.rect.min),
            _ => None,
        })
        .collect();
    assert_eq!(
        drawn.len(),
        2,
        "leaf with two ShapeRecord::Text must emit two DrawText cmds; got {drawn:?}"
    );
    drawn.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
    let [low, high] = [drawn[0], drawn[1]];
    // Slot 0 (`local_rect.min = (0, 0)`) → DrawText.min == owner.min.
    assert!(
        (low.y - owner_min.y).abs() < 0.5,
        "slot 0 with local_rect=(0,0) should emit at owner_min.y; \
       owner_min={owner_min:?} low={low:?}",
    );
    // Slot 1 (`local_rect.min = (0, 22)`) → DrawText.min.y == owner.min.y + 22.
    assert!(
        (high.y - (owner_min.y + 22.0)).abs() < 0.5,
        "slot 1 with local_rect.y=22 should emit shifted by 22 from owner_min.y; \
       owner_min={owner_min:?} high={high:?}",
    );
    // Distinct y proves the two emissions are not aliased.
    assert!(
        (high.y - low.y).abs() >= 20.0,
        "two DrawText must paint at distinct y; got {low:?} {high:?}",
    );
}

/// Pin: the cross-frame measure cache replays multi-text leaves
/// correctly. Frame 1 populates the cache; frame 2 hits and rebases
/// the snapshot's subtree-local spans + flat text-shapes back into
/// the per-frame buffer. Without correct rebase (e.g. forgetting
/// `dest_start += text_shapes.len()` or storing global indices in
/// the snapshot), frame 2 would either read from the wrong slot or
/// see stale `TextShapeKey`s.
#[test]
fn multi_shape_text_per_leaf_round_trips_through_measure_cache() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let f1_leaf = h.frame_value(build_multi_text_leaf);
    let f1_span = h.ui.layout(Layer::Main).text_spans[f1_leaf.idx()];
    let f1_first = h.ui.layout(Layer::Main).text_shapes[f1_span.start as usize];
    let f1_second = h.ui.layout(Layer::Main).text_shapes[(f1_span.start + 1) as usize];
    let f2_leaf = h.frame_value(build_multi_text_leaf);
    let f2_span = h.ui.layout(Layer::Main).text_spans[f2_leaf.idx()];
    assert_eq!(f2_span.len, 2, "frame 2 must restore both text-shape slots");
    let f2_first = h.ui.layout(Layer::Main).text_shapes[f2_span.start as usize];
    let f2_second = h.ui.layout(Layer::Main).text_shapes[(f2_span.start + 1) as usize];

    assert_eq!(
        (f1_first.key, f1_second.key),
        (f2_first.key, f2_second.key),
        "cache hit must replay the exact same TextShapeKeys per slot",
    );
    assert!(
        (f1_first.measured.w - f2_first.measured.w).abs() < 0.01
            && (f1_second.measured.w - f2_second.measured.w).abs() < 0.01,
        "cache hit must replay the exact same measured sizes per slot; \
     f1=({:?}, {:?}) f2=({:?}, {:?})",
        f1_first.measured,
        f1_second.measured,
        f2_first.measured,
        f2_second.measured,
    );
}

/// A `Fill` text child costs one bounded reshape per frame of a resize
/// drag, and the drag stays bounded — through the real layout stack, not
/// just `TextSystem` in isolation.
///
/// Two things could break quietly here. `WrapSlot` caches exactly one
/// width-bounded resolve per reuse row, so a driver that measured one
/// node at two widths in a frame would evict it twice over and
/// `supersede` a buffer it was about to reuse; no driver does that today
/// (stacks take `intrinsic` for sizing then `measure` once at the
/// resolved share, and grid does the same per cell), and the 1-shape
/// count below is what says so. And `supersede` is the only signal that
/// makes the probation window reachable, so if the reuse row ever stopped
/// surviving a drag frame, retention would silently fall back to the
/// 120-frame protected window.
#[test]
fn a_resize_drag_costs_one_reshape_a_frame_and_stays_bounded() {
    let mut h = UiHarness::with_text(UVec2::new(200, 400));
    // Frame 0 shapes the unbounded root and the first bounded resolve.
    h.frame_value(|ui| chat_message(ui, 40.0, PARAGRAPH, 14.0));

    // Redrawing at the same width reshapes nothing: the layout measure
    // cache short-circuits the subtree entirely, so `TextSystem` is never
    // even asked.
    let before = h.ui.shaper().cache_counts();
    h.frame_value(|ui| chat_message(ui, 40.0, PARAGRAPH, 14.0));
    let steady = h.ui.shaper().cache_counts() - before;
    assert_eq!(steady.shapes, 0, "a steady frame must not reshape");
    assert_eq!(steady.supersedes, 0, "nor demote the buffer still in use");

    // Now drag the share. Every frame commits a fresh whole-pixel width.
    //
    // Every changed frame demotes the width it replaced, including the
    // first one after the still frames above — reuse rows outlive a frame
    // they were not measured in, so the wrap slot is still there to say
    // which key to supersede.
    let mut shapes = 0;
    let mut supersedes = 0;
    for frame in 0..12 {
        let before = h.ui.shaper().cache_counts();
        h.frame_value(|ui| chat_message(ui, 40.0 + frame as f32 * 3.0, PARAGRAPH, 14.0));
        let d = h.ui.shaper().cache_counts() - before;
        // The unbounded root is shaped once for the whole drag; only the
        // bounded resolve moves. More than one means a driver measured
        // this node at two widths in the same frame, which would also
        // thrash the single-slot `WrapSlot`.
        assert!(
            d.shapes <= 1,
            "drag frame {frame} reshaped {} times, so some driver committed \
             two widths to one node in one frame",
            d.shapes,
        );
        shapes += d.shapes;
        supersedes += d.supersedes;
    }
    assert!(
        shapes >= 10,
        "the drag must actually be reshaping ({shapes})"
    );
    assert_eq!(
        supersedes, shapes,
        "every replaced width must be demoted, or the drag ages on the \
         protected window",
    );

    // Twelve distinct widths, but retention tracks the probation window.
    let resident = h.ui.shaper().cosmic_cache_len();
    assert!(
        resident <= 8,
        "drag retained {resident} buffers for one run; supersession is not \
         reaching them through the layout stack",
    );
}

/// Two `ShapeRecord::Text` runs in one leaf:
///   slot 0: "first" at `local_rect: Some((0, 0)+100x20)`,
///   slot 1: "second-with-different-text" at `Some((0, 22)+100x20)`.
/// Returns the leaf NodeId so callers can read `text_spans` /
/// emitted commands. Used by the multi-text-per-leaf pinning tests.
fn build_multi_text_leaf(ui: &mut Ui) -> NodeId {
    let leaf_id = WidgetId::from_hash("multi-text-leaf");
    Panel::vstack().auto_id().show(ui, |ui| {
        let node = Node::leaf().id(leaf_id);
        ui.widget(node).record(ui, None, |ui| {
            let first = ui.intern("first");
            ui.add_shape(
                Shape::text(
                    first,
                    GlyphFont {
                        line_height_px: 16.0,
                        ..GlyphFont::new(14.0)
                    },
                )
                .at_origin(glam::Vec2::new(0.0, 0.0))
                .color(RgbaF32::WHITE)
                .wrap(TextWrap::Truncate)
                .align(Align::default())
                .family(FontFamily::SANS)
                .weight(FontWeight::REGULAR),
            );
            ui.add_shape(
                Shape::rect(crate::Rect::new(0.0, 20.0, 4.0, 2.0))
                    .corners(crate::Corners::ZERO)
                    .fill(RgbaF32::WHITE)
                    .stroke(crate::Stroke::ZERO),
            );
            let second = ui.intern("second-with-different-text");
            ui.add_shape(
                Shape::text(
                    second,
                    GlyphFont {
                        line_height_px: 16.0,
                        ..GlyphFont::new(14.0)
                    },
                )
                .at_origin(glam::Vec2::new(0.0, 22.0))
                .color(RgbaF32::WHITE)
                .wrap(TextWrap::Truncate)
                .align(Align::default())
                .family(FontFamily::SANS)
                .weight(FontWeight::REGULAR),
            );
        });
    });
    ui.forest().node_for_widget_id(Layer::Main, leaf_id)
}
