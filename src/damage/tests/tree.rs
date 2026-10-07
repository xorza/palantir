//! What moving, adding and removing nodes damages.

use crate::Ui;
use crate::damage::Damage;
use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::visibility::Visibility;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, button::Button, panel::Panel};
use glam::Vec2;

/// Removing a child of a fixed-size canvas must not re-damage the canvas's own direct shapes (child markers flip `node_hash`, but `cascade_input` and every own `Paint` are unchanged); only the vacated child's footprint is damage.
#[test]
fn removing_canvas_child_does_not_redamage_sibling_shapes() {
    const LINE_PROBE: Rect = Rect::new(140.0, 140.0, 20.0, 20.0);
    const REMOVED_CHILD: Rect = Rect::new(60.0, 10.0, 20.0, 20.0);

    let canvas = |ui: &mut Ui, n_children: usize| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::line(
                        Vec2::new(120.0, 120.0),
                        Vec2::new(180.0, 180.0),
                        Stroke::new(BLUE, 2.0),
                    )
                    .cap(LineCap::Round),
                );
                for i in 0..n_children {
                    Block::new()
                        .id(WidgetId::from_hash(("child", i)))
                        .position((10.0 + i as f32 * 50.0, 10.0))
                        .size(20.0)
                        .background(Background::fill(RED))
                        .show(ui);
                }
            });
    };

    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, 2));
    frame(&mut h, |ui| canvas(ui, 1));

    let region = h.damage_region();
    assert!(
        region.any_intersects(REMOVED_CHILD),
        "the vacated child's footprint must be damaged",
    );
    assert!(
        !region.any_intersects(LINE_PROBE),
        "the canvas's own line shape must not be re-damaged by a sibling \
         removal; region = {region:?}",
    );
}

/// Two panels each with an auto-id leaf from one call site; only draw order flips, so damage is empty. Auto ids are parent-scoped, so reordering can't churn identity.
#[test]
fn reordering_nodes_does_not_damage_unchanged_leaves() {
    fn node(ui: &mut Ui, key: &str, pos: (f32, f32)) {
        Panel::vstack()
            .id(WidgetId::from_hash(key))
            .position(pos)
            .size((Sizing::fixed(30.0), Sizing::fixed(30.0)))
            .show(ui, |ui| {
                Block::new()
                    .size(10.0)
                    .background(Background::fill(RED))
                    .show(ui);
            });
    }
    let canvas = |ui: &mut Ui, order: [(&str, (f32, f32)); 2]| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (key, pos) in order {
                    node(ui, key, pos);
                }
            });
    };

    let a = ("a", (10.0, 10.0));
    let b = ("b", (120.0, 120.0));
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, [a, b]));
    frame(&mut h, |ui| canvas(ui, [b, a]));

    assert!(
        h.damage_region().is_empty(),
        "reordering nodes must not damage unchanged leaves; region = {:?}",
        h.damage_region(),
    );
}

/// Raising an overlapping painting node damages only the overlap; `c` and the rest of `a` stay clean. As canvas children, the reordered markers flip `node_hash` and the row matcher damages each inverted pair's overlap; as roots of one layer, `RootOrder` does the same.
#[test]
fn raising_an_overlapping_node_redamages_only_the_overlap() {
    type Record<'a> = &'a dyn Fn(&mut Ui, Order<'_>);

    const A: Rect = Rect::new(10.0, 10.0, 40.0, 40.0);
    const B: Rect = Rect::new(30.0, 30.0, 40.0, 40.0);
    const OVERLAP: Rect = Rect::new(32.0, 32.0, 4.0, 4.0);
    const A_ONLY: Rect = Rect::new(12.0, 12.0, 4.0, 4.0);
    const C: Rect = Rect::new(150.0, 150.0, 20.0, 20.0);

    fn block(key: &str, size: f32) -> Block {
        Block::new()
            .id(WidgetId::from_hash(key))
            .size(size)
            .background(Background::fill(BLUE))
    }
    type Order<'a> = [(&'a str, Rect); 3];
    let canvas = |ui: &mut Ui, order: Order<'_>| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (key, r) in order {
                    block(key, r.size.w).position((r.min.x, r.min.y)).show(ui);
                }
            });
    };
    let roots = |ui: &mut Ui, order: Order<'_>| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |_| {});
        for (key, r) in order {
            ui.layer(Layer::Popup).fixed_at(r.min).show(|ui| {
                block(key, r.size.w).show(ui);
            });
        }
    };

    let a = ("a", A);
    let b = ("b", B);
    let c = ("c", C);
    let arrangements: [(&str, Record<'_>); 2] =
        [("canvas children", &canvas), ("layer roots", &roots)];
    for (label, record) in arrangements {
        let mut h = UiHarness::new(DISPLAY.physical);
        frame(&mut h, |ui| record(ui, [a, b, c]));
        frame(&mut h, |ui| record(ui, [b, c, a]));

        let region = h.damage_region();
        let rects = region.iter_rects().collect::<Vec<_>>();
        assert!(
            region.any_intersects(OVERLAP),
            "{label}: raising `a` over `b` must repaint their overlap; region = {rects:?}",
        );
        assert!(
            !region.any_intersects(A_ONLY),
            "{label}: `a` outside the overlap must stay clean; region = {rects:?}",
        );
        assert!(
            !region.any_intersects(C),
            "{label}: the non-overlapping node `c` must stay clean; region = {rects:?}",
        );

        frame(&mut h, |ui| record(ui, [b, c, a]));
        assert!(
            h.damage_region().is_empty(),
            "{label}: a settled reorder must re-damage nothing; region = {:?}",
            h.damage_region(),
        );
    }
}

/// Two text nodes scrolled fully off a clipped canvas, with only draw order flipping, cause zero damage: their clipped runs are empty, so inflation must not re-grow them into edge slivers.
#[test]
fn offscreen_text_nodes_reorder_cast_no_edge_shadow() {
    fn node(ui: &mut Ui, key: &str, y: f32) {
        Button::new()
            .id(WidgetId::from_hash(key))
            .label("Node label")
            .position((-300.0, y))
            .show(ui);
    }
    let canvas = |ui: &mut Ui, order: [(&str, f32); 2]| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .clip_rect()
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (key, y) in order {
                    node(ui, key, y);
                }
            });
    };

    let a = ("a", 40.0);
    let b = ("b", 44.0);
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, [a, b]));
    frame(&mut h, |ui| canvas(ui, [b, a]));

    assert!(
        h.damage_region().is_empty(),
        "off-screen text must not fabricate edge-of-window damage on \
         reorder; region = {:?}",
        h.damage_region(),
    );
}

/// A sequential stack re-lays children by record order, so swapping two moves both; the position diff carries the damage.
#[test]
fn reordering_a_stack_is_damaged_by_the_position_diff() {
    fn child(ui: &mut Ui, key: &str, fill: RgbaF32) {
        Block::new()
            .id(WidgetId::from_hash(key))
            .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
            .background(Background::fill(fill))
            .show(ui);
    }
    let stack = |ui: &mut Ui, order: [(&str, RgbaF32); 2]| {
        Panel::vstack()
            .id(WidgetId::from_hash("stack"))
            .show(ui, |ui| {
                for (key, fill) in order {
                    child(ui, key, fill);
                }
            });
    };

    let a = ("a", BLUE);
    let b = ("b", RED);
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| stack(ui, [a, b])); // a in the top slot, b below
    frame(&mut h, |ui| stack(ui, [b, a])); // swapped

    let region = h.damage_region();
    assert!(
        region.any_intersects(Rect::new(0.0, 5.0, 40.0, 5.0))
            && region.any_intersects(Rect::new(0.0, 25.0, 40.0, 5.0)),
        "swapping stack children must damage both slots; region = {region:?}",
    );
}

/// A direct shape moving from above a child subtree to below it changes composited pixels: the row matcher sees the shape/child-marker inversion and damages only their overlap, not the far end of the line.
#[test]
fn shape_crossing_child_boundary_is_redamaged() {
    const CHILD: Rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    const PROBE: Rect = Rect::new(30.0, 39.0, 2.0, 2.0);
    const FAR_PROBE: Rect = Rect::new(64.0, 39.0, 2.0, 2.0);

    let line = |ui: &mut Ui| {
        ui.add_shape(
            Shape::line(
                Vec2::new(10.0, 40.0),
                Vec2::new(70.0, 40.0),
                Stroke::new(BLUE, 4.0),
            )
            .cap(LineCap::Round),
        );
    };
    let child = |ui: &mut Ui| {
        Block::new()
            .id(WidgetId::from_hash("child"))
            .position((CHILD.min.x, CHILD.min.y))
            .size(CHILD.size.w)
            .background(Background::fill(RED))
            .show(ui);
    };
    let over = |ui: &mut Ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                child(ui);
                line(ui);
            });
    };
    let under = |ui: &mut Ui| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                line(ui);
                child(ui);
            });
    };

    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, over);
    frame(&mut h, under);

    let region = h.damage_region();
    assert!(
        region.any_intersects(PROBE),
        "the shape's overlap with the child must be re-damaged when the \
         shape crosses the child z-boundary; region = {region:?}",
    );
    assert!(
        !region.any_intersects(FAR_PROBE),
        "the stretch of the line outside the child paints identically in \
         either order and must stay clean; region = {region:?}",
    );
}

/// Two overlapping direct shapes of one node swap record order: every content key survives, so only the span-local inversion check sees it.
#[test]
fn overlapping_direct_shape_swap_is_redamaged() {
    const PROBE: Rect = Rect::new(38.0, 29.0, 2.0, 2.0);
    let line = |ui: &mut Ui, color: RgbaF32| {
        ui.add_shape(
            Shape::line(
                Vec2::new(10.0, 30.0),
                Vec2::new(70.0, 30.0),
                Stroke::new(color, 8.0),
            )
            .cap(LineCap::Round),
        );
    };
    let canvas = |ui: &mut Ui, first: RgbaF32, second: RgbaF32| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                line(ui, first);
                line(ui, second);
            });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, BLUE, RED));
    frame(&mut h, |ui| canvas(ui, RED, BLUE));

    let region = h.damage_region();
    assert!(
        region.any_intersects(PROBE),
        "swapping two overlapping direct shapes must damage their \
         overlap; region = {region:?}",
    );
}

/// Inserting a child shifts later rows' positions but survivors keep their relative order, so an unchanged shape after the children contributes no damage.
#[test]
fn inserting_a_child_does_not_redamage_unmoved_later_shapes() {
    const CHILD_A: Rect = Rect::new(10.0, 10.0, 30.0, 30.0);
    const CHILD_B: Rect = Rect::new(120.0, 10.0, 30.0, 30.0);
    const LINE_PROBE: Rect = Rect::new(30.0, 99.0, 2.0, 2.0);

    fn node(ui: &mut Ui, key: &str, r: Rect) {
        Block::new()
            .id(WidgetId::from_hash(key))
            .position((r.min.x, r.min.y))
            .size(r.size.w)
            .background(Background::fill(BLUE))
            .show(ui);
    }
    let canvas = |ui: &mut Ui, with_b: bool| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                node(ui, "a", CHILD_A);
                if with_b {
                    node(ui, "b", CHILD_B);
                }
                ui.add_shape(
                    Shape::line(
                        Vec2::new(10.0, 100.0),
                        Vec2::new(70.0, 100.0),
                        Stroke::new(RED, 4.0),
                    )
                    .cap(LineCap::Round),
                );
            });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, false));
    frame(&mut h, |ui| canvas(ui, true));

    let region = h.damage_region();
    assert!(
        region.any_intersects(CHILD_B),
        "the inserted child must be damaged; region = {region:?}",
    );
    assert!(
        !region.any_intersects(LINE_PROBE),
        "an unchanged shape whose relative order is preserved must not \
         be re-damaged by a child insert; region = {region:?}",
    );
}

/// Re-keying a child (same content, new `WidgetId`) flips its parent's hash into the changed-paints arm, which must emit nothing for the parent; the child's pixels are damaged by its old id's eviction plus its new id's insert.
#[test]
fn rekeying_a_child_damages_only_the_child() {
    const CHILD: Rect = Rect::new(10.0, 10.0, 30.0, 30.0);
    const LINE_PROBE: Rect = Rect::new(30.0, 99.0, 2.0, 2.0);

    let canvas = |ui: &mut Ui, key: &str| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash(key))
                    .position((CHILD.min.x, CHILD.min.y))
                    .size(CHILD.size.w)
                    .background(Background::fill(BLUE))
                    .show(ui);
                ui.add_shape(
                    Shape::line(
                        Vec2::new(10.0, 100.0),
                        Vec2::new(70.0, 100.0),
                        Stroke::new(RED, 4.0),
                    )
                    .cap(LineCap::Round),
                );
            });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, "k1"));
    frame(&mut h, |ui| canvas(ui, "k2"));

    let region = h.damage_region();
    assert!(
        region.any_intersects(CHILD),
        "a re-keyed child must be damaged (evict + re-add); region = {region:?}",
    );
    assert!(
        !region.any_intersects(LINE_PROBE),
        "the parent's unchanged sibling shape must not be re-damaged by \
         a child re-key; region = {region:?}",
    );
}

/// Removing a middle shape shifts trailing ordinals; damage covers the removed pixels plus shifted shapes' old and new positions, and the snapshot tail is trimmed via `drain(ord..)`.
#[test]
fn shape_removed_from_middle_evicts_trailing_ordinals() {
    use crate::widget::Shape;

    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |include_middle: bool, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::fixed(180.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::rect(Rect::new(0.0, 0.0, 20.0, 20.0)).fill(RgbaF32::srgb(1.0, 0.0, 0.0)),
                );
                if include_middle {
                    ui.add_shape(
                        Shape::rect(Rect::new(60.0, 0.0, 20.0, 20.0))
                            .fill(RgbaF32::srgb(0.0, 1.0, 0.0)),
                    );
                }
                ui.add_shape(
                    Shape::rect(Rect::new(120.0, 0.0, 20.0, 20.0))
                        .fill(RgbaF32::srgb(0.0, 0.0, 1.0)),
                );
            });
    };

    frame(&mut h, |ui| build(true, ui));
    frame(&mut h, |ui| build(true, ui)); // settle

    let prev_shapes = h
        .engines
        .damage
        .prev_paint_rows(WidgetId::from_hash("canvas"));
    assert_eq!(prev_shapes.len(), 3);
    let prev_middle_rect = prev_shapes[1].screen;
    let prev_blue_rect = prev_shapes[2].screen;

    frame(&mut h, |ui| build(false, ui));

    let post = h.engines.damage.prev[&WidgetId::from_hash("canvas")];
    assert_eq!(
        post.paint_span.len, 2,
        "snapshot tail must be trimmed to the new paint count",
    );

    let region = h.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    let intersects = |r: Rect| rects.iter().any(|d| d.intersects(r));

    assert!(
        intersects(prev_middle_rect),
        "deleted shape's prev rect must enter damage; \
         prev_middle = {prev_middle_rect:?}, region = {rects:?}",
    );
    assert!(
        !intersects(prev_blue_rect),
        "unmoved blue shape must not enter damage; \
         prev_blue = {prev_blue_rect:?}, region = {rects:?}",
    );
}

/// Inserting a shape between two shifts trailing ordinals, but content-keyed matching pairs the existing shapes, so only the new one damages.
#[test]
fn shape_added_in_middle_damages_only_new() {
    use crate::widget::Shape;

    let mut h = UiHarness::new(DISPLAY.physical);
    let red_rect = Rect::new(0.0, 0.0, 20.0, 20.0);
    let green_rect = Rect::new(60.0, 0.0, 20.0, 20.0);
    let blue_rect = Rect::new(120.0, 0.0, 20.0, 20.0);
    let build = |include_middle: bool, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::fixed(180.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(red_rect).fill(RgbaF32::srgb(1.0, 0.0, 0.0)));
                if include_middle {
                    ui.add_shape(Shape::rect(green_rect).fill(RgbaF32::srgb(0.0, 1.0, 0.0)));
                }
                ui.add_shape(Shape::rect(blue_rect).fill(RgbaF32::srgb(0.0, 0.0, 1.0)));
            });
    };

    frame(&mut h, |ui| build(false, ui)); // red + blue
    frame(&mut h, |ui| build(false, ui)); // settle

    let prev_shapes: Vec<_> = h
        .engines
        .damage
        .prev_paint_rows(WidgetId::from_hash("canvas"))
        .to_vec();
    assert_eq!(prev_shapes.len(), 2);
    let prev_red_screen = prev_shapes[0].screen;
    let prev_blue_screen = prev_shapes[1].screen;

    frame(&mut h, |ui| build(true, ui)); // insert green between

    let post = h.engines.damage.prev[&WidgetId::from_hash("canvas")];
    assert_eq!(post.paint_span.len, 3);

    let curr_shapes: Vec<_> = h
        .engines
        .damage
        .prev_paint_rows(WidgetId::from_hash("canvas"))
        .to_vec();
    let region = h.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    let intersects = |r: Rect| rects.iter().any(|d| d.intersects(r));

    let green_screen = curr_shapes
        .iter()
        .find(|p| !prev_shapes.iter().any(|pp| pp == *p))
        .expect("inserted paint must appear in current span")
        .screen;
    assert!(
        intersects(green_screen),
        "newly inserted shape must enter damage; \
         green = {green_screen:?}, region = {rects:?}",
    );
    assert!(
        !intersects(prev_red_screen),
        "unmoved red shape must not enter damage; region = {rects:?}",
    );
    assert!(
        !intersects(prev_blue_screen),
        "ordinal-shifted-but-unchanged blue shape must not enter damage; \
         region = {rects:?}",
    );
}

/// One leaf of identical content and rect under `A` or `B` (chromeless full-surface ZStacks); only its compositing position changes (`NodeSnapshot::parent_key`). `hidden` hides both parents.
fn reparent_fixture(ui: &mut Ui, under_b: bool, hidden: bool) {
    let leaf = |ui: &mut Ui| {
        Block::new()
            .id(WidgetId::from_hash("L"))
            .size(30.0)
            .background(Background::fill(BLUE))
            .show(ui);
    };
    let parent = |ui: &mut Ui, id: &'static str, holds_leaf: bool| {
        let mut panel = Panel::zstack()
            .id(WidgetId::from_hash(id))
            .size((Sizing::FILL, Sizing::FILL));
        if hidden {
            panel = panel.hidden();
        }
        panel.show(ui, |ui| {
            if holds_leaf {
                leaf(ui);
            }
        });
    };
    Panel::zstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |ui| {
            parent(ui, "A", !under_b);
            parent(ui, "B", under_b);
        });
}

/// Reparenting a widget at an identical rect and content must damage its painted extent, since its z-order against outside content flips.
#[test]
fn reparent_at_same_rect_damages_moved_subtree() {
    const LEAF_PROBE: Rect = Rect::new(10.0, 10.0, 2.0, 2.0);
    let build = |ui: &mut Ui, under_b: bool| reparent_fixture(ui, under_b, false);
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| build(ui, false));
    let damage = frame(&mut h, |ui| build(ui, true));
    let region = Damage::expect_partial(damage);
    assert!(
        region.any_intersects(LEAF_PROBE),
        "moved leaf's extent must be damaged; region = {region:?}",
    );
    let settled = frame(&mut h, |ui| build(ui, true));
    assert_eq!(settled, None, "reparent damage must not repeat");
}

/// The same move under a hidden ancestor damages nothing: the cascade reads inherited visibility, so the subtree owns no paint rows.
#[test]
fn reparenting_a_hidden_subtree_damages_nothing() {
    let build = |ui: &mut Ui, under_b: bool| reparent_fixture(ui, under_b, true);
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| build(ui, false));
    assert_eq!(
        frame(&mut h, |ui| build(ui, true)),
        None,
        "a subtree that paints nothing costs nothing to move",
    );
}

/// Inserting a shape at the front of a node's record stream damages only the new shape; shifted rows match by content and keep their relative order.
#[test]
fn front_insert_damages_only_the_new_shape() {
    const NEW_PROBE: Rect = Rect::new(150.0, 149.0, 2.0, 2.0);
    const OLD_PROBE: Rect = Rect::new(30.0, 19.0, 2.0, 2.0);
    let line = |ui: &mut Ui, y: f32| {
        ui.add_shape(
            Shape::line(
                Vec2::new(10.0, y),
                Vec2::new(70.0, y),
                Stroke::new(BLUE, 2.0),
            )
            .cap(LineCap::Round),
        );
    };
    let build = |ui: &mut Ui, with_front: bool| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                if with_front {
                    ui.add_shape(
                        Shape::line(
                            Vec2::new(140.0, 150.0),
                            Vec2::new(170.0, 150.0),
                            Stroke::new(RED, 2.0),
                        )
                        .cap(LineCap::Round),
                    );
                }
                line(ui, 20.0);
                line(ui, 30.0);
                line(ui, 40.0);
            });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| build(ui, false));
    let damage = frame(&mut h, |ui| build(ui, true));
    let region = Damage::expect_partial(damage);
    assert!(
        region.any_intersects(NEW_PROBE),
        "inserted shape must be damaged; region = {region:?}",
    );
    assert!(
        !region.any_intersects(OLD_PROBE),
        "shifted-but-identical rows must not re-damage; region = {region:?}",
    );
}

/// A hidden container and everything under it have empty paint spans, so none may enter the snapshot map. The child has its own chrome and `Visible`, separating the cascaded reading from its own; the visible sibling is the control.
#[test]
fn a_hidden_container_takes_no_snapshot() {
    let hidden = WidgetId::from_hash("hidden-box");
    let hidden_child = WidgetId::from_hash("hidden-child");
    let shown = WidgetId::from_hash("shown-box");
    let mut h = UiHarness::cold(DISPLAY.physical);
    frame(&mut h, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::hstack().id(hidden).hidden().show(ui, |ui| {
                    Block::new()
                        .id(hidden_child)
                        .size(20.0)
                        .background(Background::fill(BLUE))
                        .show(ui);
                });
                Block::new()
                    .id(shown)
                    .size(20.0)
                    .background(Background::fill(RED))
                    .show(ui);
            });
    });
    assert!(
        h.engines.damage.prev.contains_key(&shown),
        "a painting sibling is in the map, so the frame filled it",
    );
    assert!(
        !h.engines.damage.prev.contains_key(&hidden),
        "a rowless container must not take a snapshot",
    );
    assert!(
        !h.engines.damage.prev.contains_key(&hidden_child),
        "a `Visible` child under it is rowless for the same reason",
    );
}

/// A chromeless container that stops painting takes its painted descendants' pixels with it (the moved-subtree leg must evict them); showing it again repaints both.
#[test]
fn hiding_a_chromeless_container_evicts_its_painted_descendants() {
    const CHILD_PROBE: Rect = Rect::new(45.0, 45.0, 2.0, 2.0);
    const GRANDCHILD_PROBE: Rect = Rect::new(12.0, 12.0, 2.0, 2.0);
    let child = WidgetId::from_hash("child");
    let grandchild = WidgetId::from_hash("grandchild");
    let build = |ui: &mut Ui, vis: Visibility| {
        Panel::zstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("container"))
                    .visibility(vis)
                    .show(ui, |ui| {
                        Panel::zstack()
                            .id(child)
                            .size(50.0)
                            .background(Background::fill(BLUE))
                            .show(ui, |ui| {
                                Block::new()
                                    .id(grandchild)
                                    .size(20.0)
                                    .background(Background::fill(RED))
                                    .show(ui);
                            });
                    });
            });
    };
    for vis in [Visibility::Hidden, Visibility::Collapsed] {
        let mut h = UiHarness::new(DISPLAY.physical);
        frame(&mut h, |ui| build(ui, Visibility::Visible));
        let hide = Damage::expect_partial(frame(&mut h, |ui| build(ui, vis)));
        for (probe, what) in [(CHILD_PROBE, "child"), (GRANDCHILD_PROBE, "grandchild")] {
            assert!(
                hide.any_intersects(probe),
                "{vis:?}: the {what}'s old pixels must be damaged; region = {hide:?}",
            );
        }
        for (wid, what) in [(child, "child"), (grandchild, "grandchild")] {
            assert!(
                !h.engines.damage.prev.contains_key(&wid),
                "{vis:?}: the {what} paints nothing now, so its snapshot must go",
            );
        }
        let show = Damage::expect_partial(frame(&mut h, |ui| build(ui, Visibility::Visible)));
        for (probe, what) in [(CHILD_PROBE, "child"), (GRANDCHILD_PROBE, "grandchild")] {
            assert!(
                show.any_intersects(probe),
                "{vis:?}: the {what}'s new pixels must be damaged; region = {show:?}",
            );
        }
    }
}

/// Reversing a deck of 200 equal cards at one rect costs one damage rect per card, not one per inverted pair (19 900).
#[test]
fn reversing_a_deck_pushes_one_rect_per_card() {
    const CARD: Rect = Rect::new(10.0, 10.0, 40.0, 40.0);
    let deck = |ui: &mut Ui, reversed: bool| {
        Panel::canvas()
            .id(WidgetId::from_hash("deck"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for i in 0..200u32 {
                    let card = if reversed { 199 - i } else { i };
                    Block::new()
                        .id(WidgetId::from_hash(("card", card)))
                        .position((CARD.min.x, CARD.min.y))
                        .size(CARD.size.w)
                        .background(Background::fill(BLUE))
                        .show(ui);
                }
            });
    };
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| deck(ui, false));
    frame(&mut h, |ui| deck(ui, true));
    let raw = &h.engines.damage.raw_rects;
    assert_eq!(raw.len(), 199, "{raw:?}");
    assert!(raw.iter().all(|&r| r == CARD));
}
