//! Showcase regressions where two cells from a Grid (or two
//! back-to-back grids inside a vstack) ended up painting on top of
//! each other. Pinned via arranged-rect order plus a render-pass
//! check on emitted `DrawText` x positions.
use crate::Ui;
use crate::primitives::widget_id::WidgetId;
use crate::text::wrap::TextWrap;

use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::background::Background;
use crate::primitives::shadow::Shadow;
use crate::primitives::{color::RgbaF32, corners::Corners, stroke::Stroke};
use crate::renderer::frontend::capture::PaintCall;
use crate::scene::layer::Layer;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::{grid::Grid, panel::Panel, text::Text};
use glam::UVec2;

const PARAGRAPH: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump!";

fn section(ui: &mut Ui, id: &'static str, body: &mut dyn FnMut(&mut Ui)) {
    Panel::vstack()
        .id(WidgetId::from_hash(id))
        .size((Sizing::FILL, Sizing::HUG))
        .gap(6.0)
        .padding(8.0)
        .background(Background {
            fill: RgbaF32::srgb(0.16, 0.18, 0.22).into(),
            border: Stroke::new(RgbaF32::srgb(0.30, 0.34, 0.42), 1.0),
            corners: Corners::all(4.0),
            shadow: Shadow::NONE,
        })
        .show(ui, |ui| {
            Text::new("title")
                .id(WidgetId::from_hash(("section-title", id)))
                .font_size(12.0)
                .show(ui);
            body(ui);
        });
}

/// Showcase regressions: two cells in a Grid with a wrapping text column
/// must not paint on top of each other. Pinned across two topologies:
/// a default-sized Grid with two Hug cols, and a FILL-sized Grid with
/// Hug + Fill cols (the property-grid pattern).
#[test]
fn grid_columns_with_wrapping_text_do_not_overlap() {
    type Case = (&'static str, Option<Sizing>, [Track; 2], (f32, f32));
    let cases: &[Case] = &[
        (
            "two_hug_columns",
            None,
            [Track::HUG, Track::HUG],
            (0.0, 0.0),
        ),
        (
            "hug_label_fill_value",
            Some(Sizing::FILL),
            [Track::HUG, Track::FILL],
            (6.0, 16.0),
        ),
    ];
    let long_text = "The quick brown fox jumps over the lazy dog. Pack my box \
                     with five dozen liquor jugs. How vexingly quick daft zebras jump!";
    for (label_id, grid_main, cols, gaps) in cases {
        let mut h = UiHarness::new(UVec2::new(800, 600));
        let mut left = None;
        let mut right = None;
        h.frame(|ui| {
            Panel::vstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    let mut g = Grid::new().auto_id();
                    if let Some(s) = *grid_main {
                        g = g.size((s, Sizing::HUG));
                    }
                    g.cols(*cols)
                        .rows([Track::HUG])
                        .line_gap(gaps.0)
                        .gap(gaps.1)
                        .show(ui, |ui| {
                            left = Some(
                                Text::new(long_text)
                                    .auto_id()
                                    .font_size(14.0)
                                    .text_wrap(TextWrap::WrapWithOverflow)
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            right = Some(
                                Text::new("right column")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node(),
                            );
                        });
                });
        });

        // Mono at 14 px is 7 px a char: "right column" is 12 × 7 = 84.
        let layout = h.ui.layout(Layer::Main);
        let lr = layout.rect[left.unwrap().idx()];
        let rr = layout.rect[right.unwrap().idx()];
        assert_eq!(rr.size.w, 84.0, "case: {label_id}");
        assert_eq!(
            rr.min.x,
            lr.max().x + gaps.1,
            "case: {label_id}: the right cell starts one gap past the left",
        );
        if *label_id == "two_hug_columns" {
            // The wrapping column takes what the right one leaves.
            assert_eq!(lr.size.w, 800.0 - 84.0, "case: {label_id}");
        }
    }
}

#[test]
fn text_layouts_two_sections_back_to_back_no_overlap() {
    let mut h = UiHarness::new(UVec2::new(1500, 900));

    let mut hug_left = None;
    let mut hug_right = None;
    let mut prop_label = None;
    let mut prop_value = None;

    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .gap(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                section(ui, "two-hug-columns", &mut |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("two-hug-inner"))
                        .cols([Track::HUG, Track::HUG])
                        .rows([Track::HUG])
                        .line_gap(0.0)
                        .gap(16.0)
                        .show(ui, |ui| {
                            hug_left = Some(
                                Text::new(PARAGRAPH)
                                    .auto_id()
                                    .font_size(14.0)
                                    .text_wrap(TextWrap::WrapWithOverflow)
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            hug_right = Some(
                                Text::new("right column")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 1))
                                    .show(ui)
                                    .node(),
                            );
                        });
                });

                section(ui, "property-grid", &mut |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("property-grid-inner"))
                        .size((Sizing::FILL, Sizing::HUG))
                        .cols([Track::HUG, Track::FILL])
                        .rows([Track::HUG, Track::HUG, Track::HUG])
                        .line_gap(6.0)
                        .gap(16.0)
                        .show(ui, |ui| {
                            prop_label = Some(
                                Text::new("Title:")
                                    .auto_id()
                                    .font_size(14.0)
                                    .grid_cell((0, 0))
                                    .show(ui)
                                    .node(),
                            );
                            prop_value = Some(
                                Text::new(
                                    "Lorem Ipsum is simply dummy text of the printing industry.",
                                )
                                .auto_id()
                                .font_size(14.0)
                                .text_wrap(TextWrap::WrapWithOverflow)
                                .grid_cell((0, 1))
                                .show(ui)
                                .node(),
                            );
                        });
                });
            });
    });

    let layout = h.ui.layout(Layer::Main);
    let l1 = layout.rect[hug_left.unwrap().idx()];
    let r1 = layout.rect[hug_right.unwrap().idx()];
    let l2 = layout.rect[prop_label.unwrap().idx()];
    let r2 = layout.rect[prop_value.unwrap().idx()];

    // Mono at 14 px is 7 px a char, inside a section inset 8 + 1 px by
    // its padding and border, so 1500 − 18 = 1482 wide. Two Hug columns:
    // the 122-char paragraph fits on one line, 854 px, then the 16 px gap.
    // The property grid: "Title:" is 42, and the Fill value column takes
    // the rest, 1482 − 42 − 16 = 1424.
    assert_eq!([l1.min.x, l1.size.w], [9.0, 854.0], "two-hug-columns: left");
    assert_eq!(
        [r1.min.x, r1.size.w],
        [879.0, 84.0],
        "two-hug-columns: right"
    );
    assert_eq!([l2.min.x, l2.size.w], [9.0, 42.0], "property-grid: label");
    assert_eq!(
        [r2.min.x, r2.size.w],
        [67.0, 1424.0],
        "property-grid: value"
    );
}

/// Render-pass repro: build the property-grid pattern and inspect
/// the emitted `DrawText` commands directly.
#[test]
fn property_grid_emits_distinct_drawtext_x_positions() {
    let mut h = UiHarness::with_text(UVec2::new(1500, 900));
    h.frame(|ui| {
        Panel::vstack()
            .auto_id()
            .gap(16.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Grid::new()
                    .id(WidgetId::from_hash("property-grid-inner"))
                    .size((Sizing::FILL, Sizing::HUG))
                    .cols([Track::HUG, Track::FILL])
                    .rows([Track::HUG, Track::HUG, Track::HUG])
                    .line_gap(6.0)
                    .gap(16.0)
                    .show(ui, |ui| {
                        Text::new("Title:")
                            .auto_id()
                            .font_size(14.0)
                            .grid_cell((0, 0))
                            .show(ui);
                        Text::new("Lorem Ipsum is simply dummy text of the printing industry.")
                            .auto_id()
                            .font_size(14.0)
                            .text_wrap(TextWrap::WrapWithOverflow)
                            .grid_cell((0, 1))
                            .show(ui);
                        Text::new("Description:")
                            .auto_id()
                            .font_size(14.0)
                            .grid_cell((1, 0))
                            .show(ui);
                    });
            });
    });

    let cmds = h.encode_paint();
    let mut text_xs: Vec<f32> = Vec::new();
    for command in cmds.calls.iter() {
        if let PaintCall::Text(payload) = command {
            text_xs.push(payload.rect.min.x);
        }
    }
    assert!(
        text_xs.len() >= 2,
        "expected at least two DrawText cmds; got {text_xs:?}",
    );
    assert!(
        text_xs[0] != text_xs[1],
        "Title and Lorem texts must paint at different x; got {text_xs:?}",
    );
}

/// Diagnostic: full showcase repro. Catches the screenshot bug where
/// two distinct texts emit `DrawText` at the same (x, y).
#[test]
fn text_layouts_full_showcase_drawtext_dump() {
    let mut h = UiHarness::new(UVec2::new(1620, 980));
    h.frame(|ui| {
        Panel::vstack().auto_id()
        .padding(12.0)
        .gap(12.0)
        .size((Sizing::FILL, Sizing::FILL))
        .show(ui, |ui| {
            Panel::hstack().auto_id()
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |_| {});
            Panel::zstack().auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .padding(16.0)
                .show(ui, |ui| {
                    Panel::vstack().auto_id()
                        .gap(16.0)
                        .size((Sizing::FILL, Sizing::FILL))
                        .show(ui, |ui| {
                            section(ui, "two-hug-columns", &mut |ui| {
                                Grid::new().id(WidgetId::from_hash("two-hug-inner"))
                                    .cols([Track::HUG, Track::HUG])
                                    .rows([Track::HUG])
                                    .line_gap(0.0).gap(16.0)
                                    .show(ui, |ui| {
                                        Text::new(PARAGRAPH).auto_id()
                                            .font_size(14.0)
                                            .text_wrap(TextWrap::WrapWithOverflow)
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new("right column").auto_id()
                                            .font_size(14.0)
                                            .grid_cell((0, 1))
                                            .show(ui);
                                    });
                            });
                            section(ui, "property-grid", &mut |ui| {
                                Grid::new().id(WidgetId::from_hash("property-grid-inner"))
                                    .size((Sizing::FILL, Sizing::HUG))
                                    .cols([Track::HUG, Track::FILL])
                                    .rows([Track::HUG, Track::HUG, Track::HUG])
                                    .line_gap(6.0).gap(16.0)
                                    .show(ui, |ui| {
                                        Text::new("Title:").auto_id()
                                            .font_size(14.0)
                                            .grid_cell((0, 0))
                                            .show(ui);
                                        Text::new(
                                            "Lorem Ipsum is simply dummy text of the printing industry.",
                                        ).auto_id()
                                        .font_size(14.0)
                                        .text_wrap(TextWrap::WrapWithOverflow)
                                        .grid_cell((0, 1))
                                        .show(ui);
                                        Text::new("Description:").auto_id()
                                            .font_size(14.0)
                                            .grid_cell((1, 0))
                                            .show(ui);
                                        Text::new(PARAGRAPH).auto_id()
                                            .font_size(14.0)
                                            .text_wrap(TextWrap::WrapWithOverflow)
                                            .grid_cell((1, 1))
                                            .show(ui);
                                        Text::new("Tags:").auto_id()
                                            .font_size(14.0)
                                            .grid_cell((2, 0))
                                            .show(ui);
                                        Text::new("layout, grid, intrinsic, wrapping, css").auto_id()
                                            .font_size(14.0)
                                            .text_wrap(TextWrap::WrapWithOverflow)
                                            .grid_cell((2, 1))
                                            .show(ui);
                                    });
                            });
                        });
                });
        });
    });

    let cmds = h.encode_paint();
    let mut entries: Vec<(f32, f32, u64)> = Vec::new();
    for command in cmds.calls.iter() {
        if let PaintCall::Text(payload) = command {
            entries.push((
                payload.rect.min.x,
                payload.rect.min.y,
                payload.text.key.text_hash.get(),
            ));
        }
    }
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            let (xi, yi, hi) = entries[i];
            let (xj, yj, hj) = entries[j];
            if hi != hj && (xi - xj).abs() < 0.5 && (yi - yj).abs() < 0.5 {
                panic!(
                    "two distinct texts at same (x,y): #{i} hash={hi:#x} vs #{j} hash={hj:#x} at ({xi}, {yi})",
                );
            }
        }
    }
}
