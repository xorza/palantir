//! What moving, adding and removing nodes damages.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::Color, rect::Rect};
use crate::scene::damage::Damage;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::scene::node::Configure;
use crate::shape::Shape;
use crate::shape::style::LineCap;
use crate::ui::harness::UiHarness;
use crate::widgets::{button::Button, frame::Frame, panel::Panel};
use glam::Vec2;

/// Pin: removing a child of a fixed-size canvas that paints its own
/// direct shapes must **not** re-damage those shapes. A node's
/// `node_hash` folds in a per-immediate-child marker (`compute_rollups`),
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
                ui.add_shape(
                    Shape::line(Vec2::new(120.0, 120.0), Vec2::new(180.0, 180.0), 2.0)
                        .brush(BLUE)
                        .cap(LineCap::Round),
                );
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
            .size((Sizing::fixed(30.0), Sizing::fixed(30.0)))
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
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, [a, b]));
    // Same positions + content, only draw order flips.
    frame(&mut h, |ui| canvas(ui, [b, a]));

    assert!(
        h.damage_region().rects.is_empty(),
        "reordering nodes must not damage unchanged leaves; region = {:?}",
        h.damage_region().iter_rects().collect::<Vec<_>>(),
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
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, [a, b, c]));
    // Raise `a` to the front (drawn last) — same positions + content.
    frame(&mut h, |ui| canvas(ui, [b, c, a]));

    let region = h.damage_region();
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

    // The reorder costs exactly the frame it happens on. Once the
    // snapshot holds the new order the rows match positionally, so the
    // changed-paints arm reports `geometry_unchanged` and the inversion
    // scan — which is O(rows²) once it fires — is never entered again.
    // Worth pinning: the scan's cost is bearable precisely because it is
    // one frame per raise rather than one per frame the order stays
    // flipped.
    frame(&mut h, |ui| canvas(ui, [b, c, a]));
    assert!(
        h.ui.damage_engine.counters.dirty().is_empty(),
        "a settled reorder must re-damage nothing; dirty = {:?}",
        h.ui.damage_engine.counters.dirty(),
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
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, [a, b]));
    frame(&mut h, |ui| canvas(ui, [b, a]));

    assert!(
        h.damage_region().rects.is_empty(),
        "off-screen text must not fabricate edge-of-window damage on \
         reorder; region = {:?}",
        h.damage_region().iter_rects().collect::<Vec<_>>(),
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
            .size((Sizing::fixed(40.0), Sizing::fixed(20.0)))
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
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| stack(ui, [a, b])); // a in the top slot, b below
    frame(&mut h, |ui| stack(ui, [b, a])); // swapped

    // Both slots changed content (colours swapped), so both must be
    // damaged — top slot y=[0,20], bottom y=[20,40].
    let region = h.damage_region();
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
        ui.add_shape(
            Shape::line(Vec2::new(10.0, 40.0), Vec2::new(70.0, 40.0), 4.0)
                .brush(BLUE)
                .cap(LineCap::Round),
        );
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

    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, over);
    frame(&mut h, under);

    let region = h.damage_region();
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
        ui.add_shape(
            Shape::line(Vec2::new(10.0, 30.0), Vec2::new(70.0, 30.0), 8.0)
                .brush(color)
                .cap(LineCap::Round),
        );
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
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| canvas(ui, BLUE, RED));
    frame(&mut h, |ui| canvas(ui, RED, BLUE));

    let region = h.damage_region();
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
                ui.add_shape(
                    Shape::line(Vec2::new(10.0, 100.0), Vec2::new(70.0, 100.0), 4.0)
                        .brush(RED)
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
/// the child markers `compute_rollups` folds), so re-keying a child —
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
                ui.add_shape(
                    Shape::line(Vec2::new(10.0, 100.0), Vec2::new(70.0, 100.0), 4.0)
                        .brush(RED)
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

    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |include_middle: bool, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::fixed(180.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                ui.add_shape(
                    Shape::rect(Rect::new(0.0, 0.0, 20.0, 20.0)).fill(Color::rgb(1.0, 0.0, 0.0)),
                );
                if include_middle {
                    ui.add_shape(
                        Shape::rect(Rect::new(60.0, 0.0, 20.0, 20.0))
                            .fill(Color::rgb(0.0, 1.0, 0.0)),
                    );
                }
                ui.add_shape(
                    Shape::rect(Rect::new(120.0, 0.0, 20.0, 20.0)).fill(Color::rgb(0.0, 0.0, 1.0)),
                );
            });
    };

    frame(&mut h, |ui| build(true, ui));
    frame(&mut h, |ui| build(true, ui)); // settle

    // Snapshot the prev rects for shapes 0/1/2 so we can verify the
    // post-delete damage region.
    let prev = h.ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    // Chromeless canvas ⇒ paint_snaps maps 1:1 to direct shapes.
    let prev_shapes = &h.ui.damage_engine.arena.paints.slots[prev.paint_span.range()];
    assert_eq!(prev_shapes.len(), 3);
    let prev_middle_rect = prev_shapes[1].screen;
    let prev_blue_rect = prev_shapes[2].screen;

    // Delete the middle shape. Content-keyed matching pairs red→red
    // and blue→blue between frames (same `(screen, hash)` despite the
    // ordinal shift); only the green paint is unmatched. Damage covers
    // green's prev rect and nothing else.
    frame(&mut h, |ui| build(false, ui));

    let post = h.ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    assert_eq!(
        post.paint_span.len, 2,
        "snapshot tail must be trimmed to the new paint count",
    );

    let region = h.damage_region();
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

    let mut h = UiHarness::new(DISPLAY.physical);
    let red_rect = Rect::new(0.0, 0.0, 20.0, 20.0);
    let green_rect = Rect::new(60.0, 0.0, 20.0, 20.0);
    let blue_rect = Rect::new(120.0, 0.0, 20.0, 20.0);
    let build = |include_middle: bool, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("canvas"))
            .size((Sizing::fixed(180.0), Sizing::fixed(60.0)))
            .show(ui, |ui| {
                ui.add_shape(Shape::rect(red_rect).fill(Color::rgb(1.0, 0.0, 0.0)));
                if include_middle {
                    ui.add_shape(Shape::rect(green_rect).fill(Color::rgb(0.0, 1.0, 0.0)));
                }
                ui.add_shape(Shape::rect(blue_rect).fill(Color::rgb(0.0, 0.0, 1.0)));
            });
    };

    frame(&mut h, |ui| build(false, ui)); // red + blue
    frame(&mut h, |ui| build(false, ui)); // settle

    let prev = h.ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    let prev_shapes: Vec<_> =
        h.ui.damage_engine.arena.paints.slots[prev.paint_span.range()].to_vec();
    assert_eq!(prev_shapes.len(), 2);
    let prev_red_screen = prev_shapes[0].screen;
    let prev_blue_screen = prev_shapes[1].screen;

    frame(&mut h, |ui| build(true, ui)); // insert green between

    let post = h.ui.damage_engine.prev[&WidgetId::from_hash("canvas")];
    assert_eq!(post.paint_span.len, 3);

    let curr_shapes: Vec<_> =
        h.ui.damage_engine.arena.paints.slots[post.paint_span.range()].to_vec();
    let region = h.damage_region();
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
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| build(ui, false));
    let damage = frame(&mut h, |ui| build(ui, true));
    let Damage::Partial(region) = damage else {
        panic!("expected Partial for the moved leaf, got {damage:?}");
    };
    assert!(
        region.any_intersects(LEAF_PROBE),
        "moved leaf's extent must be damaged; region = {region:?}",
    );
    // Follow-up frame with no further move settles back to Skip — the
    // refreshed snapshot carries the new parent_key.
    let settled = frame(&mut h, |ui| build(ui, true));
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
        ui.add_shape(
            Shape::line(Vec2::new(10.0, y), Vec2::new(70.0, y), 2.0)
                .brush(BLUE)
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
                        Shape::line(Vec2::new(140.0, 150.0), Vec2::new(170.0, 150.0), 2.0)
                            .brush(RED)
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
