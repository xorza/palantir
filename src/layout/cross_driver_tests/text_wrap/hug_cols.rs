//! Non-wrapping labels in hug columns, which must never shrink below their
//! full width.

use crate::TextStyle;
use crate::Ui;
use crate::WidgetId;
use crate::layout::types::sizing::Sizing;
use crate::layout::types::track::Track;
use crate::layout::{axis::Axis, intrinsic::LenReq};
use crate::scene::layer::Layer;
use crate::scene::node::configure::Configure;
use crate::scene::tree::node_id::NodeId;
use crate::text::wrap::TextWrap;
use crate::ui::harness::UiHarness;
use crate::widgets::{grid::Grid, panel::Panel, text::Text};
use glam::UVec2;

/// Repro for the showcase "text layouts" first section: a Hug+Hug grid
/// holding a wrapping paragraph in col 0 and a *non-wrapping* label in
/// col 1, nested under FILL panels that inherit a finite surface width.
/// As the surface narrows below the grid's natural intrinsic floor, the
/// grid must clamp at that floor (col 0 = paragraph longest-word-or-line,
/// col 1 = full label width). It must NOT keep shrinking col 1 below its
/// label's natural width — non-wrapping text cannot be broken.
#[test]
fn two_hug_cols_nonwrapping_label_floors_at_full_width() {
    fn build(ui: &mut Ui) -> (NodeId, NodeId) {
        let mut grid_node = None;
        let mut section_node = None;
        Panel::vstack().auto_id()
            .padding(12.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                Panel::zstack().auto_id()
                    .padding(16.0)
                    .size((Sizing::FILL, Sizing::FILL))
                    .show(ui, |ui| {
                        Panel::vstack().auto_id()
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                section_node = Some(Panel::vstack().auto_id()
                                    .size((Sizing::FILL, Sizing::HUG))
                                    .gap(6.0)
                                    .show(ui, |ui| {
                                        Text::new(
                                            "two Hug columns: paragraph wraps to fit, \
                                             label stays natural",
                                        )
                                        .id(WidgetId::from_hash("section-title"))
                                        .style(&TextStyle::default().with_font_size(12.0))
                                        .text_wrap(TextWrap::SingleLine)
                                        .show(ui);
                                        grid_node = Some(
                                            Grid::new()
                                                .id(WidgetId::from_hash("grid"))
                                                .cols([Track::HUG, Track::HUG])
                                                .rows([Track::HUG])
                                                .show(ui, |ui| {
                                                    Text::new(
                                                        "the quick brown fox jumps over the lazy dog",
                                                    ).auto_id()
                                                    .style(&TextStyle::default().with_font_size(14.0))
                                                    .text_wrap(TextWrap::WrapWithOverflow)
                                                    .grid_cell((0, 0))
                                                    .show(ui);
                                                    Text::new("right column").auto_id()
                                                        .style(
                                                            &TextStyle::default()
                                                                .with_font_size(14.0),
                                                        )
                                                        .text_wrap(TextWrap::SingleLine)
                                                        .grid_cell((0, 1))
                                                        .show(ui);
                                                })
                                                .response.node(),
                                        );
                                    }).response.node());
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

    // Once the section panel stops shrinking (its intrinsic_min is
    // pinned wider than the grid's alone — by the section title text
    // here), the Hug grid inside must NOT keep shrinking. It should
    // fill the section's committed cross extent, not the smaller
    // surface-derived `available` the measure pass received before
    // flooring.
    let widths: [u32; 5] = [400, 300, 250, 200, 150];
    let mut section_widths = Vec::new();
    let mut grid_widths = Vec::new();
    for w in widths {
        let (g, s) = measure_at(w);
        section_widths.push(s);
        grid_widths.push(g);
    }
    // Find a pair of surface widths where the section width didn't
    // change (panel stopped shrinking). Grid width must also be stable
    // there.
    for i in 1..section_widths.len() {
        if (section_widths[i] - section_widths[i - 1]).abs() < 0.5 {
            let g_prev = grid_widths[i - 1];
            let g_curr = grid_widths[i];
            assert!(
                (g_curr - g_prev).abs() <= 0.5,
                "section panel stopped shrinking at {} but grid kept shrinking: \
                 surfaces {} → {}, grid {} → {}",
                section_widths[i],
                widths[i - 1],
                widths[i],
                g_prev,
                g_curr,
            );
            return;
        }
    }
    panic!(
        "test setup did not produce a regime where section panel stops shrinking; \
         widths={widths:?} section_widths={section_widths:?}"
    );
}

/// Pin: a non-wrapping `Text` reports MinContent on the X axis equal to
/// its full unbroken width, not the longest-word width. Wrapping text
/// reports the longest-word width, since it can break between words.
/// A Hug+Hug grid containing a wrapping paragraph and a non-wrapping
/// label must give the label its full natural width as a column floor —
/// otherwise the layout solver's slack distribution shrinks the label
/// column below the label's true width, and the label paint overflows
/// its arranged cell.
#[test]
fn nonwrapping_text_minconent_equals_full_width() {
    let mut h = UiHarness::with_text(UVec2::new(400, 200));
    let label_node = h.frame_value(|ui| {
        Text::new("right column")
            .auto_id()
            .style(&TextStyle::default().with_font_size(14.0))
            .text_wrap(TextWrap::SingleLine)
            .show(ui)
            .node()
    });
    let store = h.ui.record_store();
    let interned_text = store.interned_text();
    let max_w = h.engines.layout.intrinsic(
        h.ui.tree(Layer::Main),
        label_node,
        Axis::X,
        LenReq::MaxContent,
        &interned_text,
    );
    let min_w = h.engines.layout.intrinsic(
        h.ui.tree(Layer::Main),
        label_node,
        Axis::X,
        LenReq::MinContent,
        &interned_text,
    );
    assert!(
        (min_w - max_w).abs() <= 0.5,
        "non-wrapping Text MinContent must equal MaxContent (full width); \
       max_w={max_w} min_w={min_w}",
    );
}

/// Pin issue 1a: in a `Hug+Hug` grid, when the surface is too narrow
/// to fit both columns at their natural max-content widths, the slack
/// distribution must allocate enough to the non-wrapping label column
/// for the label's full text to fit. The wrapping paragraph absorbs
/// the squeeze; the label cell rect width stays >= the label's natural
/// width.
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
                    Text::new("the quick brown fox jumps over the lazy dog")
                        .auto_id()
                        .style(&TextStyle::default().with_font_size(14.0))
                        .text_wrap(TextWrap::WrapWithOverflow)
                        .grid_cell((0, 0))
                        .show(ui)
                        .node(),
                );
                label_node = Some(
                    Text::new("right column")
                        .auto_id()
                        .style(&TextStyle::default().with_font_size(14.0))
                        .text_wrap(TextWrap::SingleLine)
                        .grid_cell((0, 1))
                        .show(ui)
                        .node(),
                );
            });
        (paragraph_node.unwrap(), label_node.unwrap())
    }

    // Probe label's natural unbroken width at an unconstrained surface.
    let mut probe = UiHarness::with_text(UVec2::new(2000, 400));
    let probe_label = probe.frame_value(|ui| build(ui).1);
    let store = probe.ui.record_store();
    let interned_text = store.interned_text();
    let label_full = probe.engines.layout.intrinsic(
        probe.ui.tree(Layer::Main),
        probe_label,
        Axis::X,
        LenReq::MaxContent,
        &interned_text,
    );
    assert!(label_full > 0.0);

    // At a surface narrower than the paragraph max-content but wider
    // than the grid's intrinsic floor, slack distribution kicks in.
    // The label cell must still get at least its full natural width.
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

/// Regression for the showcase "two Hug columns" grid: a **bare** label
/// (no `.text_wrap(...)`, so it takes the `Text` default) in a Hug+Hug grid
/// next to a wrapping paragraph must keep its full natural width — the
/// paragraph wraps to absorb the squeeze. This pins the default: `Text`
/// defaults to `TextWrap::Overflow`, whose MinContent equals its full line,
/// so the grid's Hug solver floors the label column at the label width and
/// never shrinks it (a default reporting MinContent 0 would let the slack
/// split clip "right column" → "right col").
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
                  .style(&TextStyle::default().with_font_size(14.0))
                  .text_wrap(TextWrap::WrapWithOverflow)
                  .grid_cell((0, 0))
                  .show(ui);
              // No `.text_wrap(...)` — exercises the default.
              Text::new("right column")
                  .auto_id()
                  .style(&TextStyle::default().with_font_size(14.0))
                  .grid_cell((0, 1))
                  .show(ui)
                  .node()
          })
          .inner
    }

    // Label's natural unbroken width, probed unconstrained.
    let mut probe = UiHarness::with_text(UVec2::new(2000, 400));
    let probe_label = probe.frame_value(build);
    let store = probe.ui.record_store();
    let interned_text = store.interned_text();
    let label_full = probe.engines.layout.intrinsic(
        probe.ui.tree(Layer::Main),
        probe_label,
        Axis::X,
        LenReq::MaxContent,
        &interned_text,
    );
    assert!(label_full > 0.0);

    // The long paragraph's max-content dwarfs these surfaces, so the grid
    // is in the slack-distribution regime (paragraph wraps). The default
    // label must still occupy its full width at each.
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
