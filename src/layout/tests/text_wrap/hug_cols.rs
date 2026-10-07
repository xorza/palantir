//! Non-wrapping labels in hug columns, which must never shrink below their
//! full width.

use crate::TextStyle;
use crate::Ui;
use crate::WidgetId;
use crate::internals::harness::UiHarness;
use crate::layout::intrinsic::len_req::LenReq;
use crate::layout::tests::support::PARAGRAPH;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::layout::track::Track;
use crate::scene::layer::Layer;
use crate::scene::tree::node_id::NodeId;
use crate::text::wrap::TextWrap;
use crate::widget_core::configure::Configure;
use crate::widgets::{grid::Grid, panel::Panel, text::Text};
use glam::UVec2;

/// Repro for the showcase "text layouts" section: a Hug+Hug grid with a wrapping paragraph in col 0 and a non-wrapping label in col 1, under FILL panels. As the surface narrows, the grid must clamp at its intrinsic floor, never shrinking col 1 below the label's natural width.
#[test]
fn two_hug_cols_nonwrapping_label_floors_at_full_width() {
    fn build(ui: &mut Ui) -> (NodeId, NodeId) {
        let mut grid_node = None;
        let mut section_node = None;
        Panel::vstack()
            .auto_id()
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack()
                    .auto_id()
                    .padding(16.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::vstack()
                            .auto_id()
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                section_node = Some(
                                    Panel::vstack()
                                        .auto_id()
                                        .size((Sizing::FILL, Sizing::HUG))
                                        .gap(6.0)
                                        .show(ui, |ui| {
                                            Text::new(
                                                "two Hug columns: paragraph wraps to fit, \
                                             label stays natural",
                                            )
                                            .id(WidgetId::from_hash("section-title"))
                                            .font_size(12.0)
                                            .text_wrap(TextWrap::SingleLine)
                                            .show(ui);
                                            grid_node = Some(
                                                Grid::new()
                                                    .id(WidgetId::from_hash("grid"))
                                                    .cols([Track::HUG, Track::HUG])
                                                    .rows([Track::HUG])
                                                    .show(ui, |ui| {
                                                        Text::new(PARAGRAPH)
                                                            .auto_id()
                                                            .font_size(14.0)
                                                            .text_wrap(TextWrap::WrapWithOverflow)
                                                            .grid_cell((0, 0))
                                                            .show(ui);
                                                        Text::new("right column")
                                                            .auto_id()
                                                            .style(
                                                                &TextStyle::default()
                                                                    .with_font_size(14.0),
                                                            )
                                                            .text_wrap(TextWrap::SingleLine)
                                                            .grid_cell((0, 1))
                                                            .show(ui);
                                                    })
                                                    .response
                                                    .node(),
                                            );
                                        })
                                        .response
                                        .node(),
                                );
                            });
                    });
            });
        (grid_node.unwrap(), section_node.unwrap())
    }

    fn measure_at(surface_w: u32) -> (f32, f32) {
        let mut h = UiHarness::with_text(UVec2::new(surface_w, 400));
        let nodes = h.frame_value(build);
        let (grid, section) = nodes;
        let grid_w = h.ui.arranged_rect(Layer::Main, grid).size.w;
        let section_w = h.ui.arranged_rect(Layer::Main, section).size.w;
        (grid_w, section_w)
    }

    // Once the section panel stops shrinking (floored by its title text), the Hug grid inside must fill the section's committed cross extent, not the smaller surface-derived `available`.
    let widths: [u32; 5] = [400, 300, 250, 200, 150];
    let (mut section_widths, mut grid_widths) = (Vec::new(), Vec::new());
    for w in widths {
        let (g, s) = measure_at(w);
        section_widths.push(s);
        grid_widths.push(g);
    }
    // At 400 the section fills 400 − 2·12 − 2·16 = 344; below that it floors at its 341 px single-line title (12 px Inter).
    assert_eq!(section_widths, [344.0, 341.0, 341.0, 341.0, 341.0]);
    assert_eq!(grid_widths, section_widths);
}

/// Pin: a non-wrapping `Text` reports MinContent on X equal to its full width; wrapping text reports the longest word. A Hug+Hug grid must floor the label column there, or slack distribution shrinks it and the paint overflows its cell.
#[test]
fn nonwrapping_text_minconent_equals_full_width() {
    let mut h = UiHarness::with_text(UVec2::new(400, 200));
    let label_node = h.frame_value(|ui| {
        Text::new("right column")
            .auto_id()
            .font_size(14.0)
            .text_wrap(TextWrap::SingleLine)
            .show(ui)
            .node()
    });
    let max_w = h.intrinsic(label_node, Axis::X, LenReq::MaxContent);
    let min_w = h.intrinsic(label_node, Axis::X, LenReq::MinContent);
    assert_eq!(
        min_w, max_w,
        "non-wrapping Text MinContent must equal MaxContent (full width)",
    );
}

/// Pin: in a Hug+Hug grid too narrow for both max-content widths, the wrapping paragraph absorbs the squeeze and the label cell stays at least its natural width.
#[test]
fn two_hug_cols_label_cell_never_shrinks_below_label_full_width() {
    fn build(ui: &mut Ui) -> (NodeId, NodeId) {
        let mut paragraph_node = None;
        let mut label_node = None;
        Grid::new()
            .id(WidgetId::from_hash("grid"))
            .cols([Track::HUG, Track::HUG])
            .rows([Track::HUG])
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                paragraph_node = Some(
                    Text::new(PARAGRAPH)
                        .auto_id()
                        .font_size(14.0)
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .grid_cell((0, 0))
                        .show(ui)
                        .node(),
                );
                label_node = Some(
                    Text::new("right column")
                        .auto_id()
                        .font_size(14.0)
                        .text_wrap(TextWrap::SingleLine)
                        .grid_cell((0, 1))
                        .show(ui)
                        .node(),
                );
            });
        (paragraph_node.unwrap(), label_node.unwrap())
    }

    let mut probe = UiHarness::with_text(UVec2::new(2000, 400));
    let probe_label = probe.frame_value(|ui| build(ui).1);
    let label_full = probe.intrinsic(probe_label, Axis::X, LenReq::MaxContent);
    assert!(label_full > 0.0);

    // Between the grid's floor and the paragraph's max-content, slack distribution applies; the label cell still gets its full natural width.
    for surface_w in [400u32, 300, 250, 200] {
        let mut h = UiHarness::with_text(UVec2::new(surface_w, 400));
        let label = h.frame_value(|ui| build(ui).1);
        let label_rect_w = h.ui.arranged_rect(Layer::Main, label).size.w;
        assert!(
            label_rect_w >= label_full - 0.5,
            "label cell shrank below the label's natural width — \
         non-wrapping text would visually overflow its column. \
         surface_w={surface_w} label_full={label_full} label_rect_w={label_rect_w}",
        );
    }
}

/// Regression: a bare label (default `TextWrap::Overflow`, MinContent equals its full line) in a Hug+Hug grid beside a wrapping paragraph keeps its full width; a default MinContent 0 would clip "right column" → "right col".
#[test]
fn two_hug_cols_default_label_hugs_full_width() {
    fn build(ui: &mut Ui) -> NodeId {
        Grid::new()
          .id(WidgetId::from_hash("grid"))
          .cols([Track::HUG, Track::HUG])
          .rows([Track::HUG])
          .size((Sizing::FILL, Sizing::HUG))
          .show(ui, |ui| {
              Text::new("the quick brown fox jumps over the lazy dog. pack my box with five dozen liquor jugs")
                  .auto_id()
                  .font_size(14.0)
                  .text_wrap(TextWrap::WrapWithOverflow)
                  .grid_cell((0, 0))
                  .show(ui);
              Text::new("right column")
                  .auto_id()
                  .font_size(14.0)
                  .grid_cell((0, 1))
                  .show(ui)
                  .node()
          })
          .inner
    }

    let mut probe = UiHarness::with_text(UVec2::new(2000, 400));
    let probe_label = probe.frame_value(build);
    let label_full = probe.intrinsic(probe_label, Axis::X, LenReq::MaxContent);
    assert!(label_full > 0.0);

    // The paragraph's max-content dwarfs these surfaces (slack regime); the default label must still occupy its full width.
    for surface_w in [600u32, 500, 400, 300] {
        let mut h = UiHarness::with_text(UVec2::new(surface_w, 400));
        let label = h.frame_value(build);
        let label_rect_w = h.ui.arranged_rect(Layer::Main, label).size.w;
        assert!(
            label_rect_w >= label_full - 0.5,
            "default-wrap label shrank below its natural width — it would clip. \
           surface_w={surface_w} label_full={label_full} label_rect_w={label_rect_w}",
        );
    }
}
