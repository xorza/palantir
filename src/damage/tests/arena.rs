//! Paint-snapshot storage under churn: blocks recycle in place, live spans
//! never move, and no frame pays for another's shape churn.
//!
//! Row counts are `1 + shapes` (chrome at row 0, then one row per shape).
//! [`Paint::GRANULE`] is one, so a block is exactly its span's length and each
//! row count is its own size class.
//!
//! [`Paint::GRANULE`]: crate::common::block_arena::BlockSlot::GRANULE

use crate::Ui;
use crate::common::counters::CounterSet;
use crate::common::span::Span;
use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame, one_frame};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::shape::Shape;
use crate::widget_core::configure::Configure;
use crate::widgets::panel::Panel;

/// A canvas holding `shapes` rects, all inside its box so none is culled.
fn canvas(ui: &mut Ui, id: &'static str, shapes: u32) {
    Panel::hstack()
        .id(WidgetId::from_hash(id))
        .size((Sizing::fixed(180.0), Sizing::fixed(90.0)))
        .background(Background::fill(BLUE))
        .show(ui, |ui| {
            for s in 0..shapes {
                ui.add_shape(
                    Shape::rect(Rect::new(
                        (s % 9) as f32 * 20.0,
                        (s / 9) as f32 * 20.0,
                        8.0,
                        8.0,
                    ))
                    .fill(RgbaF32::srgb(0.1 * s as f32, 0.4, 0.6)),
                );
            }
        });
}

fn arena_len(h: &UiHarness) -> usize {
    h.engines.damage.paints.slots.len()
}

fn free_classes(h: &UiHarness) -> usize {
    h.engines.damage.paints.classes_with_free_blocks()
}

fn span_of(h: &UiHarness, id: &'static str) -> Span {
    h.engines.damage.prev[&WidgetId::from_hash(id)].paint_span
}

/// A node whose paint-row count changes every frame reaches a steady state
/// where storage stops growing: once both size classes were seen, the same two
/// blocks trade back and forth.
#[test]
fn a_toggling_shape_count_trades_two_blocks_forever() {
    const FRAMES: u32 = 200;

    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |shapes: u32| move |ui: &mut Ui| canvas(ui, "canvas", shapes);

    // 3 shapes is 4 rows, behind the root panel's child-marker row.
    frame(&mut h, build(3));
    let three = span_of(&h, "canvas");
    assert_eq!((three.start, three.len), (1, 4));
    assert_eq!(arena_len(&h), 5, "one row plus four, and no slack");

    // 4 shapes is 5 rows, a different class, so a second block; the 4-row one is parked.
    frame(&mut h, build(4));
    let four = span_of(&h, "canvas");
    assert_eq!((four.start, four.len), (5, 5));
    let settled = arena_len(&h);
    assert_eq!(settled, 10);
    assert_eq!(free_classes(&h), 1, "the 4-row block is parked");

    let before = h.engines.damage.paints.counters.counts();
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

    let delta = h.engines.damage.paints.counters.counts() - before;
    assert_eq!(
        (delta.allocs, delta.reuses),
        (0, FRAMES),
        "every frame took a recycled block and none extended the arena",
    );
}

/// Blocks are exactly their span's length: the arena holds the live row count
/// and no slack. The granule of one is measured: coarser slack costs cache
/// density in the diff's per-frame span reads.
///
/// [`Paint`]: crate::cascade::paint::Paint
#[test]
fn a_span_occupies_exactly_its_row_count() {
    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
        canvas(ui, "a", 2);
        canvas(ui, "b", 7);
        canvas(ui, "c", 11);
    });

    // The root panel marks its three children, then one block per canvas of
    // `1 + shapes` rows.
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

/// A live span is stable for the snapshot's whole life, whatever a neighbour
/// does. That is what lets reclamation happen when a widget leaves, with no
/// walking pass or ordering constraint.
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
    let quiet_rows: Vec<_> = h.engines.damage.paints.slots[quiet_span.range()].to_vec();
    assert_eq!(quiet_span.len, 4, "chrome plus three shapes");

    // Walk the churner across four size classes several times.
    for round in 0..40 {
        frame(&mut h, build(4 + round % 16));
        assert_eq!(
            span_of(&h, "quiet"),
            quiet_span,
            "round {round} relocated a span nothing asked to move",
        );
    }
    assert_eq!(
        h.engines.damage.paints.slots[quiet_span.range()],
        quiet_rows[..],
        "and its rows are byte-identical, not merely at the same index",
    );
}

/// A leaving widget hands its block back and the next arrival of the same
/// class takes it, so a list swapping rows settles at one spare block.
///
/// It takes one swap to settle because departures are reclaimed at the end of
/// the diff, after this frame's arrivals. The high-water mark is the live set
/// plus one frame's departures.
#[test]
fn swapping_one_widget_for_another_settles_at_a_single_spare_block() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |which: &'static str| move |ui: &mut Ui| canvas(ui, which, 5);

    frame(&mut h, build("first"));
    let one_widget = arena_len(&h);
    // The first swap overlaps: "second" is stored before "first" is reclaimed,
    // buying the spare block.
    frame(&mut h, build("second"));
    let settled = arena_len(&h);
    assert_eq!(
        settled,
        one_widget + 6,
        "the overlap costs one block of the departing widget's 6 rows",
    );

    for round in 0..20 {
        let (arriving, departing) = if round % 2 == 0 {
            ("first", "second")
        } else {
            ("second", "first")
        };
        frame(&mut h, build(arriving));
        assert!(
            !h.engines
                .damage
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

/// A forced-full frame drops the snapshot map wholesale, so the arena drops
/// its free lists too; stale class heads would hand out indices into
/// someone else's rows.
#[test]
fn a_forced_full_frame_resets_the_arena_without_stale_free_heads() {
    let mut h = UiHarness::new(DISPLAY.physical);
    let build = |shapes: u32| move |ui: &mut Ui| canvas(ui, "canvas", shapes);

    // Straddle the class boundary so a block is parked when the reset lands.
    frame(&mut h, build(3));
    frame(&mut h, build(4));
    assert_eq!(free_classes(&h), 1, "the fixture must leave a block parked");

    // A frame whose prior output was never presented forces a full repaint,
    // invalidating the whole snapshot map and swapping in a different tree.
    h.frame_without_baseline(|ui| one_frame(ui, RED));
    assert_eq!(free_classes(&h), 0, "the free lists went with the storage");
    assert_eq!(
        arena_len(&h),
        h.engines.damage.prev.len(),
        "every snapshot in the rebuilt map holds one single-row block and \
         nothing else is allocated",
    );

    // The rebuilt snapshot addresses its own rows.
    let span = span_of(&h, "a");
    assert_eq!(span.len, 1, "the 50x50 frame contributes its chrome row");
    assert_eq!(
        h.engines.damage.paints.slots[span.range()][0].screen,
        Rect::new(0.0, 0.0, 50.0, 50.0),
    );
}
