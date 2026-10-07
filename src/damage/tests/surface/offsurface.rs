//! Rects that lie partly or wholly outside the surface.

use crate::Ui;
use crate::cascade::paint::Paint;
use crate::cascade::paint::PaintRows;
use crate::damage::Damage;
use crate::damage::region::DamageRegion;
use crate::damage::tests::support::{BLUE, DISPLAY, RED, frame};
use crate::internals::harness::UiHarness;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::paint::background::Background;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::{block::Block, panel::Panel};
use glam::Vec2;

/// `DamageRegion::collapse_from` clips each rect to the surface, so a huge rect with a tiny visible part doesn't force `Full`.
#[test]
fn partial_when_oversized_rect_lies_mostly_off_surface() {
    let surface = Rect::new(0.0, 0.0, 100.0, 100.0);
    let oversized = Rect::new(90.0, 90.0, 1000.0, 1000.0);
    assert_eq!(
        oversized.clamp_to(surface),
        Rect::new(90.0, 90.0, 10.0, 10.0),
        "sanity: 1000×1000 rect at (90,90) intersects surface in a 10×10 corner",
    );
    let collapsed = DamageRegion::collapse_from(&[oversized], f32::INFINITY, surface);
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

/// A rect fully covering the surface still trips Full, however far it overflows.
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

/// A rect entirely off the surface is dropped from the region.
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

/// A first-seen node entirely off the surface skips the `prev` insert, as `collapse_from` would drop its rect.
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
        // `Panel::transform` applies to the body, so the chrome lands wholly off a 200×200 surface.
        Panel::canvas()
            .id(WidgetId::from_hash("outer"))
            .size((Sizing::FILL, Sizing::FILL))
            .transform(TranslateScale::from_translation(Vec2::new(500.0, 500.0)))
            .show(ui, |ui| {
                Panel::hstack()
                    .id(WidgetId::from_hash("off"))
                    .size((Sizing::fixed(50.0), Sizing::fixed(50.0)))
                    .background(Background::fill(BLUE))
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

// Damage rects are in screen space.

/// A node skipped by the first-seen off-surface filter that scrolls into view
/// is covered by the curr-extent push and snapshotted in the same pass. A still
/// frame then Skips, a move clears its old position, a content change lands its
/// rect, and removal clears its pixels. The last two regress without the insert.
#[test]
fn offscreen_node_scrolling_into_view_is_covered_and_stays_sound() {
    let mut h = UiHarness::new(DISPLAY.physical);
    // "c" starts at x = 200, exactly off-surface (edge-touching rects don't intersect).
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
                            Block::new()
                                .id(WidgetId::from_hash(key))
                                .size((Sizing::fixed(100.0), Sizing::fixed(40.0)))
                                .background(Background::fill(fill))
                                .show(ui);
                        }
                    });
            });
    };
    frame(&mut h, |ui| build(0.0, Some(RED), ui));

    // Scroll left: "c" enters at 100..200; the push covers it and the insert snapshots it.
    let damage = frame(&mut h, |ui| build(-100.0, Some(RED), ui));
    let region = Damage::expect_partial(damage);
    let covers_c = region
        .iter_rects()
        .any(|r| r.min.x <= 100.5 && r.max().x >= 200.0 - 0.5 && r.max().y >= 40.0 - 0.5);
    assert!(
        covers_c,
        "curr-extent push must cover the newly revealed node. region = {region:?}",
    );

    let damage = frame(&mut h, |ui| build(-100.0, Some(RED), ui));
    assert_eq!(damage, None, "still frame after the move");

    // Second move: "c" at 0..100; its snapshot joins the prev-extent fold, repainting 100..200.
    let damage = frame(&mut h, |ui| build(-200.0, Some(RED), ui));
    let region = Damage::expect_partial(damage);
    for (label, probe) in [
        ("old", Rect::new(150.0, 0.0, 10.0, 40.0)),
        ("new", Rect::new(50.0, 0.0, 10.0, 40.0)),
    ] {
        assert!(
            region.any_intersects(probe),
            "second move must damage c's {label} position; region = {region:?}",
        );
    }

    let damage = frame(&mut h, |ui| build(-200.0, Some(BLUE), ui));
    let region = Damage::expect_partial(damage);
    let rects: Vec<Rect> = region.iter_rects().collect();
    assert_eq!(
        rects,
        vec![Rect::new(0.0, 0.0, 100.0, 40.0)],
        "content change on the revealed node damages its rect",
    );

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
