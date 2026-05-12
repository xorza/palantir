//! Text rendering fixtures.

use glam::UVec2;
use image::Rgba;
use palantir::{Background, Color, Configure, Panel, Sizing, Text, TextStyle};

use crate::diff::Tolerance;
use crate::fixtures::DARK_BG;
use crate::golden::assert_matches_golden;
use crate::harness::Harness;

/// Multi-line paragraph with mixed sizes/colors. Slightly looser
/// tolerance — glyph AA varies more across drivers than rect-only
/// scenes.
#[test]
fn text_paragraph_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(360, 140), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .auto_id()
            .padding(16.0)
            .gap(6.0)
            .show(ui, |ui| {
                Text::new("Palantir")
                    .id_salt("title")
                    .style(
                        TextStyle::default()
                            .with_font_size(20.0)
                            .with_color(Color::rgb(0.92, 0.94, 1.00)),
                    )
                    .show(ui);
                Text::new("Immediate-mode UI with WPF-style layout.")
                    .id_salt("body")
                    .style(
                        TextStyle::default()
                            .with_font_size(13.0)
                            .with_color(Color::rgb(0.72, 0.76, 0.84)),
                    )
                    .show(ui);
                Text::new("Rendered headlessly through wgpu.")
                    .id_salt("body2")
                    .style(
                        TextStyle::default()
                            .with_font_size(13.0)
                            .with_color(Color::rgb(0.72, 0.76, 0.84)),
                    )
                    .show(ui);
            });
    });
    let tol = Tolerance {
        per_channel: 4,
        max_ratio: 0.005,
    };
    assert_matches_golden("text_paragraph", &img, tol);
}

/// Row list with many labels under per-row backgrounds. Exercises
/// text-batch coalescing across distinct scissors (each row's
/// background creates a group) — the composer fuses all rows' text
/// into one glyphon `prepare`/`render`. Visual pin: every row's
/// label must read on its row's background, no glyphs missing.
#[test]
fn text_row_list_batches_into_one_render() {
    let mut h = Harness::new();
    let rows = [
        ("Alpha", Color::rgb(0.20, 0.40, 0.60)),
        ("Bravo", Color::rgb(0.60, 0.30, 0.20)),
        ("Charlie", Color::rgb(0.25, 0.55, 0.30)),
        ("Delta", Color::rgb(0.55, 0.40, 0.65)),
        ("Echo", Color::rgb(0.35, 0.55, 0.55)),
        ("Foxtrot", Color::rgb(0.65, 0.55, 0.30)),
    ];
    let img = h.render(UVec2::new(220, 200), 1.0, DARK_BG, |ui| {
        Panel::vstack()
            .auto_id()
            .padding(8.0)
            .gap(4.0)
            .show(ui, |ui| {
                for (label, bg) in rows {
                    Panel::hstack()
                        .id_salt(label)
                        .padding(6.0)
                        .background(Background {
                            fill: bg.into(),
                            ..Default::default()
                        })
                        .size((Sizing::Fixed(200.0), Sizing::Hug))
                        .show(ui, |ui| {
                            Text::new(label)
                                .auto_id()
                                .style(
                                    TextStyle::default()
                                        .with_font_size(14.0)
                                        .with_color(Color::rgb(0.95, 0.95, 1.00)),
                                )
                                .show(ui);
                        });
                }
            });
    });
    let tol = Tolerance {
        per_channel: 4,
        max_ratio: 0.005,
    };
    assert_matches_golden("text_row_list_batched", &img, tol);
}

/// Smoke pin: a row list under partial damage doesn't visibly
/// regress. Frame 1 draws six labeled rows; frame 2 flips one
/// label's text. The unchanged rows must still show glyph ink in
/// the captured frame.
///
/// Limit: this fixture does NOT catch the cross-batch damage-drop
/// bug directly — partial-damage `LoadOp::Load` preserves frame 1's
/// pixels in non-damaged regions, so even if the schedule dropped
/// the unchanged rows' batch the swapchain would still show their
/// frame-1 glyphs. The schedule drain is pinned at the unit-test
/// level (`text_batch_anchored_in_damage_skipped_group_still_emits`).
/// What this fixture pins instead: end-to-end "list rendering
/// doesn't break under partial damage" — a regression sentinel.
#[test]
fn text_row_list_survives_partial_damage_smoke() {
    let mut h = Harness::new();
    let size = UVec2::new(220, 200);
    // Frame 1: 6 rows. All labels live in one text batch (rows are
    // disjoint, no overlap-induced split).
    let row_bg = Color::rgb(0.20, 0.20, 0.24);
    let labels_initial = ["aaaa", "bbbb", "cccc", "dddd", "eeee", "ffff"];

    let scene = |labels: [&'static str; 6]| {
        move |ui: &mut palantir::Ui| {
            Panel::vstack()
                .auto_id()
                .padding(8.0)
                .gap(4.0)
                .show(ui, |ui| {
                    for (i, label) in labels.iter().enumerate() {
                        Panel::hstack()
                            .id_salt(i)
                            .padding(6.0)
                            .background(Background {
                                fill: row_bg.into(),
                                ..Default::default()
                            })
                            .size((Sizing::Fixed(200.0), Sizing::Hug))
                            .show(ui, |ui| {
                                Text::new(*label)
                                    .id_salt(("row-label", i))
                                    .style(
                                        TextStyle::default()
                                            .with_font_size(14.0)
                                            .with_color(Color::rgb(0.95, 0.95, 1.00)),
                                    )
                                    .show(ui);
                            });
                    }
                });
        }
    };

    let f1 = h.render(size, 1.0, DARK_BG, scene(labels_initial));

    // Flip row 2's text only ("cccc" → "CCCC"). Damage covers just
    // that row. Other rows' labels must still render.
    let labels_changed = ["aaaa", "bbbb", "CCCC", "dddd", "eeee", "ffff"];
    let f2 = h.render(size, 1.0, DARK_BG, scene(labels_changed));

    // Glyph-ink heuristic: a row's label region should contain at
    // least a few near-white pixels (the glyph fill) over the dark
    // row background. If the batch were dropped, the region would
    // hold only background pixels.
    let is_glyph_ink = |Rgba([r, g, b, _]): Rgba<u8>| r > 200 && g > 200 && b > 200;
    let count_ink_in_band = |img: &image::RgbaImage, y_lo: u32, y_hi: u32| {
        let mut n = 0u32;
        for y in y_lo..y_hi {
            for x in 14..80 {
                if is_glyph_ink(*img.get_pixel(x, y)) {
                    n += 1;
                }
            }
        }
        n
    };

    // Per-row approximate Y bands (8px top pad + ~26px per row).
    // Row 0: y ≈ 8..34; Row 1: 34..60; Row 3: 86..112; Row 4: 112..138;
    // Row 5: 138..164. Skip row 2 (changed) — its content is replaced.
    let unchanged_bands = [(8, 34), (34, 60), (86, 112), (112, 138), (138, 164)];
    for (lo, hi) in unchanged_bands {
        let f1_ink = count_ink_in_band(&f1, lo, hi);
        let f2_ink = count_ink_in_band(&f2, lo, hi);
        assert!(
            f1_ink > 0,
            "frame 1 label in band y={lo}..{hi} should have ink (got {f1_ink})",
        );
        assert!(
            f2_ink > 0,
            "frame 2 unchanged label in band y={lo}..{hi} must still render \
             (got {f2_ink}) — batch dropped under partial damage?",
        );
    }
}
