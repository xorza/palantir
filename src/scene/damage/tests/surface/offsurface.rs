//! Rects that lie partly or wholly outside the surface.

use crate::Ui;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::widget_id::WidgetId;
use crate::primitives::{color::RgbaF32, rect::Rect, translate_scale::TranslateScale};
use crate::scene::cascade::paint::Paint;
use crate::scene::cascade::paint::PaintRows;
use crate::scene::damage::Damage;
use crate::scene::damage::region::DamageRegion;
use crate::scene::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{frame::Frame, panel::Panel};
use glam::Vec2;

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
        oversized.clamp_to(surface),
        Rect::new(90.0, 90.0, 10.0, 10.0),
        "sanity: 1000×1000 rect at (90,90) intersects surface in a 10×10 corner",
    );
    let collapsed = DamageRegion::collapse_from(&[oversized], f32::INFINITY, surface);
    // Region stores the clipped rect, not the raw input.
    let stored: Vec<_> = collapsed.region.iter_rects().collect();
    assert_eq!(
        stored,
        vec![Rect::new(90.0, 90.0, 10.0, 10.0)],
        "collapse_from must store the surface-clipped rect, not the raw input",
    );
    let damage = Damage::new(collapsed);
    assert!(
        matches!(damage, Some(Damage::Partial(_))),
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
    let collapsed =
        DamageRegion::collapse_from(&[covers_all_plus_overflow], f32::INFINITY, surface);
    let damage = Damage::new(collapsed);
    assert_eq!(
        damage,
        Some(Damage::Full),
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
    let collapsed = DamageRegion::collapse_from(&[off_screen], f32::INFINITY, surface);
    assert!(
        collapsed.region.is_empty(),
        "wholly-off-surface rect must produce an empty region (no skip-vs-Partial drift)",
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
    let straddling = [
        Paint {
            screen: Rect::new(-20.0, 0.0, 10.0, 10.0),
            ..Default::default()
        },
        Paint {
            screen: Rect::new(110.0, 0.0, 10.0, 10.0),
            ..Default::default()
        },
    ];
    assert!(
        !straddling.any_on_surface(Rect::new(0.0, 0.0, 100.0, 100.0)),
        "the union can cross the surface even though no paint row does",
    );

    let mut h = UiHarness::new(DISPLAY.physical);
    frame(&mut h, |ui| {
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
                    .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                    .background(Background {
                        fill: BLUE.into(),
                        ..Default::default()
                    })
                    .show(ui, |_| {});
            });
    });

    assert!(
        !h.engines
            .damage
            .prev
            .contains_key(&WidgetId::from_hash("off")),
        "Vacant + off-surface paint_rect must not seed a prev entry — \
         hashmap insert + raw_rects push are both wasted work for a \
         node that contributes nothing visible",
    );
    assert!(
        h.damage_region().is_empty(),
        "no visible widgets means no damage rects on the second-frame \
         diff (first frame is Full and walks differently)",
    );
}

// DamageEngine rects must be in *screen space*. When an ancestor has a
// transform, the rendered position of a node differs from its layout
// rect; the damage rect, the prev_frame snapshot, and the encoder/
// backend scissor all need to track that screen-space position.

/// Soundness pin for the tier's entry-less leg: a node skipped by the
/// Vacant-arm off-surface filter (no `prev` snapshot) that scrolls
/// *into* view under tier 1.5 is covered by the curr-extent push and
/// gets its snapshot inserted in the same pass, a following still
/// frame is a clean Skip (tier 1 at the subtree root), a second move
/// clears its previous position (the inserted snapshot feeds the
/// prev-extent fold), a content change on it lands its rect, and
/// removing it while visible clears its pixels (the eviction tail
/// finds the snapshot). The last two legs regress without the tier-1.5
/// insert: the second move smears (old pixels stay) and the removal
/// computes no damage outright.
#[test]
fn offscreen_node_scrolling_into_view_is_covered_and_stays_sound() {
    let mut h = UiHarness::new(DISPLAY.physical);
    // Surface is 200×200 (test DISPLAY). Three 100-wide frames: "c"
    // starts at x = 200 — exactly off-surface (edge-touching rects
    // don't intersect), so its Vacant visit skips the snapshot insert.
    let build = |dx: f32, c_fill: Option<RgbaF32>, ui: &mut Ui| {
        Panel::hstack()
            .id(WidgetId::from_hash("outer"))
            .transform(TranslateScale::from_translation(Vec2::new(dx, 0.0)))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("inner"))
                    .show(ui, |ui| {
                        let cells = [("a", Some(BLUE)), ("b", Some(BLUE)), ("c", c_fill)];
                        for (key, fill) in cells {
                            let Some(fill) = fill else { continue };
                            Frame::new()
                                .id(WidgetId::from_hash(key))
                                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                                .background(Background {
                                    fill: fill.into(),
                                    ..Default::default()
                                })
                                .show(ui);
                        }
                    });
            });
    };
    frame(&mut h, |ui| build(0.0, Some(RED), ui));

    // Scroll left: "c" enters at (100..200). Tier 1.5 fires at
    // "inner"; "c" had no snapshot (off-surface skip last frame) — the
    // curr-extent push covers its pixels and the insert leg snapshots
    // it now that it's visible.
    let damage = frame(&mut h, |ui| build(-100.0, Some(RED), ui));
    let region = Damage::expect_partial(damage);
    let covers_c = region
        .iter_rects()
        .any(|r| r.min.x <= 100.5 && r.max().x >= 200.0 - 0.5 && r.max().y >= 40.0 - 0.5);
    assert!(
        covers_c,
        "curr-extent push must cover the newly revealed node. region = {:?}",
        region.iter_rects().collect::<Vec<_>>(),
    );

    // Still frame: nothing changed — tier 1 skips at the root.
    let damage = frame(&mut h, |ui| build(-100.0, Some(RED), ui));
    assert_eq!(damage, None, "still frame after the move");

    // Second move: "c" shifts to (0..100). Its just-inserted snapshot
    // joins the prev-extent fold, so its old pixels at (100..200)
    // repaint alongside the new position.
    let damage = frame(&mut h, |ui| build(-200.0, Some(RED), ui));
    let region = Damage::expect_partial(damage);
    for (label, probe) in [
        ("old", Rect::new(150.0, 0.0, 10.0, 40.0)),
        ("new", Rect::new(50.0, 0.0, 10.0, 40.0)),
    ] {
        assert!(
            region.any_intersects(probe),
            "second move must damage c's {label} position; region = {:?}",
            region.iter_rects().collect::<Vec<_>>(),
        );
    }

    // Content change on "c" (now snapshotted, at 0..100): the walk
    // descends and the changed-paints arm damages its rect.
    let damage = frame(&mut h, |ui| build(-200.0, Some(BLUE), ui));
    let region = Damage::expect_partial(damage);
    let rects: Vec<Rect> = region.iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 100.0, 40.0)],
        "content change on the revealed node damages its rect",
    );

    // Remove "c" while visible: the eviction tail finds the inserted
    // snapshot and clears its pixels.
    let damage = frame(&mut h, |ui| build(-200.0, None, ui));
    let covers_removed = match damage {
        Some(Damage::Full) => true,
        Some(Damage::Partial(damage)) => damage
            .region
            .any_intersects(Rect::new(50.0, 0.0, 10.0, 40.0)),
        None => false,
    };
    assert!(
        covers_removed,
        "removing the revealed node must damage its pixels; got {damage:?}",
    );
}
