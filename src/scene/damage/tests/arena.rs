//! What the paint-snapshot storage does under churn: blocks recycle in
//! place, live spans never move, and no frame pays for another frame's
//! shape churn.
//!
//! These are the properties that replaced compaction. The old arena
//! appended every count change to the tail and reseated every live span
//! once orphans passed 75% — correct, but the reseat landed whole on one
//! frame in N, which is exactly the lumpy cost this crate rejects
//! elsewhere. What that bought is now free, and these tests are what say
//! so.
//!
//! Row counts throughout are `1 + shapes`: a canvas contributes its
//! chrome at row 0 and then one row per shape. [`Paint::GRANULE`] is
//! one, so a block is exactly its span's length and a row count is its
//! own size class — which is what makes every arena length below a
//! sum of row counts rather than of rounded-up capacities.
//!
//! [`Paint::GRANULE`]: crate::common::block_arena::BlockSlot::GRANULE

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::color::Color;
use crate::primitives::rect::Rect;
use crate::primitives::span::Span;
use crate::primitives::widget_id::WidgetId;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, frame, one_frame};
use crate::scene::node::Configure;
use crate::shape::Shape;
use crate::ui::harness::UiHarness;
use crate::widgets::panel::Panel;

/// A canvas holding `shapes` rects, all inside its own box so nothing is
/// culled off-surface.
fn canvas(ui: &mut Ui, id: &'static str, shapes: u32) {
    Panel::hstack()
        .id(WidgetId::from_hash(id))
        .size((Sizing::fixed(180.0), Sizing::fixed(90.0)))
        .background(Background {
            fill: BLUE.into(),
            ..Default::default()
        })
        .show(ui, |ui| {
            for s in 0..shapes {
                ui.add_shape(
                    Shape::rect(Rect::new(
                        (s % 9) as f32 * 20.0,
                        (s / 9) as f32 * 20.0,
                        8.0,
                        8.0,
                    ))
                    .fill(Color::rgb(0.1 * s as f32, 0.4, 0.6)),
                );
            }
        });
}

fn arena_len(h: &UiHarness) -> usize {
    h.ui.damage_engine.paints.slots.len()
}

fn free_classes(h: &UiHarness) -> usize {
    h.ui.damage_engine.paints.classes_with_free_blocks()
}

fn span_of(h: &UiHarness, id: &'static str) -> Span {
    h.ui.damage_engine.prev[&WidgetId::from_hash(id)].paint_span
}

/// The headline: a node whose paint-row count changes every frame
/// reaches a steady state where the storage stops growing outright.
///
/// This is the workload compaction existed for. Each toggle releases the
/// old block and takes the other count's, so once both classes have been
/// seen the same two blocks trade back and forth forever — no tail
/// growth, and therefore nothing to reclaim.
#[test]
fn a_toggling_shape_count_trades_two_blocks_forever() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |shapes: u32| move |ui: &mut Ui| canvas(ui, "canvas", shapes);

    // 3 shapes is 4 rows, behind the root panel's own single
    // child-marker row.
    frame(&mut h, build(3));
    let three = span_of(&h, "canvas");
    assert_eq!((three.start, three.len), (1, 4));
    assert_eq!(arena_len(&h), 5, "one row plus four, and no slack");

    // 4 shapes is 5 rows — a different length is a different class, so a
    // second block, and the 4-row one is parked.
    frame(&mut h, build(4));
    let four = span_of(&h, "canvas");
    assert_eq!((four.start, four.len), (5, 5));
    let settled = arena_len(&h);
    assert_eq!(settled, 10);
    assert_eq!(free_classes(&h), 1, "the 4-row block is parked");

    let before = h.ui.damage_engine.paints.counters.counts();
    const FRAMES: u32 = 200;
    for f in 0..FRAMES {
        frame(&mut h, build(3 + f % 2));
        assert_eq!(
            arena_len(&h),
            settled,
            "frame {f} extended the arena — the toggle stopped recycling",
        );
        assert_eq!(
            span_of(&h, "canvas"),
            if f % 2 == 0 { three } else { four },
            "frame {f} must land back on the block its own class parked",
        );
        assert_eq!(free_classes(&h), 1, "exactly one block sits idle");
    }

    let delta = h.ui.damage_engine.paints.counters.counts() - before;
    assert_eq!(
        (delta.allocs, delta.reuses),
        (0, FRAMES),
        "every frame took a recycled block and none extended the arena",
    );
}

/// Blocks here are exactly their span's length: the arena holds the
/// live row count and not one slot more.
///
/// That is [`Paint`]'s granule of one, and it is a measured choice
/// rather than a tidiness one — the diff reads these spans back every
/// frame, so the slack a coarser granule leaves between them costs cache
/// density. Rounding to four inflated this arena by 20-30% and cost
/// 2.9% and 6.8% of `damage/workload/shape_churn_partial` and
/// `shape_churn_full`.
///
/// [`Paint`]: crate::scene::cascade::paint::Paint
#[test]
fn a_span_occupies_exactly_its_row_count() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        canvas(ui, "a", 2);
        canvas(ui, "b", 7);
        canvas(ui, "c", 11);
    });

    // The root panel marks its three children, then one block per
    // canvas of exactly `1 + shapes` rows.
    let root = 3;
    let canvases: u32 = [2u32, 7, 11].iter().map(|s| 1 + s).sum();
    assert_eq!(
        arena_len(&h),
        root + canvases as usize,
        "every block is exactly its span, so the arena is the row count",
    );
    for (id, shapes) in [("a", 2u32), ("b", 7), ("c", 11)] {
        assert_eq!(span_of(&h, id).len, 1 + shapes, "{id}");
    }
    assert_eq!(
        free_classes(&h),
        0,
        "nothing was released, so nothing is parked"
    );
}

/// A live span is stable for the snapshot's whole life, whatever a
/// neighbour does. Compaction moved spans (rewriting each owner's
/// `paint_span` as it went), so a node that never changed still had its
/// storage copied and its index rewritten; now it is untouched.
///
/// The stability is not cosmetic — it is what lets reclamation happen at
/// the point a widget leaves, with no pass that has to walk the map and
/// no ordering constraint between the two.
#[test]
fn a_quiet_node_keeps_its_span_while_a_neighbour_churns() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |shapes: u32| {
        move |ui: &mut Ui| {
            canvas(ui, "quiet", 3);
            canvas(ui, "churner", shapes);
        }
    };

    frame(&mut h, build(4));
    frame(&mut h, build(4));
    let quiet_span = span_of(&h, "quiet");
    let quiet_rows: Vec<_> = h.ui.damage_engine.paints.slots[quiet_span.range()].to_vec();
    assert_eq!(quiet_span.len, 4, "chrome plus three shapes");

    // Walk the churner across four size classes, several times over —
    // enough tail growth to have tripped the old 75%-orphan trigger
    // repeatedly.
    for round in 0..40 {
        frame(&mut h, build(4 + round % 16));
        assert_eq!(
            span_of(&h, "quiet"),
            quiet_span,
            "round {round} relocated a span nothing asked to move",
        );
    }
    assert_eq!(
        h.ui.damage_engine.paints.slots[quiet_span.range()],
        quiet_rows[..],
        "and its rows are byte-identical, not merely at the same index",
    );
}

/// A widget leaving hands its block back, and the next arrival of the
/// same size class takes it — so a list swapping one row for another
/// settles at one spare block rather than one per swap.
///
/// That it takes one swap to settle rather than none is worth stating:
/// departures are reclaimed in the removed-widget tail at the *end* of
/// the diff, after the walk has already served this frame's arrivals. So
/// the high-water mark is the live set plus one frame's departures, and
/// never more.
#[test]
fn swapping_one_widget_for_another_settles_at_a_single_spare_block() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |which: &'static str| move |ui: &mut Ui| canvas(ui, which, 5);

    frame(&mut h, build("first"));
    let one_widget = arena_len(&h);
    // The first swap overlaps: "second" is stored before "first" is
    // reclaimed, so this is the frame that buys the spare block.
    frame(&mut h, build("second"));
    let settled = arena_len(&h);
    assert_eq!(
        settled,
        one_widget + 6,
        "the overlap costs one block of the departing widget's 6 rows",
    );

    // Every later swap runs inside it.
    for round in 0..20 {
        let (arriving, departing) = if round % 2 == 0 {
            ("first", "second")
        } else {
            ("second", "first")
        };
        frame(&mut h, build(arriving));
        assert!(
            !h.ui
                .damage_engine
                .prev
                .contains_key(&WidgetId::from_hash(departing)),
            "round {round}: {departing} must be out of the snapshot map",
        );
        assert_eq!(
            arena_len(&h),
            settled,
            "round {round} bought a block instead of reusing the spare",
        );
    }
}

/// A forced-full frame drops the snapshot map wholesale, so the arena
/// drops its free lists with its storage.
///
/// Keeping them would leave every class head pointing into a buffer that
/// no longer holds blocks, and the next store would hand out an index
/// into somebody else's rows.
#[test]
fn a_forced_full_frame_resets_the_arena_without_stale_free_heads() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |shapes: u32| move |ui: &mut Ui| canvas(ui, "canvas", shapes);

    // Straddle the class boundary so a block is genuinely parked when
    // the reset lands.
    frame(&mut h, build(3));
    frame(&mut h, build(4));
    assert_eq!(free_classes(&h), 1, "the fixture must leave a block parked");

    // A frame whose prior output was never presented forces a full
    // repaint, which invalidates the whole snapshot map — and it swaps
    // in a different tree, so nothing here could be reached by re-keying.
    h.frame_without_baseline(|ui| one_frame(ui, RED));
    assert_eq!(free_classes(&h), 0, "the free lists went with the storage");
    assert_eq!(
        arena_len(&h),
        h.ui.damage_engine.prev.len(),
        "every snapshot in the rebuilt map holds one single-row block and \
         nothing else is allocated",
    );

    // And the rebuilt snapshot addresses its own rows, not a stale
    // block's.
    let span = span_of(&h, "a");
    assert_eq!(span.len, 1, "the 50x50 frame contributes its chrome row");
    assert_eq!(
        h.ui.damage_engine.paints.slots[span.range()][0].screen,
        Rect::new(0.0, 0.0, 50.0, 50.0),
    );
}
