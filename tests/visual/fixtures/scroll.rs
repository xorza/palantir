//! Scroll fixtures: scrollbar visuals and encoder-cache replay correctness.

use glam::UVec2;
use palantir::{
    Background, Block, Configure, Corners, FramePaint, Panel, RgbaF32, Scroll, ScrollbarTheme,
    Sizing,
};

use crate::golden_name::GoldenName;
use crate::goldens::{assert_matches_golden, assert_same};
use crate::harness::Harness;

const CARD: RgbaF32 = RgbaF32::srgb(0.16, 0.20, 0.28);
const ROW: RgbaF32 = RgbaF32::srgb(0.42, 0.55, 0.78);

/// Light translucent thumb so it shows on the dark fixture background.
fn light_thumb_theme(ui: &mut palantir::Ui) {
    ui.theme_mut().scrollbar = ScrollbarTheme {
        thumb: RgbaF32::srgba(1.0, 1.0, 1.0, 0.55),
        thumb_hovered: RgbaF32::srgba(1.0, 1.0, 1.0, 0.75),
        thumb_active: RgbaF32::srgba(1.0, 1.0, 1.0, 0.9),
        ..Default::default()
    };
}

/// Tall content in a fixed-height vertical scroll; the golden captures frame 2, once the bar exists.
#[test]
fn scroll_vertical_overflow_matches_golden() {
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .gap(3.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        for i in 0..30u32 {
                            Block::new()
                                .id_salt(("row", i))
                                .background(Background {
                                    fill: ROW.into(),
                                    corners: Corners::all(3.0),
                                    ..Default::default()
                                })
                                .size((Sizing::FILL, Sizing::fixed(20.0)))
                                .show(ui);
                        }
                    });
            });
    }

    let mut h = Harness::new();
    let size = UVec2::new(180, 200);
    let img = h.size(size).settled_frame(1, scene).image;
    assert_matches_golden(GoldenName::ScrollVerticalOverflow, &img);
}

/// Wide content in a fixed-width horizontal scroll; the bar lands at the bottom edge after settle.
#[test]
fn scroll_horizontal_overflow_matches_golden() {
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Scroll::horizontal()
                    .id_salt("scroll")
                    .gap(3.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        for i in 0..30u32 {
                            Block::new()
                                .id_salt(("col", i))
                                .background(Background {
                                    fill: ROW.into(),
                                    corners: Corners::all(3.0),
                                    ..Default::default()
                                })
                                .size((Sizing::fixed(40.0), Sizing::FILL))
                                .show(ui);
                        }
                    });
            });
    }

    let mut h = Harness::new();
    let size = UVec2::new(220, 80);
    let img = h.size(size).settled_frame(1, scene).image;
    assert_matches_golden(GoldenName::ScrollHorizontalOverflow, &img);
}

/// Both-axis scroll: V bar right, H bar bottom, empty corner where they meet.
#[test]
fn scroll_xy_overflow_matches_golden() {
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Scroll::both()
                    .id_salt("scroll")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Block::new()
                            .id_salt("big")
                            .background(Background {
                                fill: ROW.into(),
                                corners: Corners::all(6.0),
                                ..Default::default()
                            })
                            .size((Sizing::fixed(400.0), Sizing::fixed(400.0)))
                            .show(ui);
                    });
            });
    }

    let mut h = Harness::new();
    let size = UVec2::new(160, 160);
    let img = h.size(size).settled_frame(1, scene).image;
    assert_matches_golden(GoldenName::ScrollXyOverflow, &img);
}

/// Content that fits has no overflow, bar or reservation; the bar stays collapsed after settling.
#[test]
fn scroll_no_bar_when_content_fits_matches_golden() {
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Block::new()
                            .id_salt("short")
                            .background(Background {
                                fill: ROW.into(),
                                corners: Corners::all(3.0),
                                ..Default::default()
                            })
                            .size((Sizing::FILL, Sizing::fixed(40.0)))
                            .show(ui);
                    });
            });
    }

    let mut h = Harness::new();
    let size = UVec2::new(160, 160);
    let img = h.size(size).settled_frame(1, scene).image;
    assert_matches_golden(GoldenName::ScrollNoBarWhenFits, &img);
}

/// With user padding the bar sits flush with the OUTER right edge, not inside the padding band.
#[test]
fn scroll_with_user_padding_matches_golden() {
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Scroll::vertical()
                    .id_salt("scroll")
                    .padding(16.0)
                    .gap(3.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        for i in 0..20u32 {
                            Block::new()
                                .id_salt(("row", i))
                                .background(Background {
                                    fill: ROW.into(),
                                    corners: Corners::all(3.0),
                                    ..Default::default()
                                })
                                .size((Sizing::FILL, Sizing::fixed(20.0)))
                                .show(ui);
                        }
                    });
            });
    }

    let mut h = Harness::new();
    let size = UVec2::new(180, 180);
    let img = h.size(size).settled_frame(1, scene).image;
    assert_matches_golden(GoldenName::ScrollWithUserPadding, &img);
}

/// Warm-cache parity: a cold encode and a full repaint with warm caches must yield identical pixels.
#[test]
fn scroll_warm_cache_repaint_matches_the_cold_encode() {
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::hstack()
            .auto_id()
            .padding(8.0)
            .gap(8.0)
            .show(ui, |ui| {
                for tag in ["a", "b"] {
                    Panel::vstack()
                        .id_salt(("card", tag))
                        .padding(6.0)
                        .background(Background {
                            fill: CARD.into(),
                            corners: Corners::all(6.0),
                            ..Default::default()
                        })
                        .clip_rect()
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            Scroll::vertical()
                                .id_salt(("scroll", tag))
                                .gap(3.0)
                                .size((Sizing::FILL, Sizing::FILL))
                                .show(ui, |ui| {
                                    for i in 0..25u32 {
                                        Block::new()
                                            .id_salt((tag, "row", i))
                                            .background(Background {
                                                fill: ROW.into(),
                                                corners: Corners::all(3.0),
                                                ..Default::default()
                                            })
                                            .size((Sizing::FILL, Sizing::fixed(18.0)))
                                            .show(ui);
                                    }
                                });
                        });
                }
            });
    }

    let mut h = Harness::new();
    // Both frames are pinned to `FramePaint::Full`: an unchanged scene would skip and compare a backbuffer to itself.
    let cold = h.size(UVec2::new(280, 200)).frame(scene);
    assert_eq!(cold.paint, FramePaint::Full);
    h.host.invalidate_target_contents();
    let warm = h.frame(scene);
    assert_eq!(warm.paint, FramePaint::Full, "the warm frame repaints");
    assert_same("scroll_warm_cache_repaint", &warm.image, &cold.image);
}
