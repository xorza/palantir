//! Scroll fixtures: pin the scrollbar visuals (reservation layout,
//! bar positioning, corner avoidance) and the encoder-cache replay
//! correctness that bit us with the `exit_idx` panic.

use glam::UVec2;
use palantir::{
    Background, Color, Configure, Corners, Frame, Panel, Scroll, ScrollbarTheme, Sizing,
};

use crate::diff::Tolerance;
use crate::fixtures::DARK_BG;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

const CARD: Color = Color::rgb(0.16, 0.20, 0.28);
const ROW: Color = Color::rgb(0.42, 0.55, 0.78);

/// Override the default near-black thumb to a light translucent fill
/// so it shows up against the dark fixture background. Mirrors what
/// `examples/showcase/main.rs` does.
fn light_thumb_theme(ui: &mut palantir::Ui) {
    ui.theme.scrollbar = ScrollbarTheme {
        thumb: Color::rgba(1.0, 1.0, 1.0, 0.55),
        thumb_hover: Color::rgba(1.0, 1.0, 1.0, 0.75),
        thumb_active: Color::rgba(1.0, 1.0, 1.0, 0.9),
        ..Default::default()
    };
}

/// Tall content in a fixed-height vertical scroll. Two-frame settle:
/// frame 1 records with empty state (no overflow detected, no bar);
/// frame 2 reads the populated state and reserves padding + emits the
/// bar. The golden captures frame 2.
#[test]
fn scroll_vertical_overflow_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack().auto_id().padding(8.0).show(ui, |ui| {
            Scroll::vertical()
                .id_salt("scroll")
                .gap(3.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..30u32 {
                        Frame::new()
                            .id_salt(("row", i))
                            .background(Background {
                                fill: ROW,
                                radius: Corners::all(3.0),
                                ..Default::default()
                            })
                            .size((Sizing::FILL, Sizing::Fixed(20.0)))
                            .show(ui);
                    }
                });
        });
    }
    let size = UVec2::new(180, 200);
    let _ = h.render(size, 1.0, DARK_BG, scene);
    let img = h.render(size, 1.0, DARK_BG, scene);
    assert_matches_golden("scroll_vertical_overflow", &img, Tolerance::default());
}

/// Wide content in a fixed-width horizontal scroll. Bar lands at the
/// bottom edge after two-frame settle.
#[test]
fn scroll_horizontal_overflow_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack().auto_id().padding(8.0).show(ui, |ui| {
            Scroll::horizontal()
                .id_salt("scroll")
                .gap(3.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..30u32 {
                        Frame::new()
                            .id_salt(("col", i))
                            .background(Background {
                                fill: ROW,
                                radius: Corners::all(3.0),
                                ..Default::default()
                            })
                            .size((Sizing::Fixed(40.0), Sizing::FILL))
                            .show(ui);
                    }
                });
        });
    }
    let size = UVec2::new(220, 80);
    let _ = h.render(size, 1.0, DARK_BG, scene);
    let img = h.render(size, 1.0, DARK_BG, scene);
    assert_matches_golden("scroll_horizontal_overflow", &img, Tolerance::default());
}

/// Both-axis scroll over a content larger than the viewport on both
/// axes. Pins: V bar at right edge, H bar at bottom edge, empty
/// corner where they would have met.
#[test]
fn scroll_xy_overflow_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack().auto_id().padding(8.0).show(ui, |ui| {
            Scroll::both()
                .id_salt("scroll")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("big")
                        .background(Background {
                            fill: ROW,
                            radius: Corners::all(6.0),
                            ..Default::default()
                        })
                        .size((Sizing::Fixed(400.0), Sizing::Fixed(400.0)))
                        .show(ui);
                });
        });
    }
    let size = UVec2::new(160, 160);
    let _ = h.render(size, 1.0, DARK_BG, scene);
    let img = h.render(size, 1.0, DARK_BG, scene);
    assert_matches_golden("scroll_xy_overflow", &img, Tolerance::default());
}

/// Content fits inside the viewport — no overflow, no bar, no
/// reservation. Even after two settle frames the bar must stay
/// collapsed.
#[test]
fn scroll_no_bar_when_content_fits_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack().auto_id().padding(8.0).show(ui, |ui| {
            Scroll::vertical()
                .id_salt("scroll")
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Frame::new()
                        .id_salt("short")
                        .background(Background {
                            fill: ROW,
                            radius: Corners::all(3.0),
                            ..Default::default()
                        })
                        .size((Sizing::FILL, Sizing::Fixed(40.0)))
                        .show(ui);
                });
        });
    }
    let size = UVec2::new(160, 160);
    let _ = h.render(size, 1.0, DARK_BG, scene);
    let img = h.render(size, 1.0, DARK_BG, scene);
    assert_matches_golden("scroll_no_bar_when_fits", &img, Tolerance::default());
}

/// Scroll with user-set padding. The bar must land in the reserved
/// strip flush with the OUTER right edge — NOT inside the user's
/// padding band. Catches the regression where bar position used the
/// inner viewport (= would land inside user padding) instead of outer.
#[test]
fn scroll_with_user_padding_matches_golden() {
    let mut h = Harness::new();
    fn scene(ui: &mut palantir::Ui) {
        light_thumb_theme(ui);
        Panel::vstack().auto_id().padding(8.0).show(ui, |ui| {
            Scroll::vertical()
                .id_salt("scroll")
                .padding(16.0)
                .gap(3.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..20u32 {
                        Frame::new()
                            .id_salt(("row", i))
                            .background(Background {
                                fill: ROW,
                                radius: Corners::all(3.0),
                                ..Default::default()
                            })
                            .size((Sizing::FILL, Sizing::Fixed(20.0)))
                            .show(ui);
                    }
                });
        });
    }
    let size = UVec2::new(180, 180);
    let _ = h.render(size, 1.0, DARK_BG, scene);
    let img = h.render(size, 1.0, DARK_BG, scene);
    assert_matches_golden("scroll_with_user_padding", &img, Tolerance::default());
}

/// Warm-cache parity: render the same scene three times. Frame 1 has
/// cold caches + empty `ScrollState` (no bar). Frame 2 has populated
/// state (bar appears) but cold encoder cache for the bar shapes.
/// Frame 3 reads bar shapes through the warm encoder cache.
///
/// The encoder cache `exit_idx` bug we just fixed manifested as a
/// composer panic on frame 3 of nested clipped scrolls — but the
/// general latent risk is that warm-cache replay diverges in pixels
/// from cold-cache encode. Pin frame 3 byte-identical to frame 2.
/// No golden — pure intra-test invariant.
#[test]
fn scroll_warm_cache_matches_cold_encoded_second_frame() {
    let mut h = Harness::new();
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
                            fill: CARD,
                            radius: Corners::all(6.0),
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
                                        Frame::new()
                                            .id_salt((tag, "row", i))
                                            .background(Background {
                                                fill: ROW,
                                                radius: Corners::all(3.0),
                                                ..Default::default()
                                            })
                                            .size((Sizing::FILL, Sizing::Fixed(18.0)))
                                            .show(ui);
                                    }
                                });
                        });
                }
            });
    }
    let size = UVec2::new(280, 200);
    let _ = h.render(size, 1.0, DARK_BG, scene);
    let frame_2 = h.render(size, 1.0, DARK_BG, scene);
    let frame_3 = h.render(size, 1.0, DARK_BG, scene);
    // Strict byte-equality: same scene, deterministic encode → identical pixels.
    // If the encoder cache or compose cache corrupts replay, this diverges.
    assert_eq!(
        frame_2.dimensions(),
        frame_3.dimensions(),
        "frame dimensions must match"
    );
    let mut diffs = 0usize;
    for (p2, p3) in frame_2.pixels().zip(frame_3.pixels()) {
        if p2 != p3 {
            diffs += 1;
        }
    }
    assert_eq!(
        diffs, 0,
        "warm-cache frame diverged from cold-encoded second frame in {diffs} pixels"
    );
}
