use crate::Ui;
use crate::forest::Layer;
use crate::forest::element::Configure;
use crate::forest::rollups::CascadeInputHash;
use crate::forest::tree::NodeId;
use crate::input::InputEvent;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect, transform::TranslateScale};
use crate::shape::{LineCap, LineJoin, Shape};
use crate::text::TEXT_SCALE_STEP;
use crate::ui::damage::region::DamageRegion;
use crate::ui::damage::{Damage, DamageEngine};
use crate::ui::frame::FrameStamp;
use crate::ui::frame_report::{RenderKind, RenderPlan};
use crate::widgets::popup::Popup;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use crate::{display::Display, layout::types::sizing::Sizing};
use glam::{UVec2, Vec2};
use std::time::Duration;

#[allow(dead_code)]
const SURFACE: Rect = Rect::new(0.0, 0.0, 200.0, 200.0);
const DISPLAY: Display = Display {
    physical: UVec2::new(200, 200),
    scale_factor: 1.0,
    pixel_snap: true,
    refresh_millihertz: None,
};

/// Drive one frame through the real [`Ui::frame`] path, simulate a
/// successful `WgpuBackend::submit` so the next frame's auto-rewind
/// doesn't fire, and return the damage decision for the just-completed
/// frame. Test sites that care about the damage shape bind the return;
/// the rest ignore it.
fn frame(ui: &mut Ui, f: impl FnMut(&mut Ui)) -> Damage {
    let report = ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), f);
    ui.frame_state.mark_submitted();
    match report.plan {
        None => Damage::Skip,
        Some(RenderPlan {
            kind: RenderKind::Full,
            ..
        }) => Damage::Full,
        Some(RenderPlan {
            kind: RenderKind::Partial { region },
            ..
        }) => Damage::Partial(region),
    }
}

/// The standard "root with one 50×50 frame" tree used by most damage
/// tests. Color flips between frames to drive minimal authoring
/// changes.
const BLUE: Color = Color::rgb(0.2, 0.4, 0.8);
const RED: Color = Color::rgb(0.9, 0.4, 0.8);

fn one_frame(ui: &mut Ui, color: Color) {
    Panel::hstack()
        .id(WidgetId::from_hash("root"))
        .show(ui, |ui| {
            Frame::new()
                .id(WidgetId::from_hash("a"))
                .size(50.0)
                .background(Background {
                    fill: color.into(),
                    ..Default::default()
                })
                .show(ui);
        });
}

/// Pin: the very first frame has no `prev_frame` entries, so every
/// painting node is "added" → marked dirty and contributes its rect.
/// The root Panel records no chrome and no direct shapes, so it's
/// non-painting and stays out of `dirty`/`region`.
#[test]
fn first_frame_marks_every_painting_node_dirty() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    let painting = ui.cascades.layers[Layer::Main]
        .paint_arena
        .node_spans
        .iter()
        .filter(|s| s.len > 0)
        .count();
    assert_eq!(ui.damage_engine.dirty.len(), painting);
    // First frame is `force_full`, so `compute` short-circuits to
    // `Damage::Full` after the structural diff — and the Vacant arm
    // skips its raw-rect pushes (the region would be discarded), so
    // the buffer stays empty and its retained capacity never balloons
    // to whole-tree size on the first frame or a resize storm.
    assert!(ui.damage_engine.raw_rects.is_empty());
}

/// Pin: re-recording identical authoring → zero dirty nodes,
/// damage rect is `None`. The steady-state ideal: idle UI does
/// nothing.
#[test]
fn unchanged_authoring_produces_no_damage() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui| {
        one_frame(ui, BLUE);
    };
    frame(&mut ui, build);
    frame(&mut ui, build);

    assert!(ui.damage_engine.dirty.is_empty());
    assert!(ui.damage_region().rects.is_empty());
    assert_eq!(Damage::new(ui.damage_region()), Damage::Skip,);
}

/// Pin: removing a child of a fixed-size canvas that paints its own
/// direct shapes must **not** re-damage those shapes. A node's
/// `node_hash` folds in a per-immediate-child marker (`compute_hashes`),
/// so dropping a child flips the parent's `node_hash` and routes it to
/// the per-shape diff arm — but with `cascade_input` unchanged and every
/// own `Paint` bit-identical, the parent's pixels didn't move. Only the
/// vacated child's footprint is damage. Regression: darkroom deleting a
/// node redrew every canvas connection, because the `geometry_unchanged`
/// fallback repainted the union of all direct shapes on any `node_hash`
/// flip rather than only on a `cascade_input` change.
#[test]
fn removing_canvas_child_does_not_redamage_sibling_shapes() {
    // Direct shape lives far from both children so its potential
    // (buggy) re-damage is geometrically distinguishable from the
    // legitimate vacated-child damage.
    const LINE_PROBE: Rect = Rect::new(140.0, 140.0, 20.0, 20.0);
    const REMOVED_CHILD: Rect = Rect::new(60.0, 10.0, 20.0, 20.0);

    let canvas = |ui: &mut Ui, n_children: usize| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            // Fixed size (not the default hug) so dropping a child can't
            // change the canvas's own rect — isolating the `node_hash`
            // path from any `cascade_input` change.
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                ui.add_shape(Shape::Line {
                    a: Vec2::new(120.0, 120.0),
                    b: Vec2::new(180.0, 180.0),
                    width: 2.0,
                    brush: Brush::Solid(BLUE),
                    cap: LineCap::Round,
                });
                for i in 0..n_children {
                    Frame::new()
                        .id(WidgetId::from_hash(("child", i)))
                        .position((10.0 + i as f32 * 50.0, 10.0))
                        .size(20.0)
                        .background(Background {
                            fill: RED.into(),
                            ..Default::default()
                        })
                        .show(ui);
                }
            });
    };

    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| canvas(ui, 2));
    frame(&mut ui, |ui| canvas(ui, 1));

    let region = ui.damage_region();
    assert!(
        region.any_intersects(REMOVED_CHILD),
        "the vacated child's footprint must be damaged",
    );
    assert!(
        !region.any_intersects(LINE_PROBE),
        "the canvas's own line shape must not be re-damaged by a sibling \
         removal; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Regression: two panels at fixed canvas positions, each with an
/// auto-id painting leaf recorded from a shared helper (one call site →
/// one auto base id). Only the draw order flips between frames;
/// positions + content are identical, so nothing visible changes and
/// damage must be empty. Before auto ids were parent-scoped, the leaf's
/// id was disambiguated by *global* occurrence order, so reordering the
/// nodes shuffled which node each disambiguated id mapped to and
/// spuriously damaged both — darkroom's "selecting/raising a node
/// rerenders untouched nodes" bug. Parent-scoping ties each leaf to its
/// own stable-id node body, so a reorder can't churn its identity.
#[test]
fn reordering_nodes_does_not_damage_unchanged_leaves() {
    fn node(ui: &mut Ui, key: &str, pos: (f32, f32)) {
        Panel::vstack()
            .id(WidgetId::from_hash(key))
            .position(pos)
            .size((Sizing::Fixed(30.0), Sizing::Fixed(30.0)))
            .show(ui, |ui| {
                // Auto id — no `.id`/`.id_salt`; same call site for every
                // node, so it collides across nodes and is disambiguated.
                Frame::new()
                    .size(10.0)
                    .background(Background {
                        fill: RED.into(),
                        ..Default::default()
                    })
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
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| canvas(ui, [a, b]));
    // Same positions + content, only draw order flips.
    frame(&mut ui, |ui| canvas(ui, [b, a]));

    assert!(
        ui.damage_region().rects.is_empty(),
        "reordering nodes must not damage unchanged leaves; region = {:?}",
        ui.damage_region().iter_rects().collect::<Vec<_>>(),
    );
}

/// Regression: raising an **overlapping** painting node (moving it to
/// the front of the paint order) flips which node shows in the overlap
/// even though the raised node's own rect / content / ancestor state
/// are untouched. The reordered child markers flip the canvas's
/// `node_hash`, routing it to the changed-paints arm, whose row
/// matcher damages the overlap of each *inverted* pair's painted
/// extents. A node the raised one doesn't overlap (`c`) stays clean —
/// the reorder damages overlaps only, never untouched non-overlapping
/// nodes.
#[test]
fn raising_an_overlapping_node_redamages_only_the_overlap() {
    // `a` and `b` overlap; `c` sits far from both.
    const A: Rect = Rect::new(10.0, 10.0, 40.0, 40.0);
    const B: Rect = Rect::new(30.0, 30.0, 40.0, 40.0);
    const OVERLAP: Rect = Rect::new(32.0, 32.0, 4.0, 4.0);
    const C: Rect = Rect::new(150.0, 150.0, 20.0, 20.0);

    fn node(ui: &mut Ui, key: &str, r: Rect) {
        Frame::new()
            .id(WidgetId::from_hash(key))
            .position((r.min.x, r.min.y))
            .size(r.size.w)
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui);
    }
    let canvas = |ui: &mut Ui, order: [(&str, Rect); 3]| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for (key, r) in order {
                    node(ui, key, r);
                }
            });
    };

    let a = ("a", A);
    let b = ("b", B);
    let c = ("c", C);
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| canvas(ui, [a, b, c]));
    // Raise `a` to the front (drawn last) — same positions + content.
    frame(&mut ui, |ui| canvas(ui, [b, c, a]));

    let region = ui.damage_region();
    assert!(
        region.any_intersects(OVERLAP),
        "raising `a` over `b` must repaint their overlap; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
    assert!(
        !region.any_intersects(C),
        "the non-overlapping node `c` must stay clean; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Regression: two **text**-bearing nodes scrolled fully off the left
/// edge of a clipped canvas (bodies clip to zero width). Only their draw
/// order flips. Their labels are entirely off-screen, so they must
/// contribute nothing — but `inflate_text_damage` used to re-grow each
/// already-clipped (zero-width) run by its ladder-snap pad, pushing the
/// box back across the clip edge to `[0, pad_w]`. Those fabricated
/// sub-pixel slivers then intersected in the reorder scan into a thin,
/// tall "shadow" of damage pinned to the window edge — the real bug (a
/// `~0.28px` red strip at the canvas edge cast by nodes that are
/// completely off-screen). With the run left empty, each node's extent is
/// zero-width and can't overlap anything, so the reorder is zero damage.
#[test]
fn offscreen_text_nodes_reorder_cast_no_edge_shadow() {
    fn node(ui: &mut Ui, key: &str, y: f32) {
        // Fully off-screen (x = -300): the body and every glyph clip
        // entirely away; only text-damage inflation could fake a sliver.
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

    // Overlapping Y so their (formerly-inflated) label boxes would meet.
    let a = ("a", 40.0);
    let b = ("b", 44.0);
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| canvas(ui, [a, b]));
    frame(&mut ui, |ui| canvas(ui, [b, a]));

    assert!(
        ui.damage_region().rects.is_empty(),
        "off-screen text must not fabricate edge-of-window damage on \
         reorder; region = {:?}",
        ui.damage_region().iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin: a **sequential stack** re-lays its children by record order, so
/// swapping two moves both — the normal position-based per-node diff
/// damages their old+new footprints. The stack's row matcher also sees
/// the marker swap but the children land at disjoint extents, so the
/// order scan adds nothing; the position diff must carry the damage.
#[test]
fn reordering_a_stack_is_damaged_by_the_position_diff() {
    fn child(ui: &mut Ui, key: &str, fill: Color) {
        Frame::new()
            .id(WidgetId::from_hash(key))
            .size((Sizing::Fixed(40.0), Sizing::Fixed(20.0)))
            .background(Background {
                fill: fill.into(),
                ..Default::default()
            })
            .show(ui);
    }
    let stack = |ui: &mut Ui, order: [(&str, Color); 2]| {
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
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| stack(ui, [a, b])); // a in the top slot, b below
    frame(&mut ui, |ui| stack(ui, [b, a])); // swapped

    // Both slots changed content (colours swapped), so both must be
    // damaged — top slot y=[0,20], bottom y=[20,40].
    let region = ui.damage_region();
    assert!(
        region.any_intersects(Rect::new(0.0, 5.0, 40.0, 5.0))
            && region.any_intersects(Rect::new(0.0, 25.0, 40.0, 5.0)),
        "swapping stack children must damage both slots; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Regression: a direct shape whose content + screen rect are unchanged
/// but which moves from *above* a child subtree to *below* it (its
/// interleave position relative to the child flips) changes the
/// composited pixels — the shape now paints under the child instead of
/// over it. The content-keyed per-shape diff used to pair the two
/// byte-identical `Paint`s and emit nothing, so the old on-top pixels
/// stayed stranded over the child. Mirrors darkroom committing an
/// in-flight connection preview (drawn over the nodes) into a
/// byte-identical wire drawn under them: same curve, flipped z-order.
/// The row matcher sees the shape↔child-marker inversion and damages
/// their extent overlap — and *only* the overlap: the stretch of the
/// line outside the child paints the same pixels in either order, so
/// the far end must stay clean (an inversion is not a full-shape
/// repaint).
#[test]
fn shape_crossing_child_boundary_is_redamaged() {
    // The line overlaps the child, so a stale on-top draw would visibly
    // cover it. Probe a point inside both the line strip and the child;
    // FAR_PROBE sits on the line but outside the child.
    const CHILD: Rect = Rect::new(20.0, 20.0, 40.0, 40.0);
    const PROBE: Rect = Rect::new(30.0, 39.0, 2.0, 2.0);
    const FAR_PROBE: Rect = Rect::new(64.0, 39.0, 2.0, 2.0);

    let line = |ui: &mut Ui| {
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, 40.0),
            b: Vec2::new(70.0, 40.0),
            width: 4.0,
            brush: Brush::Solid(BLUE),
            cap: LineCap::Round,
        });
    };
    let child = |ui: &mut Ui| {
        Frame::new()
            .id(WidgetId::from_hash("child"))
            .position((CHILD.min.x, CHILD.min.y))
            .size(CHILD.size.w)
            .background(Background {
                fill: RED.into(),
                ..Default::default()
            })
            .show(ui);
    };
    // `over`: line recorded after the child → paints on top. `under`:
    // identical line recorded before the child → paints beneath.
    // Fixed-size canvas so its own rect (and thus `cascade_input`)
    // can't change between the two.
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

    let mut ui = Ui::for_test();
    frame(&mut ui, over);
    frame(&mut ui, under);

    let region = ui.damage_region();
    assert!(
        region.any_intersects(PROBE),
        "the shape's overlap with the child must be re-damaged when the \
         shape crosses the child z-boundary; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
    assert!(
        !region.any_intersects(FAR_PROBE),
        "the stretch of the line outside the child paints identically in \
         either order and must stay clean; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Regression: two overlapping direct shapes of the *same* node swap
/// record order — the visible top color flips, but every content key
/// stays put: both `(screen, hash)` pairs still exist (pass 1 of
/// `diff_changed_leg` pairs them exactly), no child is involved, and
/// the node's `cascade_input` is untouched. Only the leg's span-local
/// inversion check sees it. This was a silent stale-pixel hole before
/// the order check covered exact-matched pairs.
#[test]
fn overlapping_direct_shape_swap_is_redamaged() {
    // Coincident lines, so the overlap is the whole strip.
    const PROBE: Rect = Rect::new(38.0, 29.0, 2.0, 2.0);
    let line = |ui: &mut Ui, color: Color| {
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, 30.0),
            b: Vec2::new(70.0, 30.0),
            width: 8.0,
            brush: Brush::Solid(color),
            cap: LineCap::Round,
        });
    };
    let canvas = |ui: &mut Ui, first: Color, second: Color| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                line(ui, first);
                line(ui, second);
            });
    };
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| canvas(ui, BLUE, RED));
    frame(&mut ui, |ui| canvas(ui, RED, BLUE));

    let region = ui.damage_region();
    assert!(
        region.any_intersects(PROBE),
        "swapping two overlapping direct shapes must damage their \
         overlap; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin the survivor rule of the order check: inserting a child shifts
/// every later row's position in the parent's paint span, but the
/// survivors keep their *relative* order, so an unchanged shape drawn
/// after the children must contribute no damage — only the new child
/// does. (The old `child_rank` hash salt re-keyed every after-a-child
/// shape on insert and spuriously re-damaged its full extent.)
#[test]
fn inserting_a_child_does_not_redamage_unmoved_later_shapes() {
    const CHILD_A: Rect = Rect::new(10.0, 10.0, 30.0, 30.0);
    const CHILD_B: Rect = Rect::new(120.0, 10.0, 30.0, 30.0);
    // On the line, far below both children.
    const LINE_PROBE: Rect = Rect::new(30.0, 99.0, 2.0, 2.0);

    fn node(ui: &mut Ui, key: &str, r: Rect) {
        Frame::new()
            .id(WidgetId::from_hash(key))
            .position((r.min.x, r.min.y))
            .size(r.size.w)
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
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
                ui.add_shape(Shape::Line {
                    a: Vec2::new(10.0, 100.0),
                    b: Vec2::new(70.0, 100.0),
                    width: 4.0,
                    brush: Brush::Solid(RED),
                    cap: LineCap::Round,
                });
            });
    };
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| canvas(ui, false));
    frame(&mut ui, |ui| canvas(ui, true));

    let region = ui.damage_region();
    assert!(
        region.any_intersects(CHILD_B),
        "the inserted child must be damaged; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
    assert!(
        !region.any_intersects(LINE_PROBE),
        "an unchanged shape whose relative order is preserved must not \
         be re-damaged by a child insert; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin the re-key tradeoff: child identity lives in `node_hash` (via
/// the child markers `compute_hashes` folds), so re-keying a child —
/// same content, new `WidgetId` — flips its parent's hash and routes
/// the parent to the changed-paints arm. That arm must emit nothing
/// for the parent itself: the swapped marker rows are paint-empty, and
/// the re-keyed child's own pixels are damaged by its old id's
/// eviction plus its new id's insert. An unchanged sibling shape stays
/// clean.
#[test]
fn rekeying_a_child_damages_only_the_child() {
    const CHILD: Rect = Rect::new(10.0, 10.0, 30.0, 30.0);
    // On the line, far below the child.
    const LINE_PROBE: Rect = Rect::new(30.0, 99.0, 2.0, 2.0);

    let canvas = |ui: &mut Ui, key: &str| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash(key))
                    .position((CHILD.min.x, CHILD.min.y))
                    .size(CHILD.size.w)
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
                ui.add_shape(Shape::Line {
                    a: Vec2::new(10.0, 100.0),
                    b: Vec2::new(70.0, 100.0),
                    width: 4.0,
                    brush: Brush::Solid(RED),
                    cap: LineCap::Round,
                });
            });
    };
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| canvas(ui, "k1"));
    frame(&mut ui, |ui| canvas(ui, "k2"));

    let region = ui.damage_region();
    assert!(
        region.any_intersects(CHILD),
        "a re-keyed child must be damaged (evict + re-add); region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
    assert!(
        !region.any_intersects(LINE_PROBE),
        "the parent's unchanged sibling shape must not be re-damaged by \
         a child re-key; region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin: when a subtree's `(paint_rect, node_hash, subtree_hash,
/// cascade_input)` all match the prev-frame snapshot at its painting
/// root, the damage diff jumps to `subtree_end` instead of walking every
/// descendant. The fast path's correctness is already covered by every
/// "unchanged → no damage" test in this file; this pin specifically
/// guards that the jump *fires* — without it the path silently degrades
/// to a per-node walk that still produces correct damage.
#[test]
fn stable_painting_subtree_triggers_skip_jump() {
    let mut ui = Ui::for_test();
    // Frame with a painting parent (background) wrapping painting
    // children — both root and children land in `prev` with matching
    // snapshots on the second frame, so the root's Occupied-equal arm
    // is reached with a span > 1 and the skip counter increments.
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("painting_parent"))
                    .size((Sizing::Fixed(80.0), Sizing::Fixed(60.0)))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui, |ui| {
                        Frame::new()
                            .id(WidgetId::from_hash("child_a"))
                            .size(20.0)
                            .background(Background {
                                fill: RED.into(),
                                ..Default::default()
                            })
                            .show(ui);
                        Frame::new()
                            .id(WidgetId::from_hash("child_b"))
                            .size(20.0)
                            .background(Background {
                                fill: RED.into(),
                                ..Default::default()
                            })
                            .show(ui);
                    });
            });
    };
    frame(&mut ui, build);
    assert_eq!(
        ui.damage_subtree_skips(),
        0,
        "first frame populates prev — no prior snapshots to skip against"
    );

    frame(&mut ui, build);
    assert!(
        ui.damage_subtree_skips() >= 1,
        "identical second frame must skip at least the painting_parent subtree, got {}",
        ui.damage_subtree_skips(),
    );
    assert!(ui.damage_engine.dirty.is_empty());
}

/// Pin: a widget that loses its background between frames flips from
/// painting to non-painting. The diff must (a) contribute its prev
/// rect to damage so the prior pixels get cleared, (b) drop the entry
/// from `prev` so the next frame sees it as truly absent, and (c)
/// contribute no curr rect.
#[test]
fn paints_to_non_paints_transition_evicts_and_clears() {
    let mut ui = Ui::for_test();
    let with_bg = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };
    let no_bg = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .show(ui);
            });
    };
    frame(&mut ui, with_bg);
    let id = WidgetId::from_hash("a");
    assert!(ui.damage_engine.prev.contains_key(&id));

    frame(&mut ui, no_bg);
    assert!(
        !ui.damage_engine.prev.contains_key(&id),
        "paints→non-paints transition must evict the prev entry"
    );
    let rects: Vec<_> = ui.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 50.0, 50.0)],
        "damage must contain only the prev rect (curr doesn't paint)"
    );
}

/// Regression: a popup's full-surface invisible click-eater leaf must
/// not contribute to damage on add or remove. Otherwise opening or
/// dismissing a popup blows past the full-repaint coverage threshold.
/// Sole signal here is that filter stays `Partial` — no full-surface
/// rect lands in `region`.
#[test]
fn popup_eater_does_not_force_full_repaint() {
    let mut ui = Ui::for_test();
    let anchor = glam::Vec2::new(40.0, 40.0);
    // Frame 1: popup open. Eater (full-surface) + body (small).
    frame(&mut ui, |ui| {
        Popup::anchored_to(anchor)
            .id(WidgetId::from_hash("p"))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui, _popup| {
                Frame::new()
                    .id(WidgetId::from_hash("body-leaf"))
                    .size(60.0)
                    .background(Background {
                        fill: RED.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });

    // Frame 2: popup gone. Body + eater both removed. Without the
    // paints-gate, the eater's full-surface prev rect would dominate
    // the region.
    let out = ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
        Frame::new()
            .id(WidgetId::from_hash("placeholder"))
            .size(10.0)
            .show(ui);
    });
    let Some(RenderPlan {
        kind: RenderKind::Partial { region },
        ..
    }) = out.plan
    else {
        panic!(
            "popup dismissal escalated to {:?}; eater contributed full-surface \
             rect despite painting nothing",
            out.plan
        );
    };
    assert!(
        region.coverage < 0.5,
        "damage region covers {:.1}% of surface — eater leaked into damage",
        100.0 * region.coverage
    );
}

/// Regression: a click on empty background (no widget hit, no
/// state change) must not force the next paint to `Full`. The
/// discarded pre-pass in `run_frame` (triggered by any pointer /
/// key event via `frame_had_action`) calls `pre_record` →
/// `reset_to_idle`, then never reaches `post_record`. Pass 2's
/// `pre_record` then sees `frame_state == IDLE` and treats it as
/// "host dropped the previous frame", invalidating prev_surface
/// and forcing `Damage::Full`.
#[test]
fn click_on_empty_bg_does_not_force_full() {
    use crate::input::pointer::PointerButton;
    use std::time::Duration;
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size(50.0)
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };
    // Frame 0 (cold): expect Full. Submit.
    ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), build);
    ui.frame_state.mark_submitted();
    // Frame 1 (warm): nothing changed → Skip.
    let warm = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), build)
        .plan;
    assert!(warm.is_none(), "warm frame must Skip");
    ui.frame_state.mark_submitted();

    // Click on empty background (far from the 50×50 frame at origin).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(180.0, 180.0)));
    ui.on_input(InputEvent::PointerPressed(PointerButton::Left));
    ui.on_input(InputEvent::PointerReleased(PointerButton::Left));
    let click_plan = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), build)
        .plan;
    assert!(
        !matches!(
            click_plan,
            Some(RenderPlan {
                kind: RenderKind::Full,
                ..
            })
        ),
        "click on empty bg escalated to Full repaint: {click_plan:?}",
    );
}

/// Regression: a `Skip` frame that the host bypasses (no
/// `backend.submit` → no `mark_submitted`) must not force the next
/// frame to `Full`. `post_record` marks `Skip` as submitted directly so
/// the next `pre_record`'s auto-rewind doesn't kick in.
#[test]
fn skip_frame_does_not_force_next_to_full() {
    let mut ui = Ui::for_test();
    let first = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(matches!(
        first,
        Some(RenderPlan {
            kind: RenderKind::Full,
            ..
        })
    ));
    ui.frame_state.mark_submitted();

    // Identical content → Skip. WindowRenderer::render confirms submitted on
    // the skip path too (copies the backbuffer onto the swapchain);
    // the test mirrors that ack.
    let skip = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(skip.is_none(), "identical content must Skip");
    ui.frame_state.mark_submitted();

    // Next frame: still no diff. Pre-fix this could regress to Full if
    // the skip wasn't acked — WindowRenderer::render owns that ack now.
    let next = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(
        next.is_none(),
        "Skip frames must not poison the next frame into Full",
    );
}

/// Regression: a host that early-returns on `skip_render` (the natural
/// pattern — no swapchain acquire when there's nothing to paint, see
/// `examples/showcase/main.rs`) never calls `mark_submitted`. Without
/// `Ui::frame` self-acking skip frames, the next paint frame's
/// `classify_frame` saw `frame_skipped = true` and escalated
/// to `Full` — visible as a full-window red flash in the damage debug
/// overlay on every idle→input transition (e.g. mouse move).
#[test]
fn skip_frame_without_explicit_ack_does_not_force_next_to_full() {
    let mut ui = Ui::for_test();
    let first = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(matches!(
        first,
        Some(RenderPlan {
            kind: RenderKind::Full,
            ..
        })
    ));
    ui.frame_state.mark_submitted();

    // Identical content → Skip. WindowRenderer bypasses `render()` entirely and
    // never acks; `Ui::frame` must self-ack the skip.
    let skip = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, BLUE)
        })
        .plan;
    assert!(skip.is_none(), "identical content must Skip");
    // NOTE: deliberately no `mark_submitted` here.

    // Authoring change → expect `Partial`, not `Full`.
    let next = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            one_frame(ui, RED)
        })
        .plan;
    assert!(
        matches!(
            next,
            Some(RenderPlan {
                kind: RenderKind::Partial { .. },
                ..
            })
        ),
        "unacked skip poisoned next frame into Full: {next:?}",
    );
}

/// Pin: an authoring change on one leaf marks just that leaf
/// dirty; the parent (whose own fields didn't change and whose
/// rect is identical) stays clean.
#[test]
fn fill_change_marks_only_the_changed_leaf() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut ui, |ui| {
        one_frame(ui, RED);
    });

    assert_eq!(ui.damage_engine.dirty.len(), 1);
    let dirty_id = ui.damage_engine.dirty[0];
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("a")
    );
    // DamageEngine rect = Frame's rect (50x50 at (0,0)). Color change
    // doesn't move the rect, so prev == curr; the union is the
    // single rect.
    assert_eq!(
        ui.damage_region().iter_rects().next(),
        Some(ui.layout[Layer::Main].rect[dirty_id.idx()])
    );
}

/// Pin: a sibling reflow (Fixed-width sibling resizes) shifts
/// downstream rects — those neighbors are detected dirty by rect
/// comparison even though their authoring didn't change.
#[test]
fn sibling_reflow_marks_downstream_neighbor_dirty() {
    let mut ui = Ui::for_test();
    let build = |a_size: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("a"))
                    .size((Sizing::Fixed(a_size), Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("b"))
                    .size((Sizing::Fixed(30.0), Sizing::Fixed(20.0)))
                    .background(Background {
                        fill: Color::rgb(0.5, 0.5, 0.5).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };
    frame(&mut ui, |ui| build(50.0, ui));
    frame(&mut ui, |ui| build(80.0, ui));

    // `a` changed authoring (size). `b`'s authoring is unchanged
    // but its arranged x shifts from 50 → 80. Both are dirty.
    let dirty_ids: Vec<WidgetId> = ui
        .damage_engine
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("a")));
    assert!(dirty_ids.contains(&WidgetId::from_hash("b")));
}

/// Pin: a widget that disappears between frames contributes its
/// previous rect to damage — the renderer must repaint that
/// region to erase the leftover pixels.
#[test]
fn removed_widget_contributes_prev_rect_to_damage() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Button::new()
                    .id(WidgetId::from_hash("gone"))
                    .label("X")
                    .show(ui);
            });
    });
    let prev_button_rect = ui
        .damage_engine
        .prev_paint_rect(WidgetId::from_hash("gone"))
        .expect("gone painted last frame");

    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });

    // Button is gone; root Panel is non-painting (no chrome) so it
    // never entered prev. Only contribution is the Button's prev
    // rect, surfaced via the `removed` list.
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    assert_eq!(rects, vec![prev_button_rect]);
}

/// Pin: an added widget that wasn't in last frame contributes
/// its current rect to damage and lands in the dirty list.
#[test]
fn added_widget_contributes_curr_rect_to_damage() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |_| {});
    });
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("new"))
                    .size(50.0)
                    .background(Background {
                        fill: Color::rgb(0.2, 0.4, 0.8).into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    });

    let dirty_ids: Vec<WidgetId> = ui
        .damage_engine
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert!(dirty_ids.contains(&WidgetId::from_hash("new")));
    assert!(!ui.damage_region().rects.is_empty());
}

// --- Ui::damage_filter ---------------------------------------------------

/// Pin: a single-leaf fill flip stays in the partial-repaint regime —
/// `filter(surface)` returns `Partial(rect)`, because the rect is well
/// below the full-repaint threshold (50×50 = 2500 ≪ 200×200 surface).
#[test]
fn damage_filter_returns_partial_when_small() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        one_frame(ui, BLUE);
    });
    frame(&mut ui, |ui| {
        one_frame(ui, RED);
    });
    let region = ui.damage_region();
    let r = region
        .iter_rects()
        .next()
        .expect("single-leaf change → some damage");
    assert_eq!(Damage::new(ui.damage_region()), Damage::Partial(r.into()),);
}

// --- transforms ---------------------------------------------------------
// DamageEngine rects must be in *screen space*. When an ancestor has a
// transform, the rendered position of a node differs from its layout
// rect; the damage rect, the prev_frame snapshot, and the encoder/
// backend scissor all need to track that screen-space position.

/// Pin: when a transformed parent's child changes authoring, the
/// damage rect covers the child's *screen* rect (post-transform),
/// not its layout rect. Without this, the backend scissor would
/// clip the actual paint position and leave the screen unchanged.
#[test]
fn child_under_transformed_parent_damage_in_screen_space() {
    let translate = Vec2::new(100.0, 0.0);
    let mut ui = Ui::for_test();
    let mut child_node = None;
    let build = |fill: Color, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(translate))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: fill.into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(Color::rgb(0.2, 0.4, 0.8), &mut ui, &mut child_node);
    build(Color::rgb(0.9, 0.4, 0.8), &mut ui, &mut child_node);

    // Layout rect of the child is at the parent's inner origin (0, 0
    // in this layout). Screen rect after the parent's translate is at
    // (100, 0) — that's where the GPU actually paints. The damage
    // rect must cover *that* position, not the layout one.
    let child_layout_rect = ui.layout[Layer::Main].rect[child_node.unwrap().idx()];
    let expected_screen_rect = Rect {
        min: child_layout_rect.min + translate,
        size: child_layout_rect.size,
    };
    let region = ui.damage_region();
    let damage_rect = region
        .iter_rects()
        .next()
        .expect("child changed → some damage");
    assert!(
        damage_rect.min.x >= 100.0 - 0.5,
        "damage min.x must reflect parent translate; got {damage_rect:?}, expected near {expected_screen_rect:?}",
    );
    assert_eq!(damage_rect, expected_screen_rect);
}

/// Pin: animating a parent's transform shifts every child's screen
/// rect even though the children's authoring is unchanged. The
/// damage union must cover both prev and curr screen rects so the
/// backend repaints over the old positions too (otherwise the old
/// frame's pixels would streak through `LoadOp::Load`).
#[test]
fn animated_parent_transform_unions_old_and_new_positions() {
    let mut ui = Ui::for_test();
    let mut child_node = None;
    let build = |dx: f32, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(0.0, &mut ui, &mut child_node);
    build(50.0, &mut ui, &mut child_node);

    // Child layout rect didn't change. Parent's transform shifted by
    // (50, 0). Prev screen rect = (0,0,40,40); curr = (50,0,40,40);
    // gap of 10 px between them. bbox = 90×40 = 3600, sum = 3200,
    // SAH cost = 400 ≪ default budget — the merge rule collapses
    // into one bbox. (A *much* larger distance would push cost over
    // the budget; pinned by
    // `transform_animation_keeps_far_positions_split`.)
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(50.0, 0.0, 40.0, 40.0);
    assert_eq!(
        rects,
        vec![prev.union(curr)],
        "near transform animation → one merged bbox",
    );
    // The child is dirty: its authoring is unchanged but its screen
    // rect moved (rect comparison catches this). The parent lands on
    // the dirty list too — its self-transform is part of `node_hash`
    // (panel extras), so the changed transform routes it to the
    // changed-paints arm — but that arm emits nothing for it: its
    // only row (the child marker) is unchanged and its own
    // `cascade_input` is stable, so all damage comes from the child.
    let dirty_widget_ids: Vec<WidgetId> = ui
        .damage_engine
        .dirty
        .iter()
        .map(|n| ui.forest.tree(Layer::Main).records.widget_id()[n.idx()])
        .collect();
    assert_eq!(
        dirty_widget_ids,
        vec![WidgetId::from_hash("outer"), WidgetId::from_hash("c")],
    );
}

/// Sister case to the test above: under a tight pass-budget, a
/// far-apart transform animation keeps prev and curr screen rects
/// split. Pinning both ends of the merge rule means a budget tweak
/// can't silently flip behaviour without breaking a test.
#[test]
fn transform_animation_keeps_far_positions_split() {
    let mut ui = Ui::for_test();
    // Drop the merge budget to strict-overlap-only so the prev/curr
    // pair (cost 6 400 < default budget) stays split. Pins both
    // ends of the merge rule against future budget tweaks.
    ui.damage_engine.budget_px = 0.0;
    let mut child_node = None;
    let build = |dx: f32, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    *child = Some(
                        Frame::new()
                            .id(WidgetId::from_hash("c"))
                            .size(40.0)
                            .background(Background {
                                fill: Color::rgb(0.2, 0.4, 0.8).into(),
                                ..Default::default()
                            })
                            .show(ui)
                            .node(),
                    );
                });
        });
    };

    build(0.0, &mut ui, &mut child_node);
    build(200.0, &mut ui, &mut child_node);

    // prev (0,0,40,40) area 1600; curr (200,0,40,40) area 1600.
    // bbox 240×40 = 9600. SAH cost = 6400 — under the default
    // 20 000 budget, this would merge; the guard above drops the
    // budget to 0 to pin the strict-overlap-only branch.
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    let prev = Rect::new(0.0, 0.0, 40.0, 40.0);
    let curr = Rect::new(200.0, 0.0, 40.0, 40.0);
    assert_eq!(rects.len(), 2, "far transform animation → two rects");
    assert!(rects.contains(&prev) && rects.contains(&curr), "{rects:?}");
}

/// Soundness pin: when an ancestor's transform changes, a node whose
/// own `paint_rect` is **clipped invariant** (because its direct
/// shapes extend past the viewport / clip on every frame, so
/// `clip_to(...)` saturates to the same rect both passes) must still
/// contribute its `paint_rect` to damage. Otherwise the pixels of
/// those shapes — which DID move with the parent transform — get
/// stranded; the old positions never get cleared.
///
/// Repro of darkroom's "panning Scroll over a node-graph Canvas
/// leaves bezier trails": canvas's connection beziers are direct
/// shapes; canvas is wider than the viewport so its clipped paint
/// rect saturates; canvas's `node_hash` is stable but its
/// `cascade_input` shifts every pan frame.
#[test]
fn transform_shifted_direct_shape_with_invariant_clipped_paint_rect_contributes_damage() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    let mut ui = Ui::for_test();
    let build = |dx: f32, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(100, 100), |ui| {
            // Outermost clip pins descendants to the surface viewport
            // — without it, `parent_clip = None` and inner's paint
            // rect translates freely (the bug then doesn't manifest;
            // damage catches the rect change via the normal path).
            Panel::hstack()
                .id(WidgetId::from_hash("clip"))
                .clip_rect()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("xform"))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("inner"))
                                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                                .show(ui, |ui| {
                                    // Shape wider than the surface so
                                    // the clipped paint rect
                                    // saturates and stays invariant
                                    // under small `dx` translates.
                                    ui.add_shape(Shape::RoundedRect {
                                        local_rect: Some(Rect::new(-200.0, 0.0, 500.0, 50.0)),
                                        corners: Corners::ZERO,
                                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                                        stroke: Stroke::default(),
                                    });
                                });
                        });
                });
        });
    };
    build(0.0, &mut ui);
    build(5.0, &mut ui);
    let region = ui.damage_region();
    let covered = region.iter_rects().any(|r| {
        // Damage must cover the inner node's clipped paint area
        // (0..100 × 0..50) — that's where the shape's pixels live
        // both before and after the small pan.
        r.min.x <= 0.5 && r.min.y <= 0.5 && r.max().x >= 50.0 - 0.5 && r.max().y >= 50.0 - 0.5
    });
    assert!(
        covered,
        "ancestor-transform shift moves a direct-shape leaf's pixels; \
         damage must still cover the shape area even though the \
         clipped paint_rect is invariant. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Sister test to the soundness pin above: the new "cascade_input
/// shift on a direct-paint node → push `curr_rect`" branch in the
/// damage diff must not trip `FULL_REPAINT_THRESHOLD` for a pan of a
/// modestly-sized clip-saturated node. Same setup as that pin, but
/// repeated for several pan ticks; each step's damage stays
/// `Partial` and stays bounded to the inner clipped area.
#[test]
fn pan_with_invariant_clipped_paint_rect_stays_partial() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    let mut ui = Ui::for_test();
    let build = |dx: f32, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(100, 100), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("clip"))
                .clip_rect()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("xform"))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("inner"))
                                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                                .show(ui, |ui| {
                                    ui.add_shape(Shape::RoundedRect {
                                        local_rect: Some(Rect::new(-200.0, 0.0, 500.0, 50.0)),
                                        corners: Corners::ZERO,
                                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                                        stroke: Stroke::default(),
                                    });
                                });
                        });
                });
        });
    };
    build(0.0, &mut ui);
    for dx in [3.0, 6.0, 9.0, 12.0] {
        build(dx, &mut ui);
        let region = ui.damage_region();
        let damage = Damage::new(region);
        assert!(
            matches!(damage, Damage::Partial(_)),
            "pan with clip-saturated direct-paint node must stay Partial \
             (the new diff branch pushes one paint_rect per shifted node; \
             that must not blow past FULL_REPAINT_THRESHOLD on a single tick). \
             dx = {dx}, region = {:?}, damage = {damage:?}",
            region.iter_rects().collect::<Vec<_>>(),
        );
    }
}

/// Reproduces the darkroom graph-canvas regression: a panel with
/// `Panel::transform` and direct shapes (bezier connections) shifts
/// its own transform every pan frame. Under the `Panel::transform`
/// contract those shapes paint *inside* the self-transform, so their
/// tessellated pixels move — but `cascade_input` only tracks
/// ancestor state and stays put. The fix is at the source: own
/// transform now folds into `node_hash`, so the diff's
/// `e.get().hash == curr_node_hash` guard fails and the generic
/// Occupied arm pushes both prev and curr rects, sweeping where the
/// shapes were and are.
#[test]
fn self_transform_shift_damages_direct_shapes() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    let mut ui = Ui::for_test();
    let build = |dx: f32, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Panel::canvas()
                        .id(WidgetId::from_hash("xpanel"))
                        .size((Sizing::FILL, Sizing::FILL))
                        .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                        .show(ui, |ui| {
                            // Direct shape on the transformed panel —
                            // mirrors how darkroom adds connection
                            // beziers on the inner canvas.
                            ui.add_shape(Shape::RoundedRect {
                                local_rect: Some(Rect::new(40.0, 40.0, 30.0, 30.0)),
                                corners: Corners::ZERO,
                                fill: Color::rgb(0.2, 0.6, 0.9).into(),
                                stroke: Stroke::default(),
                            });
                        });
                });
        });
    };
    build(0.0, &mut ui);
    build(20.0, &mut ui);
    let region = ui.damage_region();

    // After translating self by dx=20, the shape's prev pixels lived
    // at [40, 70] × [40, 70] (translation 0) and the new pixels live
    // at [60, 90] × [40, 70]. Damage must cover both — i.e. at least
    // [40, 90] × [40, 70].
    let covered = region.iter_rects().any(|r| {
        r.min.x <= 40.5 && r.min.y <= 40.5 && r.max().x >= 90.0 - 0.5 && r.max().y >= 70.0 - 0.5
    });
    assert!(
        covered,
        "self-transform shift on a panel with direct shapes must \
         damage both old and new shape positions. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin the moved-subtree tier (tier 1.5): a transformed parent over an
/// authoring-identical subtree damages exactly `prev extent ∪ curr
/// extent`, and — the load-bearing part — the bulk snapshot refresh
/// leaves next frame's baseline intact:
///
/// - a second tick's damage is anchored at the *refreshed* positions
///   (if the refresh forgot to copy the rows' screens, damage would
///   still cover the original position);
/// - a still frame after the motion is a clean `Skip` (refreshed
///   `cascade_input` lets tier 1 skip at the subtree root).
#[test]
fn moved_subtree_damages_extents_and_refreshes_snapshots() {
    let mut ui = Ui::for_test();
    let build = |dx: f32, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("inner"))
                        .show(ui, |ui| {
                            for key in ["a", "b"] {
                                Frame::new()
                                    .id(WidgetId::from_hash(key))
                                    .size(40.0)
                                    .background(Background {
                                        fill: BLUE.into(),
                                        ..Default::default()
                                    })
                                    .show(ui);
                            }
                        });
                });
        });
    };

    build(0.0, &mut ui);

    // Tick 1: dx 0 → 30. "outer"'s own transform rides its node_hash
    // (panel extras), so outer takes the changed-paints arm (child
    // marker matches exactly — no damage); "inner"'s authoring is
    // untouched but its cascade prefix moved → tier 1.5. Subtree
    // extent = both 40×40 frames side by side: prev (0,0,80,40),
    // curr (30,0,80,40) — intersecting, so the region merges them
    // into one bbox.
    build(30.0, &mut ui);
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 110.0, 40.0)],
        "tick 1: prev ∪ curr subtree extents",
    );

    // Tick 2: dx 30 → 60. Damage must anchor at the tick-1 position —
    // its left edge is 30, not 0 — proving the tier refreshed the
    // rows' screens, not just `cascade_input`.
    build(60.0, &mut ui);
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(30.0, 0.0, 110.0, 40.0)],
        "tick 2: damage anchored at the refreshed (tick-1) extent",
    );

    // Still frame: identical dx → tier 1 skips at the root, no dirty
    // nodes, clean Skip. Fails loudly if the bulk refresh corrupted
    // any snapshot field.
    build(60.0, &mut ui);
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "still frame after motion must not dirty any node",
    );
    assert_eq!(
        Damage::new(ui.damage_region()),
        Damage::Skip,
        "still frame after motion",
    );
}

/// Sister pin: a *content* change under a constant transform must not
/// take the moved-subtree tier (`subtree_hash` differs) — the per-row
/// diff still produces leaf-tight damage, not the subtree extent.
#[test]
fn content_change_under_constant_transform_stays_row_tight() {
    let mut ui = Ui::for_test();
    let build = |fill: Color, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("outer"))
                .transform(TranslateScale::from_translation(Vec2::new(30.0, 0.0)))
                .show(ui, |ui| {
                    Panel::hstack()
                        .id(WidgetId::from_hash("inner"))
                        .show(ui, |ui| {
                            Frame::new()
                                .id(WidgetId::from_hash("a"))
                                .size(40.0)
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                            Frame::new()
                                .id(WidgetId::from_hash("b"))
                                .size(40.0)
                                .background(Background {
                                    fill: BLUE.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        });
                });
        });
    };
    build(BLUE, &mut ui);
    build(RED, &mut ui);
    // Only "a" changed; damage is its screen rect (layout 0..40 + the
    // 30 px transform), NOT the whole inner extent (which would reach
    // x = 110 and cover the untouched "b").
    let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(30.0, 0.0, 40.0, 40.0)],
        "fill flip under constant transform damages only the leaf",
    );
}

/// Soundness pin for the tier's entry-less leg: a node skipped by the
/// Vacant-arm off-surface filter (no `prev` snapshot) that scrolls
/// *into* view under tier 1.5 is covered by the curr-extent push, a
/// following still frame is a clean Skip (tier 1 at the subtree root —
/// the node legitimately stays entry-less), and a later content change
/// on it still lands damage via the Vacant insert arm.
#[test]
fn offscreen_node_scrolling_into_view_is_covered_and_stays_sound() {
    let mut ui = Ui::for_test();
    // Surface is 200×200 (test DISPLAY). Three 100-wide frames: "c"
    // starts at x = 200 — exactly off-surface (edge-touching rects
    // don't intersect), so its Vacant visit skips the snapshot insert.
    let build = |dx: f32, c_fill: Color, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("outer"))
            .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        for (key, fill) in [("a", BLUE), ("b", BLUE), ("c", c_fill)] {
                            Frame::new()
                                .id(WidgetId::from_hash(key))
                                .size((Sizing::Fixed(100.0), Sizing::Fixed(40.0)))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            });
    };
    frame(&mut ui, |ui| build(0.0, RED, ui));

    // Scroll left: "c" enters at (100..200). Tier 1.5 fires at
    // "inner"; "c" has no snapshot (off-surface skip last frame) but
    // the curr-extent push covers its pixels.
    let damage = frame(&mut ui, |ui| build(-100.0, RED, ui));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial, got {damage:?}");
    };
    let covers_c = region
        .iter_rects()
        .any(|r| r.min.x <= 100.5 && r.max().x >= 200.0 - 0.5 && r.max().y >= 40.0 - 0.5);
    assert!(
        covers_c,
        "curr-extent push must cover the newly revealed node. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Still frame: "c" is visible but entry-less — tier 1 skips at
    // the root and nothing is damaged, which is correct (no pixels
    // changed; they were painted by the scroll frame).
    let damage = frame(&mut ui, |ui| build(-100.0, RED, ui));
    assert_eq!(damage, Damage::Skip, "still frame with entry-less node");

    // Content change on the entry-less node: subtree hashes flip up
    // the chain, the walk descends, and "c" takes the Vacant insert
    // arm — its full rect is damage.
    let damage = frame(&mut ui, |ui| build(-100.0, BLUE, ui));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial, got {damage:?}");
    };
    let rects: Vec<Rect> = region.iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(100.0, 0.0, 100.0, 40.0)],
        "content change on an entry-less node damages its rect",
    );
}

// --- DamageEngine::filter heuristic ---------------------------------------------

const TEST_SURFACE: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

#[test]
fn no_damage_means_skip() {
    let d = DamageEngine::default();
    // No damage rect → `filter` returns `Skip` (no work to do; the
    // backbuffer already holds the right pixels). Distinct from
    // `Full` ("everything changed"), which is what coverage above
    // [`FULL_REPAINT_THRESHOLD`] produces.
    assert_eq!(
        Damage::new(DamageRegion::collapse_from(
            &d.raw_rects,
            d.budget_px,
            TEST_SURFACE
        )),
        Damage::Skip,
    );
}

/// Heuristic: total coverage = `sum(rect.area()) / surface_area`;
/// strictly above `FULL_REPAINT_THRESHOLD` (0.7) ⇒ Full, otherwise
/// Partial. The check is `>`, not `>=`, so coverage exactly at the
/// threshold stays Partial. `total_area` sums per-rect areas of the
/// post-merge region, so adjacent rects that the proximity-merge
/// rule collapses contribute their merged-bbox area (which here
/// equals the input sum since they tile cleanly). Inputs go through
/// `collapse_from` (the only constructor that seals `coverage`); the
/// `region()` helper builds the unsealed *expected* values, which
/// still match because coverage is excluded from `PartialEq`.
#[test]
fn damage_filter_threshold_cases() {
    use crate::ui::damage::region::{DEFAULT_PASS_BUDGET_PX, DamageRegion};
    fn region(rects: &[Rect]) -> DamageRegion {
        let mut r = DamageRegion::default();
        for rect in rects {
            r.add(*rect);
        }
        r
    }
    // Adjacent halves on the 100×100 surface — a perfectly adjacent
    // pair has `union_excess = bbox − a − b = 0`, below any positive
    // SAH budget, so each pair collapses into one rect whose area
    // equals the input sum. The region's `total_area` then lands
    // exactly at the threshold (or just above) and the strict `>`
    // decision logic is what's under test; the merge is guaranteed by
    // the zero excess alone, independent of the budget's exact value.
    const PAIR_BELOW: [Rect; 2] = [
        // Merges to Rect(0,0,70,100); total_area = 7000 / 10000 = 0.70
        // → stays Partial (`>` is strict).
        Rect::new(0.0, 0.0, 35.0, 100.0),
        Rect::new(35.0, 0.0, 35.0, 100.0),
    ];
    const PAIR_ABOVE: [Rect; 2] = [
        // Merges to Rect(0,0,72,100); total_area = 7200 / 10000 = 0.72
        // → escalates Full.
        Rect::new(0.0, 0.0, 36.0, 100.0),
        Rect::new(36.0, 0.0, 36.0, 100.0),
    ];
    let cases: &[(&str, &[Rect], Rect, Damage)] = &[
        (
            "small_1pct",
            &[Rect::new(0.0, 0.0, 10.0, 10.0)],
            TEST_SURFACE,
            Damage::Partial(Rect::new(0.0, 0.0, 10.0, 10.0).into()),
        ),
        (
            "large_81pct_above_threshold",
            &[Rect::new(0.0, 0.0, 90.0, 90.0)],
            TEST_SURFACE,
            Damage::Full,
        ),
        (
            "below_threshold_64pct_stays_partial",
            &[Rect::new(0.0, 0.0, 80.0, 80.0)],
            TEST_SURFACE,
            Damage::Partial(Rect::new(0.0, 0.0, 80.0, 80.0).into()),
        ),
        (
            "exact_70pct_stays_partial",
            &[Rect::new(0.0, 0.0, 70.0, 100.0)],
            TEST_SURFACE,
            Damage::Partial(Rect::new(0.0, 0.0, 70.0, 100.0).into()),
        ),
        (
            "two_rect_sum_at_threshold_stays_partial",
            &PAIR_BELOW,
            TEST_SURFACE,
            Damage::Partial(region(&PAIR_BELOW)),
        ),
        (
            "two_rect_sum_above_threshold_escalates_full",
            &PAIR_ABOVE,
            TEST_SURFACE,
            Damage::Full,
        ),
        // Zero-area-surface case dropped: `collapse_from` now asserts
        // `surface_area > EPS` (host filters resize-to-zero before we
        // ever reach this layer), so the prior `Damage::Full` fallback
        // became unreachable.
    ];
    for (label, rects, surface, want) in cases {
        let region = DamageRegion::collapse_from(rects, DEFAULT_PASS_BUDGET_PX, *surface);
        assert_eq!(Damage::new(region), *want, "case: {label}");
    }
}

/// Pin: a Display change between frames (resize or scale-factor)
/// forces the next compute to `Full` regardless of how few widgets
/// are dirty. The backend recreates the backbuffer / reshapes text
/// and a partial paint over a freshly cleared backbuffer would leave
/// the rest of the screen as clear color — the showcase resize-flicker
/// case.
#[test]
fn display_change_forces_full_repaint() {
    let cases: &[(&str, Display)] = &[
        (
            "resize_1px",
            Display {
                physical: UVec2::new(199, 200),
                ..DISPLAY
            },
        ),
        (
            "scale_factor",
            Display {
                scale_factor: 2.0,
                ..DISPLAY
            },
        ),
        // DPI-monitor move: physical and scale change proportionally,
        // leaving `logical_rect` bit-identical — yet the swapchain is
        // reconfigured to a new pixel size and must repaint. Comparing
        // logical rects alone classified this as Skip and the window
        // kept stale old-DPI content until unrelated damage arrived.
        (
            "dpi_move_constant_logical",
            Display {
                physical: UVec2::new(400, 400),
                scale_factor: 2.0,
                ..DISPLAY
            },
        ),
        // Snap flips change compose-time rasterization with identical
        // logical damage — same blind spot as the DPI move.
        (
            "pixel_snap_flip",
            Display {
                pixel_snap: false,
                ..DISPLAY
            },
        ),
    ];
    for (label, mutated) in cases {
        let mut ui = Ui::for_test();
        let mut build = |ui: &mut Ui| {
            one_frame(ui, BLUE);
        };

        // Steady-state: Full first frame, then Skip on identical re-record.
        let f1 = ui
            .frame(FrameStamp::new(DISPLAY, Duration::ZERO), &mut build)
            .plan;
        assert!(
            matches!(
                f1,
                Some(RenderPlan {
                    kind: RenderKind::Full,
                    ..
                })
            ),
            "case: {label} f1"
        );
        ui.frame_state.mark_submitted();
        let f2 = ui
            .frame(FrameStamp::new(DISPLAY, Duration::ZERO), &mut build)
            .plan;
        assert!(f2.is_none(), "case: {label} f2 must Skip");
        assert!(ui.damage_engine.dirty.is_empty(), "case: {label} steady");
        ui.frame_state.mark_submitted();

        // Mutate Display; identical authoring; must short-circuit to Full.
        let mutated_plan = ui
            .frame(FrameStamp::new(*mutated, Duration::ZERO), &mut build)
            .plan;
        assert!(
            matches!(
                mutated_plan,
                Some(RenderPlan {
                    kind: RenderKind::Full,
                    ..
                })
            ),
            "case: {label} display change"
        );
        ui.frame_state.mark_submitted();
        assert!(
            !ui.damage_engine.dirty.is_empty(),
            "case: {label} display change should mark some nodes dirty (rects shifted)",
        );

        // Stable surface at the new size, identical authoring → back to Skip.
        let stable = ui
            .frame(FrameStamp::new(*mutated, Duration::ZERO), &mut build)
            .plan;
        assert!(
            stable.is_none(),
            "case: {label} post-mutation steady must Skip",
        );
        assert!(
            ui.damage_engine.dirty.is_empty(),
            "case: {label} post-mutation dirty empty"
        );
    }
}

/// Pin (precise bug reproducer): the showcase resize-flicker fired
/// when surface changed AND the damage rect was small enough to fall
/// below the area threshold — only a few descendants shifted while
/// the root and most others were stable. Without the surface-change
/// short-circuit, `compute` returns `Some(small_rect)` and the
/// encoder produces a damage-filtered partial paint, but the backend
/// force-clears the freshly recreated backbuffer, leaving the rest of
/// the screen as clear color.
///
/// The test uses a Fixed-size root so descendant rects are stable
/// across surface changes; a tiny injected nudge to one descendant's
/// `prev` snapshot would, absent the short-circuit, produce a small
/// partial damage rect on the resize frame.
#[test]
fn small_damage_with_surface_change_forces_full_repaint() {
    let mut ui = Ui::for_test();
    let big = Display {
        physical: UVec2::new(2000, 2000),
        ..DISPLAY
    };
    // Root: Fixed-size VStack containing two Fixed children. Stacked
    // vertically so both children's `paint_rect`s land inside the
    // 2000×2000 surface — required since the Vacant arm in the diff
    // skips inserting an off-surface widget into `prev` (no visible
    // pixels to track). Root rect is stable across surface changes
    // (Fixed never reads `available`), so any damage-rect change
    // must come from the descendant nudge, not the root re-resolving.
    // Frame "small" ends up at (0, 60, 50, 60).
    let mut scene = |ui: &mut Ui| {
        Panel::vstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::Fixed(60.0), Sizing::Fixed(120.0)))
            .show(ui, |ui| {
                Frame::new()
                    .id(WidgetId::from_hash("big"))
                    .size((60.0, 60.0))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
                Frame::new()
                    .id(WidgetId::from_hash("small"))
                    .size((50.0, 60.0))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui);
            });
    };

    ui.frame(FrameStamp::new(big, Duration::ZERO), &mut scene);
    ui.frame_state.mark_submitted();
    ui.frame(FrameStamp::new(big, Duration::ZERO), &mut scene);
    ui.frame_state.mark_submitted();
    assert!(ui.damage_engine.dirty.is_empty());

    // Inject: flip widget "small"'s prev `cascade_input` so the next
    // diff sees it as a cascade-state change and damages its paint_rect
    // (50×60 = 3000 area) inside a 2000×2000 surface (4M area) —
    // ratio ≈ 0.075%, well below the full-repaint threshold.
    let target_wid = WidgetId::from_hash("small");
    let snap = ui
        .damage_engine
        .prev
        .get_mut(&target_wid)
        .expect("small in prev");
    snap.cascade_input = CascadeInputHash(snap.cascade_input.0 ^ 1);

    let smaller = Display {
        physical: UVec2::new(1999, 2000),
        ..big
    };
    let resize_plan = ui
        .frame(FrameStamp::new(smaller, Duration::ZERO), &mut scene)
        .plan;

    assert!(
        matches!(
            resize_plan,
            Some(RenderPlan {
                kind: RenderKind::Full,
                ..
            })
        ),
        "small-damage + surface-change must force full repaint \
         (this is the showcase resize-flicker case — encoder would emit a \
         damage-filtered partial paint over a backend-cleared backbuffer)",
    );
}

/// Pin (negative): a stable surface across many frames does *not*
/// fire the surface-change short-circuit on every frame. This guards
/// the alpha-mode / present-mode / swapchain-recreated-but-backbuffer-
/// kept scenarios from the damage layer's POV — they all leave the
/// surface rect unchanged, so damage must pass through to the normal
/// dirty/threshold logic. Without this guarantee partial repaint
/// would never apply.
#[test]
fn stable_surface_does_not_short_circuit() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui, color: Color| {
        one_frame(ui, color);
    };

    // Warm up: two identical frames bring damage to steady state.
    ui.frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
        build(ui, BLUE)
    });
    ui.frame_state.mark_submitted();
    let warm = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            build(ui, BLUE)
        })
        .plan;
    assert!(warm.is_none(), "warm steady-state must Skip");
    assert!(ui.damage_engine.dirty.is_empty());
    ui.frame_state.mark_submitted();

    // Frame 3: same surface, *one leaf* changes color. Diff must
    // produce a `Partial(small_rect)`, not `Full`/`Skip` — that
    // proves the surface-change short-circuit didn't fire.
    let changed = ui
        .frame(FrameStamp::new(DISPLAY, Duration::ZERO), |ui| {
            build(ui, RED)
        })
        .plan;
    let Some(RenderPlan {
        kind: RenderKind::Partial { region },
        ..
    }) = changed
    else {
        panic!(
            "stable surface + one-leaf change should produce a partial \
             repaint, got {changed:?} — surface-change short-circuit fired incorrectly",
        );
    };
    // DamageEngine rect = the 50×50 frame's rect. Well below 50% of 200×200.
    assert!(
        region.coverage < 0.5,
        "damage region should be small (partial repaint range), got {region:?}",
    );
}

/// Pin (motivating workload): hovering a button causes exactly one
/// node — the button — to be dirty, with damage rect == button's
/// rect. This is the bread-and-butter case Stage 3 is designed for:
/// pointer hover changes a small region; partial repaint should win.
///
/// Hit-test response lags by one frame (recording reads last frame's
/// state), so we run enough frames at each pointer position to let
/// the damage stream settle, then assert on the *transition* frame.
#[test]
fn button_hover_damage_covers_only_the_button() {
    let mut ui = Ui::for_test();
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |ui: &mut Ui, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
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

    // Pointer parked off-button. Settle for two frames so hit-test +
    // damage are at steady state (no diff).
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "off-button pointer should reach a no-diff steady state"
    );

    let hot_rect = ui.layout[Layer::Main].rect[hot_node.unwrap().idx()];
    let target = hot_rect.min + Vec2::new(5.0, 5.0);

    // Move pointer onto the hot button. The *next* post_record computes
    // hover=true. The frame *after* that records the button as
    // hovered → its fill differs → it lands in the dirty set alone.
    // `on_input` recomputes hover against the existing hit_index
    // immediately, so the *next* recording sees `hovered=true` and
    // emits the hovered fill. DamageEngine = button rect only.
    ui.on_input(InputEvent::PointerMoved(target));
    build(&mut ui, &mut hot_node, &mut cold_node);

    assert_eq!(
        ui.damage_engine.dirty.len(),
        1,
        "only the hovered button should be dirty"
    );
    let dirty_id = ui.damage_engine.dirty[0];
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[dirty_id.idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::new(ui.damage_region()),
        Damage::Partial(hot_rect.into()),
        "small per-button damage must not trip the full-repaint heuristic",
    );

    // Next frame at same cursor → no diff (settled).
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "settled hover should produce no further damage"
    );
}

/// Pin: leaving the button (un-hover) is symmetric — the only diff
/// is the button's fill flipping back, damage = button rect.
#[test]
fn button_unhover_damage_covers_only_the_button() {
    let mut ui = Ui::for_test();
    let mut hot_node = None;
    let mut cold_node = None;
    let build = |ui: &mut Ui, hot: &mut Option<NodeId>, cold: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
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

    // Settle two frames with cursor over the hot button.
    build(&mut ui, &mut hot_node, &mut cold_node);
    let hot_rect = ui.layout[Layer::Main].rect[hot_node.unwrap().idx()];
    ui.on_input(InputEvent::PointerMoved(hot_rect.min + Vec2::new(5.0, 5.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert!(ui.damage_engine.dirty.is_empty(), "settled hover");

    // Pointer leaves the button.
    ui.on_input(InputEvent::PointerMoved(Vec2::new(380.0, 380.0)));
    build(&mut ui, &mut hot_node, &mut cold_node);
    assert_eq!(ui.damage_engine.dirty.len(), 1);
    assert_eq!(
        ui.forest.tree(Layer::Main).records.widget_id()[ui.damage_engine.dirty[0].idx()],
        WidgetId::from_hash("hot"),
    );
    assert_eq!(ui.damage_region().iter_rects().next(), Some(hot_rect));
    assert_eq!(
        Damage::new(ui.damage_region()),
        Damage::Partial(hot_rect.into()),
    );
}

/// Pin: a child whose layout rect overflows a clipped panel (e.g. a
/// scrolled-offscreen row inside a `Scroll` viewport) contributes
/// only its *visible* portion to the damage region. The fix replaces
/// `Cascade.screen_rect` with `Cascade.visible_rect` (raw screen rect
/// intersected with the active ancestor clip) as the damage rect
/// source — without it, panning a long list under a small viewport
/// would inflate the damage union to the full content extent and
/// trip `FULL_REPAINT_THRESHOLD` every frame.
#[test]
fn child_overflowing_clipped_parent_damage_clipped_to_viewport() {
    let mut ui = Ui::for_test();
    let mut child_node = None;
    let viewport_size = 100.0;
    let child_size = 200.0;
    let build = |fill: Color, ui: &mut Ui, child: &mut Option<NodeId>| {
        ui.run_at_acked(UVec2::new(400, 400), |ui| {
            // Root hstack so the inner zstack honors its `Fixed` size
            // (root nodes get stretched to the surface anchor by the
            // layout engine, which would defeat the clip).
            Panel::hstack()
                .id(WidgetId::from_hash("clip-host"))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("clip-root"))
                        .size((Sizing::Fixed(viewport_size), Sizing::Fixed(viewport_size)))
                        .clip_rect()
                        .show(ui, |ui| {
                            *child = Some(
                                Frame::new()
                                    .id(WidgetId::from_hash("overflow"))
                                    .size(child_size)
                                    .background(Background {
                                        fill: fill.into(),
                                        ..Default::default()
                                    })
                                    .show(ui)
                                    .node(),
                            );
                        });
                });
        });
    };

    build(BLUE, &mut ui, &mut child_node);
    // Authoring change on the child only — fill flips. The child's
    // layout rect is `child_size × child_size` (way past the clip),
    // but the damage rect must stay inside the parent's clip.
    build(RED, &mut ui, &mut child_node);

    let region = ui.damage_region();
    let damage_rect = region
        .iter_rects()
        .next()
        .expect("child changed → some damage");
    assert!(
        damage_rect.size.w <= viewport_size + 0.5 && damage_rect.size.h <= viewport_size + 0.5,
        "damage rect must be clipped to the {viewport_size}px viewport; got {damage_rect:?}",
    );
}

/// Pin: a node that paints a drop shadow contributes its **inflated**
/// paint bounds (rect + `|offset| + 3σ + spread` on each side) to the
/// damage region, not just the arranged rect. Both routes — direct
/// `Shape::Shadow` push and `Background::shadow` chrome — must reach
/// the same `paint_rect` so a tab swap clears the full halo, not just
/// the layout rect.
#[test]
fn drop_shadow_overhang_contributes_to_damage_on_remove() {
    use crate::Shadow;
    use crate::primitives::corners::Corners;
    use crate::shape::Shape;

    let frame_size = 50.0;
    let expected_overhang = 3.0 * 8.0 + 2.0;

    type Build = fn(&mut Ui);
    let cases: &[(&str, Build)] = &[
        ("shape", |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("card"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .show(ui, |ui| {
                    ui.add_shape(Shape::Shadow {
                        local_rect: None,
                        corners: Corners::all(0.0),
                        shadow: Shadow {
                            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                            offset: Vec2::ZERO,
                            blur: 8.0,
                            spread: 2.0,
                            inset: false,
                        },
                    });
                });
        }),
        ("chrome", |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("card"))
                .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                .background(Background {
                    fill: BLUE.into(),
                    shadow: Shadow {
                        color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                        offset: Vec2::ZERO,
                        blur: 8.0,
                        spread: 2.0,
                        inset: false,
                    },
                    ..Default::default()
                })
                .show(ui, |_| {});
        }),
    ];
    for (label, build) in cases {
        let mut ui = Ui::for_test();
        frame(&mut ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, build);
        });
        let prev_rect = ui
            .damage_engine
            .prev_paint_rect(WidgetId::from_hash("card"))
            .expect("card painted last frame");
        assert!(
            prev_rect.size.w >= frame_size + 2.0 * expected_overhang - 0.5
                && prev_rect.size.h >= frame_size + 2.0 * expected_overhang - 0.5,
            "[{label}] snapshot rect must include drop-shadow overhang; got {prev_rect:?}",
        );

        frame(&mut ui, |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("root"))
                .show(ui, |_| {});
        });
        let rects: Vec<Rect> = ui.damage_region().iter_rects().collect();
        // `damage_engine.prev` stores the raw paint_rect including
        // the shadow halo, which extends off the top-left of the
        // 200×200 surface for a 50×50 frame at origin. The damage
        // region, however, clips each rect to the surface in
        // `collapse_from` (off-surface pixels can never be painted
        // and would bias the Full-repaint threshold), so the emitted
        // damage is the visible portion of `prev_rect`.
        assert_eq!(
            rects,
            vec![prev_rect.intersect(TEST_SURFACE)],
            "[{label}] damage region",
        );
    }
}

/// Pin: a drop-shadow whose halo extends past a clipping ancestor
/// contributes only the **clipped** halo to damage. The shadow's
/// overhang is folded into `paint_rect` in owner-local space before
/// the ancestor clip is applied, so a `ClipMode::Clip` parent caps
/// the contribution at the parent's bounds — otherwise the halo
/// pretends to paint pixels the GPU's scissor will discard.
#[test]
fn shadow_overhang_inside_clipped_parent_is_clamped() {
    use crate::Shadow;
    use crate::primitives::corners::Corners;
    use crate::shape::Shape;

    let viewport = 60.0;
    let card = 40.0;
    let blur = 8.0;

    let mut ui = Ui::for_test();
    let build = |fill: Color, ui: &mut Ui| {
        ui.run_at_acked(UVec2::new(200, 200), |ui| {
            Panel::hstack()
                .id(WidgetId::from_hash("host"))
                .show(ui, |ui| {
                    Panel::zstack()
                        .id(WidgetId::from_hash("viewport"))
                        .size((Sizing::Fixed(viewport), Sizing::Fixed(viewport)))
                        .clip_rect()
                        .show(ui, |ui| {
                            Panel::hstack()
                                .id(WidgetId::from_hash("card"))
                                .size((Sizing::Fixed(card), Sizing::Fixed(card)))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui, |ui| {
                                    ui.add_shape(Shape::Shadow {
                                        local_rect: None,
                                        corners: Corners::all(0.0),
                                        shadow: Shadow {
                                            color: Color::rgba(0.0, 0.0, 0.0, 0.5),
                                            offset: Vec2::ZERO,
                                            blur,
                                            spread: 0.0,
                                            inset: false,
                                        },
                                    });
                                });
                        });
                });
        });
    };

    build(BLUE, &mut ui);
    build(RED, &mut ui);

    for r in ui.damage_region().iter_rects() {
        assert!(
            r.size.w <= viewport + 0.5 && r.size.h <= viewport + 0.5,
            "shadow halo damage must stay inside the {viewport}px clip; got {r:?}",
        );
    }
}

/// `DamageRegion::collapse_from` intersects each input rect with the
/// surface before folding it into the region. Without this, a
/// paint_rect whose bounds extend past the viewport (root-level
/// transformed canvas with no clip ancestor, plus high zoom —
/// `parent_clip` stays `None` so `cascade::compute_paint_rect` never
/// clips down) would inflate `total_area` past the threshold despite
/// only a tiny visible fraction. Reproduces the darkroom graph
/// pan/zoom regression where a few zoomed-up node panels off-screen
/// would force `Damage::Full` each pan tick.
#[test]
fn partial_when_oversized_rect_lies_mostly_off_surface() {
    let surface = Rect::new(0.0, 0.0, 100.0, 100.0);
    // 1000×1000 paint_rect anchored at (90, 90): only a 10×10 corner
    // pokes into the surface, the rest sticks off-screen. Pre-fix:
    // rect.area() = 1e6, ratio = 1e6 / 1e4 = 100 ⇒ Full. Post-fix:
    // collapse_from clips to (90,90,10,10), area = 100, ratio = 0.01
    // ≪ 0.7 ⇒ Partial.
    let oversized = Rect::new(90.0, 90.0, 1000.0, 1000.0);
    assert_eq!(
        oversized.intersect(surface),
        Rect::new(90.0, 90.0, 10.0, 10.0),
        "sanity: 1000×1000 rect at (90,90) intersects surface in a 10×10 corner",
    );
    let region = DamageRegion::collapse_from(&[oversized], f32::INFINITY, surface);
    // Region stores the clipped rect, not the raw input.
    let stored: Vec<_> = region.iter_rects().collect();
    assert_eq!(
        stored,
        vec![Rect::new(90.0, 90.0, 10.0, 10.0)],
        "collapse_from must store the surface-clipped rect, not the raw input",
    );
    let damage = Damage::new(region);
    assert!(
        matches!(damage, Damage::Partial(_)),
        "off-surface inflation must not trip FULL_REPAINT_THRESHOLD; got {damage:?}",
    );
}

/// Sister to the above: a rect that *fully* covers the surface
/// (regardless of how much extends past) still trips Full. The intent
/// of the surface-clamp is "don't count pixels that can't be painted,"
/// not "don't ever Full" — when the visible portion is the whole
/// viewport, Full is still the right call.
#[test]
fn full_when_visible_portion_covers_surface_even_if_rect_overflows() {
    let surface = Rect::new(0.0, 0.0, 100.0, 100.0);
    let covers_all_plus_overflow = Rect::new(-50.0, -50.0, 1000.0, 1000.0);
    let region = DamageRegion::collapse_from(&[covers_all_plus_overflow], f32::INFINITY, surface);
    let damage = Damage::new(region);
    assert_eq!(
        damage,
        Damage::Full,
        "rect that covers entire surface (plus overflow) must still trip Full",
    );
}

/// A rect that lies entirely off the surface contributes nothing to
/// the region (zero-area after clipping, dropped). Pins the "early-out
/// on degenerate clip" branch in `collapse_from`.
#[test]
fn fully_off_surface_rect_is_dropped_from_region() {
    let surface = Rect::new(0.0, 0.0, 100.0, 100.0);
    let off_screen = Rect::new(500.0, 500.0, 50.0, 50.0);
    let region = DamageRegion::collapse_from(&[off_screen], f32::INFINITY, surface);
    assert!(
        region.rects.is_empty(),
        "wholly-off-surface rect must produce an empty region (no Damage::Skip vs Partial drift)",
    );
}

/// First-seen Vacant arm short-circuits when `curr_rect` lies entirely
/// off the surface. The hashmap insert and rect push would both be
/// wasted: the rect is dropped by `collapse_from`'s surface-clip
/// downstream, and the prev entry would just describe an invisible
/// snapshot that the next frame's diff would have to evict. Pins the
/// pan/zoom workload where a node panned past the viewport edge
/// contributes nothing useful to damage bookkeeping.
#[test]
fn off_surface_first_seen_node_skips_prev_insert() {
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        // Wrap in a transformed parent: `Panel::transform` applies to
        // the body (children), so the inner panel's chrome paint_rect
        // = parent_transform.apply_rect(inner.layout_rect). With a
        // (+500,+500) parent translate over a 200×200 surface, the
        // inner panel's chrome lands at (500,500,50,50) — wholly off.
        Panel::canvas()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::FILL, Sizing::FILL))
            .transform(TranslateScale::from_translation(Vec2::new(500.0, 500.0)))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("off"))
                    .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui, |_| {});
            });
    });

    assert!(
        !ui.damage_engine
            .prev
            .contains_key(&WidgetId::from_hash("off")),
        "Vacant + off-surface paint_rect must not seed a prev entry — \
         hashmap insert + raw_rects push are both wasted work for a \
         node that contributes nothing visible",
    );
    assert!(
        ui.damage_region().rects.is_empty(),
        "no visible widgets means no damage rects on the second-frame \
         diff (first frame is Full and walks differently)",
    );
}

/// `NodeSnapshot.paint_span` covers one entry per Paint row on the
/// node — chrome at row 0 when present, then each direct shape — with
/// matching rect and canonical hash. Mirrors `Cascades::paint_arenas`.
#[test]
fn node_snapshot_decomposition_matches_cascade() {
    use crate::Shape;
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("multi"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(Shape::Line {
                    a: Vec2::new(0.0, 0.0),
                    b: Vec2::new(10.0, 10.0),
                    width: 1.0,
                    brush: Color::rgb(1.0, 0.0, 0.0).into(),
                    cap: LineCap::Butt,
                });
                ui.add_shape(Shape::Line {
                    a: Vec2::new(20.0, 20.0),
                    b: Vec2::new(30.0, 30.0),
                    width: 1.0,
                    brush: Color::rgb(0.0, 1.0, 0.0).into(),
                    cap: LineCap::Butt,
                });
            });
    });

    let snap = ui.damage_engine.prev[&WidgetId::from_hash("multi")];
    let layer = Layer::Main;
    let node_idx = ui.cascades.by_id[&WidgetId::from_hash("multi")].node.idx();
    let node_span = ui.cascades.layers[layer].paint_arena.node_spans[node_idx];
    let layer_paints = &ui.cascades.layers[layer].paint_arena.rows;

    // Chrome lands at row 0 of the node's paint span when present.
    let chrome_paint = layer_paints[node_span.start as usize];
    assert!(
        chrome_paint.screen.area() > 0.0,
        "chrome panel must have non-zero chrome rect",
    );

    // Snapshot mirrors the cascade arena slice.
    let snap_paints = &ui.damage_engine.arena.snaps[snap.paint_span.range()];
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

    // The force-full first frame skips the Vacant pushes (its region
    // is discarded) — the buffer stays empty.
    assert!(ui.damage_engine.raw_rects.is_empty());

    // A widget added on an *incremental* frame hits the same Vacant
    // arm with the pushes live: one rect per paint row (chrome + each
    // shape). The unchanged "multi" subtree-skips and contributes
    // nothing, so the buffer holds exactly the newcomer's rows.
    let two_lines = |ui: &mut Ui| {
        ui.add_shape(Shape::Line {
            a: Vec2::new(0.0, 0.0),
            b: Vec2::new(10.0, 10.0),
            width: 1.0,
            brush: Color::rgb(1.0, 0.0, 0.0).into(),
            cap: LineCap::Butt,
        });
        ui.add_shape(Shape::Line {
            a: Vec2::new(20.0, 20.0),
            b: Vec2::new(30.0, 30.0),
            width: 1.0,
            brush: Color::rgb(0.0, 1.0, 0.0).into(),
            cap: LineCap::Butt,
        });
    };
    frame(&mut ui, |ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("multi"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| two_lines(ui));
        Panel::hstack()
            .id(WidgetId::from_hash("multi2"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| two_lines(ui));
    });
    let snap2 = ui.damage_engine.prev[&WidgetId::from_hash("multi2")];
    let snap2_paints = &ui.damage_engine.arena.snaps[snap2.paint_span.range()];
    assert_eq!(
        ui.damage_engine.raw_rects.len(),
        3,
        "incremental Vacant insert pushes one rect per paint row",
    );
    assert_eq!(ui.damage_engine.raw_rects[0], snap2_paints[0].screen);
    assert_eq!(ui.damage_engine.raw_rects[1], snap2_paints[1].screen);
    assert_eq!(ui.damage_engine.raw_rects[2], snap2_paints[2].screen);
}

/// Slice 4 headline: a multi-shape owner whose shapes are spatially
/// disjoint pushes only the *changed* shape's rect pair on a frame
/// where one endpoint moved. Reproduces the darkroom graph pattern
/// (canvas owns N bezier connections; drag one node, only the
/// connections actually touching it should enter damage). Pre-slice-4
/// the Occupied-changed arm pushed `prev_rect ∪ curr_rect = union of
/// all shapes`; slice 4 pushes only the moved shape's prev + curr.
#[test]
fn per_shape_damage_only_pushes_changed_shapes() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    // Two stable shapes (drawn at fixed coords) + one shape whose
    // endpoint shifts between frames. Frame N records all three;
    // frame N+1 shifts only the third — the diff must push exactly
    // that shape's pair of rects.
    let mut ui = Ui::for_test();
    let build = |moving_y: f32, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::Fixed(180.0), Sizing::Fixed(180.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 20.0, 10.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(60.0, 0.0, 20.0, 10.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(0.0, 1.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                // The moving shape, far from the other two, so its
                // bbox doesn't merge with theirs in the damage region.
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, moving_y, 20.0, 10.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(0.0, 0.0, 1.0).into(),
                    stroke: Stroke::ZERO,
                });
            });
    };

    // Frame 1 (cold) and frame 2 (steady — no diff).
    frame(&mut ui, |ui| build(120.0, ui));
    frame(&mut ui, |ui| build(120.0, ui));
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "steady frame must produce no diff"
    );

    // Frame 3 nudges shape 2's y endpoint. Slice 4 contract: only
    // shape 2's prev rect (at y=120) and curr rect (at y=140) enter
    // the damage region. Chrome (canvas background) is unchanged in
    // geometry AND authoring → no chrome push. Shapes 0 and 1 are
    // bit-identical → no push.
    let prev_snap = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    // paint_snaps row 0 is chrome; shapes follow at offset 1.
    let prev_shape2_rect = ui.damage_engine.arena.snaps[prev_snap.paint_span.range()][1 + 2].screen;
    frame(&mut ui, |ui| build(140.0, ui));

    let canvas_snap = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    let curr_shape2_rect =
        ui.damage_engine.arena.snaps[canvas_snap.paint_span.range()][1 + 2].screen;

    // The damage region must intersect both old and new positions of
    // shape 2 (so the pixels-at-old-position get cleared and
    // pixels-at-new-position get painted). It must NOT intersect the
    // disjoint regions occupied by shapes 0 and 1 — those didn't move.
    let region = ui.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));
    assert!(
        intersects(prev_shape2_rect),
        "old position of moved shape must be in damage region; \
         prev_rect = {prev_shape2_rect:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
    assert!(
        intersects(curr_shape2_rect),
        "new position of moved shape must be in damage region; \
         curr_rect = {curr_shape2_rect:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Sentinel: a rect on the chrome's top edge between shapes 0/1
    // (y < 120) must NOT be in the region — chrome didn't change,
    // shapes 0/1 are unchanged, only the moving shape's y-band gets
    // damaged. Pre-slice-4 the whole `paint_rect` union (covering
    // the entire 180×180 canvas) would have hit. This is the
    // tight-damage win.
    let stale_chrome_band = Rect::new(40.0, 40.0, 20.0, 20.0); // inside chrome, away from moved shape
    assert!(
        !intersects(stale_chrome_band),
        "unchanged chrome interior must not enter damage; \
         stale_band = {stale_chrome_band:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Chrome authoring change (hover fill flip, no rect change) must
/// push the chrome rect even though the geometric rect is identical.
/// Chrome is row 0 of the node's paint span and carries its own
/// authoring hash via `Paint.hash`; without that, a hover-color flip
/// would fall through the rect-only guard and emit no damage at all.
#[test]
fn chrome_authoring_change_pushes_chrome_paint_row() {
    let mut ui = Ui::for_test();
    let build = |fill: Color, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("c"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: fill.into(),
                ..Default::default()
            })
            .show(ui, |_| {});
    };
    frame(&mut ui, |ui| build(BLUE, ui));
    frame(&mut ui, |ui| build(BLUE, ui)); // settle
    let snap = ui.damage_engine.prev[&WidgetId::from_hash("c")];
    let snap_rect = ui.damage_engine.arena.snaps[snap.paint_span.start as usize].screen;

    frame(&mut ui, |ui| build(RED, ui));
    let region = ui.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    assert!(
        rects.iter().any(|r| r.intersects(snap_rect)),
        "chrome authoring change must push chrome paint row even when \
         rect geometry is unchanged; region = {rects:?}",
    );
}

/// Ordinal shift: when the user removes a shape from the middle of a
/// widget's authoring (e.g., deletes a connection in the middle of a
/// connection list), the per-shape diff sees the trailing ordinals as
/// "different" because they now align with a *different* prev shape.
/// The contract: damage stays correct (the removed shape's pixels +
/// the shifted shapes' old+new positions all enter the region), and
/// the snapshot tail is trimmed via the `drain(ord..)` branch in the
/// Occupied-changed arm.
///
/// This is the degraded-coarsening behaviour mentioned in the design
/// doc — frame stays correct, one frame of over-paint, settles next.
#[test]
fn shape_removed_from_middle_evicts_trailing_ordinals() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;

    let mut ui = Ui::for_test();
    let build = |include_middle: bool, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::Fixed(180.0), Sizing::Fixed(60.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(0.0, 0.0, 20.0, 20.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                if include_middle {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new(60.0, 0.0, 20.0, 20.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.0, 1.0, 0.0).into(),
                        stroke: Stroke::ZERO,
                    });
                }
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(Rect::new(120.0, 0.0, 20.0, 20.0)),
                    corners: Corners::ZERO,
                    fill: Color::rgb(0.0, 0.0, 1.0).into(),
                    stroke: Stroke::ZERO,
                });
            });
    };

    frame(&mut ui, |ui| build(true, ui));
    frame(&mut ui, |ui| build(true, ui)); // settle

    // Snapshot the prev rects for shapes 0/1/2 so we can verify the
    // post-delete damage region.
    let prev = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    // Chromeless canvas ⇒ paint_snaps maps 1:1 to direct shapes.
    let prev_shapes = &ui.damage_engine.arena.snaps[prev.paint_span.range()];
    assert_eq!(prev_shapes.len(), 3);
    let prev_middle_rect = prev_shapes[1].screen;
    let prev_blue_rect = prev_shapes[2].screen;

    // Delete the middle shape. Content-keyed matching pairs red→red
    // and blue→blue between frames (same `(screen, hash)` despite the
    // ordinal shift); only the green paint is unmatched. Damage covers
    // green's prev rect and nothing else.
    frame(&mut ui, |ui| build(false, ui));

    let post = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    assert_eq!(
        post.paint_span.len, 2,
        "snapshot tail must be trimmed to the new paint count",
    );

    let region = ui.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    let intersects = |r: Rect| rects.iter().any(|d| d.intersects(r));

    // The deleted shape's pixels must be in damage (cleared this frame).
    assert!(
        intersects(prev_middle_rect),
        "deleted shape's prev rect must enter damage; \
         prev_middle = {prev_middle_rect:?}, region = {rects:?}",
    );
    // The blue shape never moved (positioned absolutely via local_rect)
    // and its content is unchanged — content-keyed matching detects
    // this and excludes it from damage. The damaged region must NOT
    // intersect blue's rect.
    assert!(
        !intersects(prev_blue_rect),
        "unmoved blue shape must not enter damage; \
         prev_blue = {prev_blue_rect:?}, region = {rects:?}",
    );
}

/// Symmetric to `shape_removed_from_middle_…`: inserting a new shape
/// between two existing ones shifts every trailing ordinal, but with
/// content-keyed matching the existing shapes pair with their prev
/// counterparts and only the new shape contributes damage.
#[test]
fn shape_added_in_middle_damages_only_new() {
    use crate::Shape;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;

    let mut ui = Ui::for_test();
    let red_rect = Rect::new(0.0, 0.0, 20.0, 20.0);
    let green_rect = Rect::new(60.0, 0.0, 20.0, 20.0);
    let blue_rect = Rect::new(120.0, 0.0, 20.0, 20.0);
    let build = |include_middle: bool, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::Fixed(180.0), Sizing::Fixed(60.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(red_rect),
                    corners: Corners::ZERO,
                    fill: Color::rgb(1.0, 0.0, 0.0).into(),
                    stroke: Stroke::ZERO,
                });
                if include_middle {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(green_rect),
                        corners: Corners::ZERO,
                        fill: Color::rgb(0.0, 1.0, 0.0).into(),
                        stroke: Stroke::ZERO,
                    });
                }
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(blue_rect),
                    corners: Corners::ZERO,
                    fill: Color::rgb(0.0, 0.0, 1.0).into(),
                    stroke: Stroke::ZERO,
                });
            });
    };

    frame(&mut ui, |ui| build(false, ui)); // red + blue
    frame(&mut ui, |ui| build(false, ui)); // settle

    let prev = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    let prev_shapes: Vec<_> = ui.damage_engine.arena.snaps[prev.paint_span.range()].to_vec();
    assert_eq!(prev_shapes.len(), 2);
    let prev_red_screen = prev_shapes[0].screen;
    let prev_blue_screen = prev_shapes[1].screen;

    frame(&mut ui, |ui| build(true, ui)); // insert green between

    let post = ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    assert_eq!(post.paint_span.len, 3);

    let curr_shapes: Vec<_> = ui.damage_engine.arena.snaps[post.paint_span.range()].to_vec();
    let region = ui.damage_region();
    let rects: Vec<_> = region.iter_rects().collect();
    let intersects = |r: Rect| rects.iter().any(|d| d.intersects(r));

    // Green has no prev counterpart — its curr screen rect enters
    // damage as "added."
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
    // Red and blue paints are bit-identical between frames (same
    // `(screen, hash)`); content-keyed matching pairs them off and
    // they must not enter damage despite their ordinal shifting.
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

/// Painting-only invariant: every `DamageEngine.prev` entry covers
/// at least one Paint row. A chrome-only owner used to land in `prev`
/// with `shape_span.len == 0` (chrome was tracked in a separate
/// column); under the unified `paint_arena`, chrome is row 0 of the
/// node's span, so the same owner now has `paint_span.len == 1`.
/// `compact_paint_snaps` asserts on zero-len entries — this test
/// pins the producer side of that contract.
#[test]
fn chrome_only_owner_has_nonzero_paint_span() {
    let mut ui = Ui::for_test();
    let build = |ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("chrome_only"))
            .size((Sizing::Fixed(50.0), Sizing::Fixed(50.0)))
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            })
            .show(ui, |_| {});
    };
    frame(&mut ui, build);
    frame(&mut ui, build); // settle prev

    let wid = WidgetId::from_hash("chrome_only");
    let snap = ui.damage_engine.prev[&wid];
    assert_eq!(
        snap.paint_span.len, 1,
        "chrome-only owner must contribute exactly one Paint row (chrome)",
    );

    // Every entry in `prev` covers at least one row.
    for (k, s) in &ui.damage_engine.prev {
        assert!(
            s.paint_span.len > 0,
            "prev entry {k:?} has zero-len paint_span, violating painting-only invariant",
        );
    }

    // Compaction must accept the live state without tripping the
    // invariant assert.
    ui.damage_engine.compact_paint_snaps(&ui.forest);
}

/// Pin: changing the *content* of a `Shape::Text` with
/// `local_origin: Some(_)` damages the shaped-text bbox, not just the
/// origin point.
///
/// Before the fix, `paint_bbox_local` for `Text { local_origin: Some(_) }`
/// returned `{ min: origin, size: ZERO }` — a degenerate point, because
/// the glyph extent isn't known to the record. Cascade dutifully stored
/// that point in `Cascades::shape_rects[idx]`; the diff's
/// `diff_changed_shape_leg` then pushed two zero-size rects when text
/// changed → effectively no damage from the text shape. The
/// user-visible symptom: type a character in a `TextEdit`, and only the
/// caret-sized strip got repainted while the rest of the text went
/// stale.
///
/// Post-fix, cascade looks up the shaped extent from
/// `LayerLayout::text_shapes` (already computed by the measure pass)
/// and stores the tight `(origin, measured)` rect. The diff pushes
/// prev + curr extents, so the damage region covers the union of both
/// strings' bboxes.
#[test]
fn text_content_change_damages_shaped_extent_not_just_origin() {
    use crate::forest::element::{Element, LayoutMode, Salt};
    use crate::primitives::size::Size;
    use crate::shape::{Shape, TextWrap};
    use crate::text::{FontFamily, FontWeight};

    let mut ui = Ui::for_test();
    // Mono fallback geometry: glyph width = font_size_px * 0.5, line
    // height = font_size_px. With font_size_px = 14, "abc" measures
    // 21×14 and "abcdef" measures 42×14.
    const FONT: f32 = 14.0;
    const ORIGIN: Vec2 = Vec2::new(10.0, 10.0);
    let leaf_id = WidgetId::from_hash("text-host");
    let build = |text: &'static str, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("root"))
            .size((Sizing::Fixed(100.0), Sizing::Fixed(50.0)))
            .show(ui, |ui| {
                let mut element = Element::new(LayoutMode::Leaf);
                element.salt = Salt::Verbatim(leaf_id);
                ui.node(leaf_id, element, None, |ui| {
                    ui.add_shape(Shape::Text {
                        local_origin: Some(ORIGIN),
                        text: text.into(),
                        brush: Color::WHITE.into(),
                        font_size_px: FONT,
                        line_height_px: FONT,
                        wrap: TextWrap::Truncate,
                        align: Default::default(),
                        family: FontFamily::Sans,
                        weight: FontWeight::Regular,
                    });
                });
            });
    };

    frame(&mut ui, |ui| build("abc", ui));
    frame(&mut ui, |ui| build("abc", ui));
    assert!(
        ui.damage_engine.dirty.is_empty(),
        "steady frame must produce no diff"
    );

    // Cache prev shaped rect (size of "abc") off the previous snapshot
    // so the assertion below can reason from the actual measured
    // values rather than hand-recomputing mono geometry.
    // Damage rects inflate by `TEXT_SCALE_STEP * measured` total per
    // axis (`STEP/2` per side) to cover composer ladder snaps — see
    // `text_paint_bbox_local`. Expected shaped size scales by the
    // same factor.
    let inflate = 1.0 + TEXT_SCALE_STEP;
    let prev_snap = ui.damage_engine.prev[&leaf_id];
    let prev_text_rect = ui.damage_engine.arena.snaps[prev_snap.paint_span.range()][0].screen;
    let prev_size_short: Size = Size::new(FONT * 0.5 * 3.0 * inflate, FONT * inflate);
    assert!(
        (prev_text_rect.size.w - prev_size_short.w).abs() < 0.5
            && (prev_text_rect.size.h - prev_size_short.h).abs() < 0.5,
        "prev text rect should have shaped size ≈ {prev_size_short:?}, got {prev_text_rect:?}",
    );

    frame(&mut ui, |ui| build("abcdef", ui));

    let curr_snap = ui.damage_engine.prev[&leaf_id];
    let curr_text_rect = ui.damage_engine.arena.snaps[curr_snap.paint_span.range()][0].screen;
    let curr_size_long: Size = Size::new(FONT * 0.5 * 6.0 * inflate, FONT * inflate);
    assert!(
        (curr_text_rect.size.w - curr_size_long.w).abs() < 0.5
            && (curr_text_rect.size.h - curr_size_long.h).abs() < 0.5,
        "curr text rect should have shaped size ≈ {curr_size_long:?}, got {curr_text_rect:?}",
    );

    let region = ui.damage_region();
    let intersects = |r: Rect| region.iter_rects().any(|d| d.intersects(r));

    // Probe deep inside the new "abcdef" rect but past where the old
    // "abc" rect ended (x = origin.x + 30 ≈ middle of "abcdef", past
    // the 21-px width of "abc"). Pre-fix this point is *not* in damage
    // (per-shape rect was a zero-size point at origin); post-fix it is
    // (curr rect spans origin..origin+42px).
    let inside_new_only = Rect::new(ORIGIN.x + 30.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_new_only),
        "probe inside new text but past old text must be in damage; \
         probe = {inside_new_only:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Also assert prev's middle gets damaged (so the old glyph
    // pixels actually clear).
    let inside_old = Rect::new(ORIGIN.x + 10.0, ORIGIN.y + 5.0, 1.0, 1.0);
    assert!(
        intersects(inside_old),
        "probe inside old text must be in damage; \
         probe = {inside_old:?}, region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );
}

/// Pin: a direct shape on a clipped node has its per-shape rect (the
/// column the damage diff reads from) clipped to the node's own clip
/// mask — not just the ancestor clip.
///
/// Before the fix, `compute_paint_rect` clipped each shape's screen
/// rect to `parent_clip` only. A `Shape::Text` with `local_origin`
/// expressing a scroll offset reported its **full** shaped extent as
/// the per-shape rect (cosmic-text's measured `Size` for the whole
/// buffer). For a multi-line `TextEdit` taller than its visible rect,
/// scrolling produced damage rects spanning the entire text — way
/// past the editor's own `ClipMode::Rect`. The encoder's GPU scissor
/// clips the actual pixels, so the user *saw* tight repaints, but
/// the damage region driving the scissor pass was over-large,
/// inflating the partial-redraw quad to the unclipped text bbox.
///
/// This test fakes the scenario with a `RoundedRect` shape extending
/// past the host's clip on the right edge; pre-fix the per-shape rect
/// captures the full 400-px-wide shape, post-fix it's clipped to the
/// host's deflated mask.
#[test]
fn direct_shape_on_clipped_node_clips_to_own_mask() {
    use crate::Shape;
    use crate::forest::Layer;
    use crate::primitives::corners::Corners;
    use crate::primitives::stroke::Stroke;
    // WindowRenderer panel: 80×40, padding 4 each side via background. The
    // direct shape extends to x=400 (well past 80). After the cascade
    // walk, `shape_rects[idx]` must be clipped to the host's deflated
    // mask, not span the full 400 px.
    let mut ui = Ui::for_test();
    let host_id = WidgetId::from_hash("clip-host");
    let build = |ui: &mut Ui| {
        Panel::hstack().auto_id().show(ui, |ui| {
            Panel::hstack()
                .id(host_id)
                .size((Sizing::Fixed(80.0), Sizing::Fixed(40.0)))
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .clip_rect()
                .show(ui, |ui| {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect::new(0.0, 0.0, 400.0, 20.0)),
                        corners: Corners::ZERO,
                        fill: Color::rgb(1.0, 0.0, 0.0).into(),
                        stroke: Stroke::ZERO,
                    });
                });
        });
    };
    frame(&mut ui, build);
    frame(&mut ui, build);

    // Locate the host node by widget id and read its first shape's
    // cascaded screen rect. Pre-fix the rect spans the full 400 px;
    // post-fix it's clamped to (host_width − padding-fold).
    let cascades = &ui.cascades;
    let host_ep = *cascades.by_id.get(&host_id).expect("host node recorded");
    let host_entry_idx = (cascades.layers[host_ep.layer].entries_base + host_ep.node.0) as usize;
    let host_rect = cascades.entries.rect()[host_entry_idx];
    let tree = ui.forest.tree(Layer::Main);
    let shape_span = tree.records.shape_span()[host_ep.node.idx()];
    assert!(shape_span.len >= 1, "host should have at least one shape");
    // The host paints chrome (the BLUE background), so row 0 of its
    // span is the chrome `Paint` — whose screen always equals the
    // 80×40 arranged rect and would pass the assertion below even
    // with the clip regressed. The direct shape under test is row 1.
    let paint_arena = &cascades.layers[Layer::Main].paint_arena;
    let node_span = paint_arena.node_spans[host_ep.node.idx()];
    assert!(node_span.len >= 2, "expected chrome row + shape row");
    let shape_rect = paint_arena.rows[node_span.start as usize + 1].screen;
    assert!(
        shape_rect.size.w <= host_rect.size.w + 0.5,
        "direct shape rect must be clipped to the host's own mask; \
         host_rect = {host_rect:?}, shape_rect = {shape_rect:?}",
    );
}

/// Pin: a visibility flip landing on the SAME frame as a paint-row
/// change must still damage the exact-matched rows. The union push for
/// a `cascade_input` change used to be gated on `geometry_unchanged`,
/// so hiding a node while one of its shapes was mid-change damaged
/// only the changed shape — the chrome and untouched shapes kept
/// their stale pixels on screen.
#[test]
fn visibility_flip_with_coincident_shape_change_damages_whole_node() {
    // Chrome corner far from the line, so its damage is geometrically
    // distinguishable from the changed shape's.
    const CHROME_PROBE: Rect = Rect::new(44.0, 44.0, 2.0, 2.0);
    const LINE_PROBE: Rect = Rect::new(10.0, 9.0, 2.0, 2.0);
    let node = |ui: &mut Ui, hidden: bool, color: Color| {
        let mut p = Panel::zstack()
            .id(WidgetId::from_hash("a"))
            .size(50.0)
            .background(Background {
                fill: BLUE.into(),
                ..Default::default()
            });
        if hidden {
            p = p.hidden();
        }
        p.show(ui, |ui| {
            ui.add_shape(Shape::Line {
                a: Vec2::new(5.0, 10.0),
                b: Vec2::new(20.0, 10.0),
                width: 2.0,
                brush: Brush::Solid(color),
                cap: LineCap::Round,
            });
        });
    };
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| node(ui, false, BLUE));
    let damage = frame(&mut ui, |ui| node(ui, true, RED));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial, got {damage:?}");
    };
    assert!(
        region.any_intersects(LINE_PROBE),
        "changed shape's own rect must be damaged",
    );
    assert!(
        region.any_intersects(CHROME_PROBE),
        "exact-matched chrome must also clear when the node hides; region = {region:?}",
    );
}

/// Pin: reparenting a widget at an identical rect with identical
/// content must damage its painted extent. Both parents are chromeless
/// full-surface ZStacks, so the leaf's arranged rect, authoring hash,
/// and cascade input are all bit-identical across the move — only its
/// compositing position changed (`NodeSnapshot::parent_key`). The
/// pre-fix tier-1 skip treated the leaf as unchanged and the frame
/// classified Skip, leaving stale overlap pixels wherever the leaf's
/// z-order against outside content flipped.
#[test]
fn reparent_at_same_rect_damages_moved_subtree() {
    const LEAF_PROBE: Rect = Rect::new(10.0, 10.0, 2.0, 2.0);
    let build = |ui: &mut Ui, under_b: bool| {
        let leaf = |ui: &mut Ui| {
            Frame::new()
                .id(WidgetId::from_hash("L"))
                .size(30.0)
                .background(Background {
                    fill: BLUE.into(),
                    ..Default::default()
                })
                .show(ui);
        };
        Panel::zstack()
            .id(WidgetId::from_hash("root"))
            .show(ui, |ui| {
                Panel::zstack()
                    .id(WidgetId::from_hash("A"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        if !under_b {
                            leaf(ui);
                        }
                    });
                Panel::zstack()
                    .id(WidgetId::from_hash("B"))
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        if under_b {
                            leaf(ui);
                        }
                    });
            });
    };
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| build(ui, false));
    let damage = frame(&mut ui, |ui| build(ui, true));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial for the moved leaf, got {damage:?}");
    };
    assert!(
        region.any_intersects(LEAF_PROBE),
        "moved leaf's extent must be damaged; region = {region:?}",
    );
    // Follow-up frame with no further move settles back to Skip — the
    // refreshed snapshot carries the new parent_key.
    let settled = frame(&mut ui, |ui| build(ui, true));
    assert_eq!(settled, Damage::Skip, "reparent damage must not repeat");
}

/// Pin: inserting one shape at the FRONT of a node's record stream
/// (every row shifts by one) damages only the new shape — the shifted
/// rows exact-match by content through the keyed merge and their
/// relative order is preserved, so no inversion overlap fires either.
#[test]
fn front_insert_damages_only_the_new_shape() {
    const NEW_PROBE: Rect = Rect::new(150.0, 149.0, 2.0, 2.0);
    const OLD_PROBE: Rect = Rect::new(30.0, 19.0, 2.0, 2.0);
    let line = |ui: &mut Ui, y: f32| {
        ui.add_shape(Shape::Line {
            a: Vec2::new(10.0, y),
            b: Vec2::new(70.0, y),
            width: 2.0,
            brush: Brush::Solid(BLUE),
            cap: LineCap::Round,
        });
    };
    let build = |ui: &mut Ui, with_front: bool| {
        Panel::canvas()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                if with_front {
                    ui.add_shape(Shape::Line {
                        a: Vec2::new(140.0, 150.0),
                        b: Vec2::new(170.0, 150.0),
                        width: 2.0,
                        brush: Brush::Solid(RED),
                        cap: LineCap::Round,
                    });
                }
                line(ui, 20.0);
                line(ui, 30.0);
                line(ui, 40.0);
            });
    };
    let mut ui = Ui::for_test();
    frame(&mut ui, |ui| build(ui, false));
    let damage = frame(&mut ui, |ui| build(ui, true));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial, got {damage:?}");
    };
    assert!(
        region.any_intersects(NEW_PROBE),
        "inserted shape must be damaged; region = {region:?}",
    );
    assert!(
        !region.any_intersects(OLD_PROBE),
        "shifted-but-identical rows must not re-damage; region = {region:?}",
    );
}
