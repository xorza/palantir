//! Text owned by a container rather than a leaf: paint-only, ordered, and
//! cached alongside its children.

use crate::TextStyle;
use crate::Ui;
use crate::layout::cross_driver_tests::text_wrap::support::PARAGRAPH;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::color::Color;
use crate::primitives::size::Size;
use crate::renderer::frontend::capture::PaintCall;
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::scene::visibility::Visibility;
use crate::shape::Shape;
use crate::text::font_family::FontFamily;
use crate::text::font_weight::FontWeight;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;
use crate::widgets::text::Text;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::UVec2;

#[test]
fn container_text_is_paint_only_and_wraps_to_final_inner_width() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let scene = h.frame_value(build_wrapping_container_text);
    let layout = h.ui.layout(Layer::Main);
    assert_eq!(layout.text_shapes.len(), 1);
    let container_rect = layout.rect[scene.container.idx()];
    let child_rect = layout.rect[scene.child.idx()];

    assert_eq!(child_rect.size, Size::new(80.0, 20.0));
    assert_eq!(container_rect.size, Size::new(100.0, 40.0));

    let span = layout.text_spans[scene.container.idx()];
    assert_eq!(span.len, 1, "container owns one direct text run");
    let shaped = layout.text_shapes[span.start as usize];
    assert_eq!(shaped.measured, Size::new(73.0, 80.0));

    let draw_keys: Vec<_> = h
        .encode_paint()
        .calls
        .iter()
        .filter_map(|command| match command {
            PaintCall::Text(payload) => Some(payload.text.key),
            _ => None,
        })
        .collect();
    assert_eq!(draw_keys, [shaped.buffer_key()]);
    let leaf = h.frame_value(|ui| Text::new("leaf-only").show(ui).node());
    let layout = h.ui.layout(Layer::Main);
    assert_eq!(layout.text_shapes.len(), 1);
    assert_eq!(layout.text_spans[leaf.idx()].len, 1);
}

#[test]
fn container_text_visibility_distinguishes_hidden_from_collapsed() {
    let surface = UVec2::new(400, 400);
    let mut h = UiHarness::with_text(surface);
    let hidden_node =
        h.frame_value(|ui| build_container_text_with_visibility(ui, Visibility::Hidden));
    let hidden_layout = h.ui.layout(Layer::Main);
    assert_eq!(
        hidden_layout.rect[hidden_node.idx()].size,
        Size::new(100.0, 100.0),
    );
    assert_eq!(hidden_layout.text_spans[hidden_node.idx()].len, 0);
    assert!(hidden_layout.text_shapes.is_empty());

    let collapsed_node =
        h.frame_value(|ui| build_container_text_with_visibility(ui, Visibility::Collapsed));
    let collapsed_layout = h.ui.layout(Layer::Main);
    assert_eq!(collapsed_layout.rect[collapsed_node.idx()].size, Size::ZERO);
    assert_eq!(collapsed_layout.text_spans[collapsed_node.idx()].len, 0);
    assert!(collapsed_layout.text_shapes.is_empty());

    let visible_node =
        h.frame_value(|ui| build_container_text_with_visibility(ui, Visibility::Visible));
    let visible_layout = h.ui.layout(Layer::Main);
    assert_eq!(
        visible_layout.rect[visible_node.idx()].size,
        Size::new(100.0, 100.0),
    );
    let span = visible_layout.text_spans[visible_node.idx()];
    assert_eq!(span.len, 1);
    assert_eq!(
        visible_layout.text_shapes[span.start as usize].measured,
        Size::new(73.0, 80.0),
    );

    // The same two states one level up: a `Visible` owner under them
    // shapes nothing either, because the worklist carries the cascade.
    for (ancestor, owner_size) in [
        (Visibility::Hidden, Size::new(100.0, 100.0)),
        (Visibility::Collapsed, Size::ZERO),
    ] {
        let owner = h.frame_value(|ui| {
            Panel::vstack()
                .id_salt("container-text-ancestor")
                .size((Sizing::fixed(200.0), Sizing::fixed(200.0)))
                .visibility(ancestor)
                .show(ui, |ui| {
                    build_container_text_with_visibility(ui, Visibility::Visible)
                })
                .inner
        });
        let layout = h.ui.layout(Layer::Main);
        assert_eq!(
            layout.rect[owner.idx()].size,
            owner_size,
            "{ancestor:?} ancestor decides whether the owner keeps a slot",
        );
        assert_eq!(
            layout.text_spans[owner.idx()].len,
            0,
            "{ancestor:?} ancestor leaves the owner's span empty",
        );
        assert!(
            layout.text_shapes.is_empty(),
            "a {ancestor:?} ancestor leaves the paint-only run unshaped",
        );
    }
}

#[test]
fn container_and_child_text_keep_independent_order_across_cache_hit() {
    let mut h = UiHarness::with_text(UVec2::new(400, 400));
    let first_scene = h.frame_value(build_interleaved_container_text);
    let first_layout = h.ui.layout(Layer::Main);
    assert_eq!(first_layout.text_shapes.len(), 3);
    let first_parent_span = first_layout.text_spans[first_scene.container.idx()];
    let first_child_span = first_layout.text_spans[first_scene.child.idx()];
    assert_eq!(first_parent_span.len, 2);
    assert_eq!(first_child_span.len, 1);
    let first_parent_keys = [
        first_layout.text_shapes[first_parent_span.start as usize].buffer_key(),
        first_layout.text_shapes[(first_parent_span.start + 1) as usize].buffer_key(),
    ];
    let first_child_key = first_layout.text_shapes[first_child_span.start as usize].buffer_key();
    assert_ne!(first_parent_keys[0], first_child_key);
    assert_ne!(first_parent_keys[1], first_child_key);
    let first_draw_keys: Vec<_> = h
        .encode_paint()
        .calls
        .iter()
        .filter_map(|command| match command {
            PaintCall::Text(payload) => Some(payload.text.key),
            _ => None,
        })
        .collect();
    assert_eq!(
        first_draw_keys,
        [first_parent_keys[0], first_child_key, first_parent_keys[1]],
    );
    let second_scene = h.frame_value(build_interleaved_container_text);
    assert!(
        !h.engines.layout.scratch.counters.cache_hits().is_empty(),
        "second identical frame should exercise measure-cache replay",
    );
    let second_layout = h.ui.layout(Layer::Main);
    assert_eq!(second_layout.text_shapes.len(), 3);
    let second_parent_span = second_layout.text_spans[second_scene.container.idx()];
    let second_child_span = second_layout.text_spans[second_scene.child.idx()];
    assert_eq!(second_parent_span.len, 2);
    assert_eq!(second_child_span.len, 1);
    let second_parent_keys = [
        second_layout.text_shapes[second_parent_span.start as usize].buffer_key(),
        second_layout.text_shapes[(second_parent_span.start + 1) as usize].buffer_key(),
    ];
    let second_child_key = second_layout.text_shapes[second_child_span.start as usize].buffer_key();
    assert_eq!(second_parent_keys, first_parent_keys);
    assert_eq!(second_child_key, first_child_key);
    let second_draw_keys: Vec<_> = h
        .encode_paint()
        .calls
        .iter()
        .filter_map(|command| match command {
            PaintCall::Text(payload) => Some(payload.text.key),
            _ => None,
        })
        .collect();
    assert_eq!(
        second_draw_keys,
        [
            second_parent_keys[0],
            second_child_key,
            second_parent_keys[1],
        ],
    );
}

fn build_wrapping_container_text(ui: &mut Ui) -> ContainerTextScene {
    let mut child = None;
    let container = Panel::vstack()
        .id_salt("wrapping-container-text")
        .size((Sizing::HUG, Sizing::HUG))
        .padding(10.0)
        .show(ui, |ui| {
            add_direct_text(ui, PARAGRAPH, 14.0, 16.0, TextWrap::Wrap, None);
            child = Some(
                Frame::new()
                    .id_salt("container-size-driver")
                    .size((Sizing::fixed(80.0), Sizing::fixed(20.0)))
                    .show(ui)
                    .node(),
            );
        })
        .response
        .node();
    ContainerTextScene {
        container,
        child: child.unwrap(),
    }
}

fn build_interleaved_container_text(ui: &mut Ui) -> ContainerTextScene {
    let mut child = None;
    let container = Panel::vstack()
        .id_salt("interleaved-container-text")
        .size((Sizing::fixed(240.0), Sizing::fixed(100.0)))
        .show(ui, |ui| {
            add_direct_text(
                ui,
                "parent-before",
                12.0,
                14.0,
                TextWrap::SingleLine,
                Some(glam::Vec2::new(0.0, 0.0)),
            );
            child = Some(
                Text::new("child-between")
                    .id_salt("interleaved-child")
                    .style(&TextStyle::default().with_font_size(18.0))
                    .show(ui)
                    .node(),
            );
            add_direct_text(
                ui,
                "parent-after-is-longer",
                14.0,
                16.0,
                TextWrap::SingleLine,
                Some(glam::Vec2::new(0.0, 60.0)),
            );
        })
        .response
        .node();
    ContainerTextScene {
        container,
        child: child.unwrap(),
    }
}

fn build_container_text_with_visibility(ui: &mut Ui, visibility: Visibility) -> NodeId {
    Panel::vstack()
        .id_salt("container-text-visibility")
        .size((Sizing::fixed(100.0), Sizing::fixed(100.0)))
        .padding(10.0)
        .visibility(visibility)
        .show(ui, |ui| {
            add_direct_text(ui, PARAGRAPH, 14.0, 16.0, TextWrap::Wrap, None);
        })
        .response
        .node()
}

fn add_direct_text(
    ui: &mut Ui,
    text: &'static str,
    font_size_px: f32,
    line_height_px: f32,
    wrap: TextWrap,
    local_origin: Option<glam::Vec2>,
) {
    let text = ui.intern(text);
    let shape = Shape::text(
        text,
        GlyphFont {
            line_height_px,
            ..GlyphFont::new(font_size_px)
        },
    )
    .color(Color::WHITE)
    .wrap(wrap)
    .align(Align::default())
    .family(FontFamily::SANS)
    .weight(FontWeight::REGULAR);
    ui.add_shape(match local_origin {
        Some(origin) => shape.at_origin(origin),
        None => shape,
    });
}

#[derive(Debug)]
struct ContainerTextScene {
    container: NodeId,
    child: NodeId,
}
