//! What one node redrawing damages, shape by shape.

use crate::Ui;
use crate::damage::Damage;
use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame, one_frame};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::text::TEXT_SCALE_STEP;
use crate::text::glyph_font::GlyphFont;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::{UVec2, Vec2};

/// The first frame has no `prev_frame`, so every painting node is added; the non-painting root stays out of `dirty`/`region`.
#[test]
fn first_frame_marks_every_painting_node_dirty() {
    let mut h = UiHarness::cold(DISPLAY.physical);
    frame(&mut h, |ui| {
        one_frame(ui, BLUE);
    });
    let painting = h.ui.cascade().layers[Layer::Main]
        .paint_arena
        .node_spans
        .iter()
        .filter(|s| s.len > 0)
        .count();
    assert_eq!(h.engines.damage.counters.dirty().len(), painting);
    assert!(h.engines.damage.raw_rects.is_empty());
}

/// Re-recording identical authoring gives zero dirty nodes and no damage rect.
#[test]
fn unchanged_authoring_produces_no_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };
    frame(&mut h, build);
    frame(&mut h, build);

    assert!(h.engines.damage.counters.dirty().is_empty());
    assert!(h.damage_region().is_empty());
    assert_eq!(Damage::new(h.collapsed_damage()), None);
}

/// An authoring change on one leaf dirties just that leaf; the unchanged parent stays clean.
#[test]
fn fill_change_marks_only_the_changed_leaf() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut h, |ui| {
        one_frame(ui, RED);
    });

    assert_eq!(h.engines.damage.counters.dirty().len(), 1);
    let dirty_id = h.engines.damage.counters.dirty()[0];
    assert_eq!(
        h.ui.tree(Layer::Main).records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("a")
    );
    assert_eq!(
        h.damage_region().iter_rects().next(),
        Some(h.ui.arranged_rect(Layer::Main, dirty_id))
    );
}

/// A sibling reflow shifts downstream rects, so neighbors are dirty by rect comparison.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("a"))
                    .size((Sizing::fixed(a_size), Sizing::fixed(20.0)))
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("b"))
                    .size((Sizing::fixed(30.0), Sizing::fixed(20.0)))
                    .background(Background::fill(RgbaF32::srgb(0.5, 0.5, 0.5)))
                    .show(ui);
            });
    };
    frame(&mut h, |ui| build(50.0, ui));
    frame(&mut h, |ui| build(80.0, ui));

    // `a` changed authoring; `b`'s x shifts 50 -> 80. Both are dirty.
    let dirty_ids: Vec<WidgetId> = h
        .engines
        .damage
        .counters
        .dirty()
        .iter()
        .map(|n| h.ui.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("a")));
    assert!(dirty_ids.contains(&WidgetId::from_hash("b")));
}

/// A widget that disappears contributes its previous rect to damage.
#[test]
fn removed_widget_contributes_prev_rect_to_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("gone"))
                    .label("X")
                    .show(ui);
            });
    });
    let prev_button_rect = h
        .engines
        .damage
        .prev_paint_rect(WidgetId::from_hash("gone"))
        .expect("gone painted last frame");

    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    let rects: Vec<Rect> = h.damage_region().iter_rects().collect();
    assert_eq!(rects, vec![prev_button_rect]);
}

/// An added widget contributes its current rect and lands in the dirty list.
#[test]
fn added_widget_contributes_curr_rect_to_damage() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("new"))
                    .size(50.0)
                    .background(Background::fill(RgbaF32::srgb(0.2, 0.4, 0.8)))
                    .show(ui);
            });
    });

    let dirty_ids: Vec<WidgetId> = h
        .engines
        .damage
        .counters
        .dirty()
        .iter()
        .map(|n| h.ui.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(!h.damage_region().is_empty());
}

/// Hovering a button dirties exactly the button; hit-test lags a frame, so frames settle before the transition is asserted.
#[test]
fn button_hover_damage_covers_only_the_button() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |h: &mut UiHarness, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    *hot = Some(
                        Button::new()
                            .id(WidgetId::from_hash("hot"))
                            .label("Hover me")
                            .show(ui)
                            .node(),
                    );
                    *cold = Some(
                        Button::new()
                            .id(WidgetId::from_hash("cold"))
                            .label("Quiet")
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    h.move_to(Vec2::new(380.0, 380.0));
    build(&mut h, &mut hot_node, &mut cold_node);
    build(&mut h, &mut hot_node, &mut cold_node);
    assert!(
        h.engines.damage.counters.dirty().is_empty(),
        "off-button pointer should reach a no-diff steady state"
    );

    let hot_rect = h.ui.arranged_rect(Layer::Main, hot_node.unwrap());
    let target = hot_rect.min + Vec2::new(5.0, 5.0);

    // `on_input` recomputes hover against the existing hit_index, so the next recording emits the hovered fill.
    h.move_to(target);
    build(&mut h, &mut hot_node, &mut cold_node);

    assert_eq!(
        h.engines.damage.counters.dirty().len(),
        1,
        "only the hovered button should be dirty"
    );
    let dirty_id = h.engines.damage.counters.dirty()[0];
    assert_eq!(
        h.ui.tree(Layer::Main).records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(h.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::expect_partial(Damage::new(h.collapsed_damage())),
        hot_rect.into(),
        "small per-button damage must not trip the full-repaint heuristic",
    );

    build(&mut h, &mut hot_node, &mut cold_node);
    assert!(
        h.engines.damage.counters.dirty().is_empty(),
        "settled hover should produce no further damage"
    );
}

/// Un-hovering is symmetric: damage is the button rect.
#[test]
fn button_unhover_damage_covers_only_the_button() {
    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |h: &mut UiHarness, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        h.frame(|ui| {
            Panel::vstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |ui| {
                    *hot = Some(
                        Button::new()
                            .id(WidgetId::from_hash("hot"))
                            .label("Hover me")
                            .show(ui)
                            .node(),
                    );
                    *cold = Some(
                        Button::new()
                            .id(WidgetId::from_hash("cold"))
                            .label("Quiet")
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(&mut h, &mut hot_node, &mut cold_node);
    let hot_rect = h.ui.arranged_rect(Layer::Main, hot_node.unwrap());
    h.move_to(hot_rect.min + Vec2::new(5.0, 5.0));
    build(&mut h, &mut hot_node, &mut cold_node);
    build(&mut h, &mut hot_node, &mut cold_node);
    assert!(
        h.engines.damage.counters.dirty().is_empty(),
        "settled hover"
    );

    h.move_to(Vec2::new(380.0, 380.0));
    build(&mut h, &mut hot_node, &mut cold_node);
    assert_eq!(h.engines.damage.counters.dirty().len(), 1);
    assert_eq!(
        h.ui.tree(Layer::Main).records.widget_id()[h.engines.damage.counters.dirty()[0].idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(h.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::expect_partial(Damage::new(h.collapsed_damage())),
        hot_rect.into()
    );
}

/// A spinning stroke is damaged against the square it sweeps, matching `StrokeBounds::new`, so `extend_predamaged` covers every angle.
///
/// Fixture: 80x40 owner, line (10,10)-(70,30), pivot (40,20), far endpoint r = sqrt(30^2 + 10^2) = 31.623. A quarter turn puts it at pivot + (-10,30), outside the owner box. The pad is 0.5 half-width plus 0.5 fringe (butt cap, no join): 1 per side. Time zero, so the cascade covers the sweep from the registration alone.
#[test]
fn a_spun_stroke_is_damaged_against_the_square_it_sweeps() {
    use crate::scene::tree::paint_anims::curves;
    use crate::scene::tree::paint_anims::paint_animation::PaintAnimation;
    use crate::scene::tree::paint_anims::paint_animation::PaintRepeat;
    use std::f32::consts::TAU;
    use std::time::Duration;

    let owner_id = WidgetId::from_hash("spin_owner");
    let mut h = UiHarness::cold(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(owner_id)
                    .size((Sizing::fixed(80.0), Sizing::fixed(40.0)))
                    .show(ui, |ui| {
                        ui.add_shape_animated(
                            Shape::polyline(
                                &[Vec2::new(10.0, 10.0), Vec2::new(70.0, 30.0)],
                                Stroke::new(RED, 1.0),
                            ),
                            PaintAnimation::turn(0.0, 1.0)
                                .with_started_at(Duration::ZERO)
                                .with_period(Duration::from_secs_f32(TAU / 1.0))
                                .with_repeat(PaintRepeat::Forever)
                                .with_curve(curves::linear),
                        );
                    });
            });
    });

    let owner = h.rect(owner_id).expect("the owner arranged");
    let node_idx = h.ui.cascade().by_id[&owner_id].node.idx();
    let span = h.ui.cascade().layers[Layer::Main].paint_arena.node_spans[node_idx];
    assert_eq!(span.len, 1, "the owner paints the one animated shape");
    let row = h.ui.cascade().layers[Layer::Main].paint_arena.rows[span.start as usize].screen;

    let pivot = owner.min + Vec2::new(40.0, 20.0);
    let want = Rect::square_about(pivot, 30.0_f32.hypot(10.0)).inflated(1.0);
    assert_eq!(row, want, "the damage is the swept square");
    let quarter_turn = pivot + Vec2::new(-10.0, 30.0);
    assert!(
        row.contains(quarter_turn),
        "damaged {row:?} misses the sweep"
    );
    assert!(
        !owner.contains(quarter_turn),
        "the fixture's sweep must leave the owner box, or it proves nothing"
    );
}

/// `NodeSnapshot.paint_span` has one entry per Paint row (chrome at row 0, then direct shapes), mirroring `Cascade::paint_arenas`.
#[test]
fn node_snapshot_decomposition_matches_cascade() {
    use crate::widget::Shape;
    let mut h = UiHarness::cold(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("multi"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background::fill(BLUE))
            .show(ui, |ui| {
                ui.add_shape(Shape::line(
                    Vec2::new(0.0, 0.0),
                    Vec2::new(10.0, 10.0),
                    Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 1.0),
                ));
                ui.add_shape(Shape::line(
                    Vec2::new(20.0, 20.0),
                    Vec2::new(30.0, 30.0),
                    Stroke::new(RgbaF32::srgb(0.0, 1.0, 0.0), 1.0),
                ));
            });
    });
    let layer = Layer::Main;
    let node_idx = h.ui.cascade().by_id[&WidgetId::from_hash("multi")]
        .node
        .idx();
    let node_span = h.ui.cascade().layers[layer].paint_arena.node_spans[node_idx];
    let layer_paints = &h.ui.cascade().layers[layer].paint_arena.rows;

    let chrome_paint = layer_paints[node_span.start as usize];
    assert!(
        chrome_paint.screen.area() > 0.0,
        "chrome panel must have non-zero chrome rect",
    );

    let snap_paints = h
        .engines
        .damage
        .prev_paint_rows(WidgetId::from_hash("multi"));
    assert_eq!(snap_paints.len(), 3, "chrome + 2 direct shapes ⇒ 3 rows");
    let cascade_paints = &layer_paints[node_span.range()];
    for (ord, p) in snap_paints.iter().enumerate() {
        assert_eq!(
            p.screen, cascade_paints[ord].screen,
            "paint #{ord} rect must match cascade column",
        );
        assert_eq!(
            p.hash, cascade_paints[ord].hash,
            "paint #{ord} hash must match cascade column",
        );
    }

    assert!(h.engines.damage.raw_rects.is_empty());

    let two_lines = |ui: &mut Ui| {
        ui.add_shape(Shape::line(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 10.0),
            Stroke::new(RgbaF32::srgb(1.0, 0.0, 0.0), 1.0),
        ));
        ui.add_shape(Shape::line(
            Vec2::new(20.0, 20.0),
            Vec2::new(30.0, 30.0),
            Stroke::new(RgbaF32::srgb(0.0, 1.0, 0.0), 1.0),
        ));
    };
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("multi"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background::fill(BLUE))
            .show(ui, |ui| two_lines(ui));
        Panel::hstack()
            .id(WidgetId::from_hash("multi2"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background::fill(BLUE))
            .show(ui, |ui| two_lines(ui));
    });
    let snap2_paints = h
        .engines
        .damage
        .prev_paint_rows(WidgetId::from_hash("multi2"));
    assert_eq!(
        h.engines.damage.raw_rects.len(),
        3,
        "incremental Vacant insert pushes one rect per paint row",
    );
    assert_eq!(h.engines.damage.raw_rects[0], snap2_paints[0].screen);
    assert_eq!(h.engines.damage.raw_rects[1], snap2_paints[1].screen);
    assert_eq!(h.engines.damage.raw_rects[2], snap2_paints[2].screen);
}

/// A multi-shape owner with disjoint shapes pushes only the moved shape's rect pair (the darkroom graph pattern).
#[test]
fn per_shape_damage_only_pushes_changed_shapes() {
    use crate::widget::Shape;

    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |moving_y: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::fixed(180.0), Sizing::fixed(180.0)))
            .background(Background::fill(BLUE))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::rect(Rect::new(0.0, 0.0, 20.0, 10.0)).fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(60.0, 0.0, 20.0, 10.0))
                        .fill(RgbaF32::srgb(0.0, 1.0, 0.0)),
                );
                ui.add_shape(
                    Shape::rect(Rect::new(0.0, moving_y, 20.0, 10.0))
                        .fill(RgbaF32::srgb(0.0, 0.0, 1.0)),
                );
            });
    };

    frame(&mut h, |ui| build(120.0, ui));
    frame(&mut h, |ui| build(120.0, ui));
    assert!(
        h.engines.damage.counters.dirty().is_empty(),
        "steady frame must produce no diff"
    );

    // Frame 3 nudges shape 2's y: only its prev (y=120) and curr (y=140) rects enter damage.
    let prev_snap = h.engines.damage.prev[&WidgetId::from_hash("canvas")];
    let prev_arena_len = h.engines.damage.paints.slots.len();
    let prev_shape2_rect = h
        .engines
        .damage
        .prev_paint_rows(WidgetId::from_hash("canvas"))[1 + 2]
        .screen;
    frame(&mut h, |ui| build(140.0, ui));

    let canvas_snap = h.engines.damage.prev[&WidgetId::from_hash("canvas")];
    let curr_shape2_rect = h
        .engines
        .damage
        .prev_paint_rows(WidgetId::from_hash("canvas"))[1 + 2]
        .screen;
    assert_eq!(
        canvas_snap.paint_span, prev_snap.paint_span,
        "same-count paint changes must refresh the existing arena span",
    );
    assert_eq!(
        h.engines.damage.paints.slots.len(),
        prev_arena_len,
        "an in-place refresh must not touch the allocator at all",
    );

    let region = h.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));
    assert!(
        intersects(prev_shape2_rect),
        "old position of moved shape must be in damage region; \
         prev_rect = {prev_shape2_rect:?}, region = {region:?}",
    );
    assert!(
        intersects(curr_shape2_rect),
        "new position of moved shape must be in damage region; \
         curr_rect = {curr_shape2_rect:?}, region = {region:?}",
    );

    // Sentinel on the chrome's top edge (y < 120): the whole 180x180 paint_rect union would hit it.
    let stale_chrome_band = Rect::new(40.0, 40.0, 20.0, 20.0); // inside chrome, away from moved shape
    assert!(
        !intersects(stale_chrome_band),
        "unchanged chrome interior must not enter damage; \
         stale_band = {stale_chrome_band:?}, region = {region:?}",
    );
}

/// A chrome authoring change (same rect) must push the chrome rect: it is row 0 and carries its own hash via `Paint.hash`.
#[test]
fn chrome_authoring_change_pushes_chrome_paint_row() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |fill: RgbaF32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("c"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background::fill(fill))
            .show(ui, |_| {});
    };
    frame(&mut h, |ui| build(BLUE, ui));
    frame(&mut h, |ui| build(BLUE, ui)); // settle
    let snap_rect = h.engines.damage.prev_paint_rows(WidgetId::from_hash("c"))[0].screen;

    frame(&mut h, |ui| build(RED, ui));
    let region = h.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    assert!(
        rects.iter().any(|r| r.intersects(snap_rect)),
        "chrome authoring change must push chrome paint row even when \
         rect geometry is unchanged; region = {rects:?}",
    );
}

/// The focus ring arriving damages that node only: it rides the chrome row, whose hash carries it.
#[test]
fn a_focus_ring_damages_only_its_node() {
    use crate::input::keyboard::key::Key;
    use crate::widgets::block::Block;

    let [a, b] = ["a", "b"].map(WidgetId::from_hash);
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().gap(20.0).show(ui, |ui| {
            for id in [a, b] {
                Block::new()
                    .id(id)
                    .size((Sizing::fixed(40.0), Sizing::fixed(30.0)))
                    .focusable(true)
                    .show(ui);
            }
        });
    };
    frame(&mut h, build);
    frame(&mut h, build); // settle
    h.key(Key::Tab);
    frame(&mut h, build);
    assert_eq!(h.ui.focus(), Some(a));
    let a_rect = h.engines.damage.prev_paint_rows(a)[0].screen;
    let b_rect = h.ui.response_for(b).rect.expect("b arranged");
    let region = h.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    assert!(!rects.is_empty(), "the ring damages something");
    assert!(
        rects.iter().all(|r| a_rect.contains_rect(*r)),
        "every damage rect lies in the ringed node {a_rect:?}: {rects:?}",
    );
    assert!(
        rects.iter().all(|r| !r.intersects(b_rect)),
        "the neighbour {b_rect:?} is not damaged: {rects:?}",
    );
}

/// Every `DamageEngine.prev` entry covers at least one Paint row (chrome is row 0), else `DamageEngine::compute`'s removal tail leaves pixels unrepainted.
#[test]
fn chrome_only_owner_has_nonzero_paint_span() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("chrome_only"))
            .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
            .background(Background::fill(BLUE))
            .show(ui, |_| {});
    };
    frame(&mut h, build);
    frame(&mut h, build); // settle prev

    let wid = WidgetId::from_hash("chrome_only");
    let snap = h.engines.damage.prev[&wid];
    assert_eq!(
        snap.paint_span.len, 1,
        "chrome-only owner must contribute exactly one Paint row (chrome)",
    );

    for (k, s) in &h.engines.damage.prev {
        assert!(
            s.paint_span.len > 0,
            "prev entry {k:?} has zero-len paint_span, violating painting-only invariant",
        );
    }
}

/// Changing a `Shape::Text` with `local_origin: Some(_)` damages the shaped-text bbox, not just the origin point: prev and curr extents come from `LayerLayout::text_shapes`.
#[test]
fn text_content_change_damages_shaped_extent_not_just_origin() {
    use crate::shape::Shape;
    use crate::text::font_family::FontFamily;
    use crate::text::font_weight::FontWeight;
    use crate::text::wrap::TextWrap;
    use crate::widget_core::widget::Widget;

    // Mono geometry: glyph width = font_size * 0.5, line height = font_size; at 14, "abc" is 21x14 and "abcdef" 42x14.
    const FONT: f32 = 14.0;
    const ORIGIN: Vec2 = Vec2::new(10.0, 10.0);

    let mut h = UiHarness::new(DISPLAY.physical);
    let leaf_id = WidgetId::from_hash("text-host");
    let build = |text: &'static str, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(100.0), Sizing::fixed(50.0)))
            .show(ui, |ui| {
                let widget = Widget::leaf().id(leaf_id);
                widget.record(ui, None, |ui| {
                    let text = ui.intern(text);
                    ui.add_shape(
                        Shape::text(
                            text,
                            GlyphFont {
                                line_height: FONT,
                                ..GlyphFont::new(FONT)
                            },
                        )
                        .at_origin(ORIGIN)
                        .color(RgbaF32::WHITE)
                        .wrap(TextWrap::Truncate)
                        .family(FontFamily::SANS)
                        .weight(FontWeight::REGULAR),
                    );
                });
            });
    };

    frame(&mut h, |ui| build("abc", ui));
    frame(&mut h, |ui| build("abc", ui));
    assert!(
        h.engines.damage.counters.dirty().is_empty(),
        "steady frame must produce no diff"
    );

    // Damage rects inflate by `TEXT_SCALE_STEP * measured` per axis (`STEP/2` per side; see `text_paint_bbox_local`).
    let inflate = 1.0 + TEXT_SCALE_STEP;
    let prev_text_rect = h.engines.damage.prev_paint_rows(leaf_id)[0].screen;
    let prev_size_short: Size = Size::new(FONT * 0.5 * 3.0 * inflate, FONT * inflate);
    assert_eq!(
        prev_text_rect.size, prev_size_short,
        "the short text's damage size"
    );

    frame(&mut h, |ui| build("abcdef", ui));
    let curr_text_rect = h.engines.damage.prev_paint_rows(leaf_id)[0].screen;
    let curr_size_long: Size = Size::new(FONT * 0.5 * 6.0 * inflate, FONT * inflate);
    assert_eq!(
        curr_text_rect.size, curr_size_long,
        "the long text's damage size"
    );

    let region = h.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));

    // Probe at origin.x + 30: inside the new "abcdef" rect (42px) but past the old "abc" (21px).
    let inside_new_only = Rect::new(ORIGIN.x + 30.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_new_only),
        "probe inside new text but past old text must be in damage; \
         probe = {inside_new_only:?}, region = {region:?}",
    );

    let inside_old = Rect::new(ORIGIN.x + 10.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_old),
        "probe inside old text must be in damage; \
         probe = {inside_old:?}, region = {region:?}",
    );
}

/// A run's damage covers its glyphs' ink (an italic `f` reaches past its advance): the block grown by the measured ink, then padded by the scale-step fraction as `inflate_text_damage` does.
#[test]
fn a_text_run_damages_its_ink_past_the_block() {
    const ORIGIN: Vec2 = Vec2::new(20.0, 10.0);
    const FONT: f32 = 64.0;

    use crate::text::font_family::FontFamily;
    use crate::text::font_slant::FontSlant;
    use crate::text::wrap::TextWrap;
    use crate::widget_core::widget::Widget;
    let mut h = UiHarness::with_text(DISPLAY.physical);
    let leaf_id = WidgetId::from_hash("text-host");
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::fixed(200.0), Sizing::fixed(100.0)))
            .show(ui, |ui| {
                Widget::leaf().id(leaf_id).record(ui, None, |ui| {
                    let text = ui.intern("f");
                    ui.add_shape(
                        Shape::text(
                            text,
                            GlyphFont {
                                line_height: FONT,
                                slant: FontSlant::Italic,
                                ..GlyphFont::new(FONT)
                            },
                        )
                        .at_origin(ORIGIN)
                        .color(RgbaF32::WHITE)
                        .wrap(TextWrap::SingleLine)
                        .family(FontFamily::SANS),
                    );
                });
            });
    });

    let shaped = h.ui.layout(Layer::Main).text_shapes[0];
    let [left, top, right, bottom] = shaped.extent.ink.as_array();
    assert!(
        left > 0.0 && right > 0.0,
        "the italic f reaches past both sides: {shaped:?}"
    );
    let inked = Size::new(
        shaped.extent.size.w + left + right,
        shaped.extent.size.h + top + bottom,
    );
    let pad = Vec2::new(inked.w, inked.h) * (TEXT_SCALE_STEP * 0.5);
    assert_eq!(
        h.engines.damage.prev_paint_rows(leaf_id)[0].screen,
        Rect {
            min: ORIGIN - Vec2::new(left, top) - pad,
            size: Size::new(inked.w + 2.0 * pad.x, inked.h + 2.0 * pad.y),
        },
    );
    let paint = h.encode_paint();
    assert_eq!(paint.calls[0].as_text().unwrap().ink, shaped.extent.ink);
}

/// A visibility flip on the same frame as a paint-row change must still damage the exact-matched rows, not only the changed shape.
#[test]
fn visibility_flip_with_coincident_shape_change_damages_whole_node() {
    const CHROME_PROBE: Rect = Rect::new(44.0, 44.0, 2.0, 2.0);
    const LINE_PROBE: Rect = Rect::new(10.0, 9.0, 2.0, 2.0);
    let node = |ui: &mut Ui, hidden: bool, color: RgbaF32| {
        let mut p = Panel::zstack()
            .id(WidgetId::from_hash("a"))
            .size(50.0)
            .background(Background::fill(BLUE));
        if hidden {
            p = p.hidden();
        }
        p.show(ui, |ui| {
            ui.add_shape(
                Shape::line(
                    Vec2::new(5.0, 10.0),
                    Vec2::new(20.0, 10.0),
                    Stroke::new(color, 2.0),
                )
                .cap(LineCap::Round),
            );
        });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| node(ui, false, BLUE));
    let damage = frame(&mut h, |ui| node(ui, true, RED));
    let region = Damage::expect_partial(damage);
    assert!(
        region.any_intersects(LINE_PROBE),
        "changed shape's own rect must be damaged",
    );
    assert!(
        region.any_intersects(CHROME_PROBE),
        "exact-matched chrome must also clear when the node hides; region = {region:?}",
    );
}
