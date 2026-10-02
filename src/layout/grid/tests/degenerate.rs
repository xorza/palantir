//! Zero extents, empty dimensions, and a track list long enough to test the
//! inline cap.

use crate::internals::harness::UiHarness;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::size::Size;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::{block::Block, grid::Grid, panel::Panel};
use glam::UVec2;

/// Pin: empty grid (zero rows or zero cols) measures + arranges to zero
/// without panicking; child rects are zeroed at parent anchor. The other
/// dimension's one 50 px track does not survive: a grid with no cells in
/// one direction has none at all.
#[test]
fn grid_empty_dim_measures_to_zero_and_zeros_children() {
    let empty: &[Track] = &[];
    let one: &[Track] = &[Track::fixed(50.0)];
    for (label, cols, rows) in [("no rows", one, empty), ("no cols", empty, one)] {
        let mut h = UiHarness::new(UVec2::new(400, 400));
        h.frame(|ui| {
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    Grid::new()
                        .id(WidgetId::from_hash("empty-grid"))
                        .cols(cols)
                        .rows(rows)
                        .size((Sizing::HUG, Sizing::HUG))
                        .show(ui, |ui| {
                            Block::new()
                                .id(WidgetId::from_hash("ghost"))
                                .size((20.0, 20.0))
                                .show(ui);
                        });
                });
        });
        let grid = h.arranged(WidgetId::from_hash("empty-grid"));
        assert_eq!(grid.size, Size::ZERO, "{label}: the grid");
        let ghost = h.arranged(WidgetId::from_hash("ghost"));
        assert_eq!(ghost.size, Size::ZERO, "{label}: its child");
    }
}

/// Pin: a grid whose own slot resolves to zero extent still gives its
/// Fixed track the declared size, and its Fill track the nothing that
/// remains.
///
/// `0.0` is a legitimate `resolve_axis` total, so arrange must not read
/// it as "measure never ran for this grid". This is the frame where that
/// distinction bites: measure records a zero total, arrange is handed the
/// same zero, and the track sizes it reuses have to be the ones the
/// solver produced.
#[test]
fn zero_extent_grid_keeps_fixed_track_when_arrange_reuses_the_resolution() {
    fn build(ui: &mut Ui) {
        Grid::new()
            .id(WidgetId::from_hash("zero-grid"))
            .cols([Track::fixed(30.0), Track::FILL])
            .rows([Track::fixed(20.0)])
            .size((Sizing::fixed(0.0), Sizing::fixed(0.0)))
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("fixed-cell"))
                    .grid_cell((0, 0))
                    .show(ui);
                Block::new()
                    .id(WidgetId::from_hash("fill-cell"))
                    .grid_cell((0, 1))
                    .show(ui);
            });
    }

    let mut h = UiHarness::new(UVec2::new(400, 400));
    let mut frames = Vec::new();
    for _ in 0..2 {
        h.frame(build);
        let grid = h.arranged(WidgetId::from_hash("zero-grid"));
        let fixed = h.arranged(WidgetId::from_hash("fixed-cell"));
        let fill = h.arranged(WidgetId::from_hash("fill-cell"));

        assert_eq!((grid.size.w, grid.size.h), (0.0, 0.0));
        // Phase 1 commits Fixed tracks before any leftover is shared out,
        // so the zero total leaves the 30×20 cell whole and the cell
        // overflows its parent — the contains-content rule, same as
        // anywhere else.
        assert_eq!((fixed.size.w, fixed.size.h), (30.0, 20.0));
        assert_eq!(fixed.min, grid.min);
        // Nothing left after the Fixed column, but the row still stands.
        assert_eq!((fill.size.w, fill.size.h), (0.0, 20.0));
        assert_eq!(fill.min.x, grid.min.x + 30.0);
        frames.push((fixed, fill));
    }
    assert_eq!(
        frames[0], frames[1],
        "a second frame over the same tree must arrange identically",
    );
}

#[test]
fn large_inline_track_definition_has_exact_extent_and_last_cell_position() {
    const COLS: usize = 64;
    let cols: [Track; COLS] = std::array::from_fn(|i| Track::fixed((i + 1) as f32));
    let mut h = UiHarness::new(UVec2::new(3_000, 100));
    h.frame(|ui| {
        Grid::new()
            .id(WidgetId::from_hash("large-grid"))
            .rows([Track::fixed(10.0)])
            .cols(cols)
            .line_gap(0.0)
            .gap(2.0)
            .show(ui, |ui| {
                Block::new()
                    .id(WidgetId::from_hash("last-cell"))
                    .grid_cell((0, (COLS - 1) as u16))
                    .show(ui);
            });
    });

    // Sum 1..=64 = 2,080; 63 gaps × 2 = 126.
    let grid = h.arranged(WidgetId::from_hash("large-grid"));
    assert_eq!(grid.size, Size::new(2_206.0, 10.0));

    // Sum 1..=63 = 2,016; 63 preceding gaps × 2 = 126.
    let last = h.arranged(WidgetId::from_hash("last-cell"));
    assert_eq!(last.min, glam::Vec2::new(2_142.0, 0.0));
    assert_eq!(last.size, Size::new(64.0, 10.0));
}
