//! Text owned by a container rather than a leaf: paint-only, ordered, and
//! cached alongside its children.

use crate::host::harness::Harness;
use crate::internals::paint_capture::PaintCall;
use crate::layout::tests::support::PARAGRAPH;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::Align;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::paint::color::RgbaF32;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widgets::text::Text;
use crate::widgets::{block::Block, panel::Panel};
use glam::UVec2;

#[test]
fn container_text_is_paint_only_and_wraps_to_final_inner_width() {
    let mut h = Harness::new(UVec2::new(400, 400));
    let scene = h.frame_value(build_wrapping_container_text);
    let layout = h.ui().layout(Layer::Main);
    assert_eq!(layout.text_shapes.len(), 1);
    let container_rect = layout.rect[scene.container.idx()];
    let child_rect = layout.rect[scene.child.idx()];

    assert_eq!(child_rect.size, Size::new(80.0, 20.0));
    assert_eq!(container_rect.size, Size::new(100.0, 40.0));

    let span = layout.text_spans[scene.container.idx()];
    assert_eq!(span.len, 1, "container owns one direct text run");
    let shaped = layout.text_shapes[span.start as usize];
    assert_eq!(shaped.extent.size, Size::new(73.0, 80.0));

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
    let leaf = h.frame_value(|ui| Text::new("leaf-only").show(ui).node(ui));
    let layout = h.ui().layout(Layer::Main);
    assert_eq!(layout.text_shapes.len(), 1);
    assert_eq!(layout.text_spans[leaf.idx()].len, 1);
}

/// A container of a measure-hit subtree arranged at the width its runs were shaped at copies them
/// forward; arranged at another width, it shapes them again. At 400 the paragraph keeps one line;
/// at 150 it wraps, so the second shape is taller.
#[test]
fn container_text_replays_at_its_width_and_reshapes_at_another() {
    let mut h = Harness::new(UVec2::new(400, 400));
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id_salt("fill-container-text")
            .size((Sizing::FILL, Sizing::HUG))
            .padding(10.0)
            .show(ui, |ui| {
                DirectText::PARAGRAPH.add(ui);
                Block::new()
                    .id_salt("fill-container-child")
                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                    .show(ui);
            })
            .response
            .node(ui)
    };
    let run_of = |h: &Harness, node: NodeId| {
        let layout = h.ui().layout(Layer::Main);
        let span = layout.text_spans[node.idx()];
        assert_eq!(span.len, 1, "the container owns one run");
        let shaped = layout.text_shapes[span.start as usize];
        (shaped.extent.size, shaped.buffer_key())
    };
    let replays = |h: &mut Harness| {
        h.frames_mut()
            .layout
            .scratch
            .counters
            .arrange_replays()
            .container_text
    };
    let node = h.frame_value(build);
    let first = run_of(&h, node);

    h.frames_mut().layout.forget_last_run();
    let node = h.frame_value(build);
    assert_eq!(replays(&mut h), 1, "the same width replays the run");
    assert_eq!(run_of(&h, node), first);

    h.resize(UVec2::new(150, 400));
    let node = h.frame_value(build);
    assert_eq!(replays(&mut h), 0, "another width shapes the run again");
    let (narrow, _) = run_of(&h, node);
    assert!(
        narrow.h > first.0.h,
        "the narrow run wraps: {narrow:?} vs {:?}",
        first.0
    );
}

/// A container under a hidden ancestor shapes no runs, yet its subtree measures and is cached;
/// once the ancestor shows, the subtree hits the cache at the same width, and the container
/// shapes its run rather than replaying the none it had.
#[test]
fn container_text_under_a_shown_ancestor_shapes_what_it_did_not_replay() {
    let mut h = Harness::new(UVec2::new(400, 400));
    let build = |visibility: Visibility| {
        move |ui: &mut Ui| {
            let mut inner = None;
            Panel::vstack()
                .id_salt("hiding-ancestor")
                .visibility(visibility)
                .show(ui, |ui| {
                    inner = Some(
                        Panel::vstack()
                            .id_salt("shown-container")
                            .padding(10.0)
                            .show(ui, |ui| {
                                DirectText::PARAGRAPH.add(ui);
                                Block::new()
                                    .id_salt("shown-child")
                                    .size((Sizing::fixed(20.0), Sizing::fixed(20.0)))
                                    .show(ui);
                            })
                            .response
                            .node(ui),
                    );
                });
            inner.unwrap()
        }
    };
    let node = h.frame_value(build(Visibility::Hidden));
    assert_eq!(h.ui().layout(Layer::Main).text_spans[node.idx()].len, 0);
    let node = h.frame_value(build(Visibility::Visible));
    let container = h.ui().tree(Layer::Main).records.widget_id()[node.idx()];
    assert!(
        h.frames_mut().layout.cache_hits().contains(&container),
        "fixture: the container's subtree hits the cache"
    );
    assert_eq!(
        h.ui().layout(Layer::Main).text_spans[node.idx()].len,
        1,
        "the shown container shapes its run"
    );
}

#[test]
fn container_text_visibility_distinguishes_hidden_from_collapsed() {
    let surface = UVec2::new(400, 400);
    let mut h = Harness::new(surface);
    let hidden_node =
        h.frame_value(|ui| build_container_text_with_visibility(ui, Visibility::Hidden));
    let hidden_layout = h.ui().layout(Layer::Main);
    assert_eq!(
        hidden_layout.rect[hidden_node.idx()].size,
        Size::new(100.0, 100.0),
    );
    assert_eq!(hidden_layout.text_spans[hidden_node.idx()].len, 0);
    assert!(hidden_layout.text_shapes.is_empty());

    let collapsed_node =
        h.frame_value(|ui| build_container_text_with_visibility(ui, Visibility::Collapsed));
    let collapsed_layout = h.ui().layout(Layer::Main);
    assert_eq!(collapsed_layout.rect[collapsed_node.idx()].size, Size::ZERO);
    assert_eq!(collapsed_layout.text_spans[collapsed_node.idx()].len, 0);
    assert!(collapsed_layout.text_shapes.is_empty());

    let visible_node =
        h.frame_value(|ui| build_container_text_with_visibility(ui, Visibility::Visible));
    let visible_layout = h.ui().layout(Layer::Main);
    assert_eq!(
        visible_layout.rect[visible_node.idx()].size,
        Size::new(100.0, 100.0),
    );
    let span = visible_layout.text_spans[visible_node.idx()];
    assert_eq!(span.len, 1);
    assert_eq!(
        visible_layout.text_shapes[span.start as usize].extent.size,
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
        let layout = h.ui().layout(Layer::Main);
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
    let mut h = Harness::new(UVec2::new(400, 400));
    let first_scene = h.frame_value(build_interleaved_container_text);
    let first_layout = h.ui().layout(Layer::Main);
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
    h.frames_mut().layout.forget_last_run();
    let second_scene = h.frame_value(build_interleaved_container_text);
    assert!(
        !h.frames_mut()
            .layout
            .scratch
            .counters
            .cache_hits()
            .is_empty(),
        "second identical frame should exercise measure-cache replay",
    );
    let second_layout = h.ui().layout(Layer::Main);
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
            DirectText::PARAGRAPH.add(ui);
            child = Some(
                Block::new()
                    .id_salt("container-size-driver")
                    .size((Sizing::fixed(80.0), Sizing::fixed(20.0)))
                    .show(ui)
                    .node(ui),
            );
        })
        .response
        .node(ui);
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
            DirectText {
                text: "parent-before",
                font_size: 12.0,
                line_height: 14.0,
                wrap: TextWrap::SingleLine,
                local_origin: Some(glam::Vec2::new(0.0, 0.0)),
            }
            .add(ui);
            child = Some(
                Text::new("child-between")
                    .id_salt("interleaved-child")
                    .font_size(18.0)
                    .show(ui)
                    .node(ui),
            );
            DirectText {
                text: "parent-after-is-longer",
                font_size: 14.0,
                line_height: 16.0,
                wrap: TextWrap::SingleLine,
                local_origin: Some(glam::Vec2::new(0.0, 60.0)),
            }
            .add(ui);
        })
        .response
        .node(ui);
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
            DirectText::PARAGRAPH.add(ui);
        })
        .response
        .node(ui)
}

/// One text shape recorded straight on the open node, at `local_origin` or the default.
#[derive(Clone, Copy, Debug)]
struct DirectText {
    text: &'static str,
    font_size: f32,
    line_height: f32,
    wrap: TextWrap,
    local_origin: Option<glam::Vec2>,
}

impl DirectText {
    /// A wrapping paragraph at 14 px over 16 px lines, at the default origin.
    const PARAGRAPH: Self = Self {
        text: PARAGRAPH,
        font_size: 14.0,
        line_height: 16.0,
        wrap: TextWrap::Wrap,
        local_origin: None,
    };

    fn add(self, ui: &mut Ui) {
        let text = ui.intern(self.text);
        let shape = Shape::text(
            text,
            GlyphFont {
                line_height: self.line_height,
                ..GlyphFont::new(self.font_size)
            },
        )
        .color(RgbaF32::WHITE)
        .wrap(self.wrap)
        .align(Align::default());
        ui.add_shape(match self.local_origin {
            Some(origin) => shape.at_origin(origin),
            None => shape,
        });
    }
}

#[derive(Debug)]
struct ContainerTextScene {
    container: NodeId,
    child: NodeId,
}
