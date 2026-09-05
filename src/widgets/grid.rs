//! The WPF-style grid: explicit row and column tracks, with each child
//! placed into a cell it names.

use crate::layout::types::layout_mode::LayoutMode;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::ui::Ui;
use crate::widgets::response::InnerResponse;
use crate::widgets::widget::Widget;
use std::rc::Rc;

/// WPF-style grid: explicit row + column track definitions, per-track
/// `Pixel`/`Auto`/`Star` sizing with optional `[min, max]` clamps, and
/// children placed by `(row, col)` with optional `(row_span, col_span)`.
///
/// Track sizing maps 1:1 to `Sizing`: `Fixed` = Pixel, `Hug` = Auto,
/// `Fill(weight)` = Star. Star tracks split the leftover after Fixed and Hug
/// tracks resolve, weighted, with bounded constraint resolution if any
/// `Track::min` / `Track::max` clamps fire.
///
/// Arrays remain inline in the builder and borrowed slices remain borrowed.
/// On `show`, tracks are copied into the current Tree's capacity-retained
/// arena, so natural array declarations are allocation-free after warmup.
///
/// The layout driver documents the three-phase solver and its explicit
/// non-goals: no Auto-vs-Star cycle, `SharedSizeScope`, or auto-flow.
#[derive(Debug)]
pub struct Grid<Rows = [Track; 0], Cols = [Track; 0]> {
    widget: Widget,
    rows: Rows,
    cols: Cols,
    chrome: Option<Background>,
}

impl Grid {
    #[track_caller]
    pub fn new() -> Self {
        Self {
            widget: Widget::grid(),
            rows: [],
            cols: [],
            chrome: None,
        }
    }
}

impl<Rows, Cols> Grid<Rows, Cols> {
    pub fn rows<NewRows: AsRef<[Track]>>(self, rows: NewRows) -> Grid<NewRows, Cols> {
        Grid {
            widget: self.widget,
            rows,
            cols: self.cols,
            chrome: self.chrome,
        }
    }

    pub fn cols<NewCols: AsRef<[Track]>>(self, cols: NewCols) -> Grid<Rows, NewCols> {
        Grid {
            widget: self.widget,
            rows: self.rows,
            cols,
            chrome: self.chrome,
        }
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None` is
    /// the default; theme fallback in [`Self::show`] fills it in from
    /// `ui.theme().panel_background` when unset. Pass [`Background::NONE`]
    /// to suppress that fallback for this grid.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R>
    where
        Rows: AsRef<[Track]>,
        Cols: AsRef<[Track]>,
    {
        let id = ui.push_grid_def(self.rows.as_ref(), self.cols.as_ref());
        let mut widget = self.widget;
        widget.node.set_mode(LayoutMode::Grid(id));

        // Theme fallback for chrome / clip — see `Panel::show`, including
        // why the handle is what gets cloned.
        let theme = Rc::clone(ui.theme());
        let chrome = widget
            .node
            .resolve_container_chrome(self.chrome.as_ref(), theme.container_chrome());
        widget.show(ui, chrome, body)
    }
}

impl_configure!(<Rows, Cols> Grid<Rows, Cols>);

#[cfg(test)]
mod tests {
    use super::Grid;
    use crate::layout::types::limits::MAX_PACKED_GAP;
    use crate::widgets::configure::Configure;

    /// A grid's spacing is the node column every other container uses,
    /// so it is set through the same two setters and faces the same
    /// packed-gap range.
    #[test]
    fn gaps_validate_and_store_values() {
        let configured = Grid::new().line_gap(3.0).gap(5.0);
        assert_eq!(configured.widget.node.gaps.line_gap(), Some(3.0));
        assert_eq!(configured.widget.node.gaps.gap(), Some(5.0));

        let edge = Grid::new().line_gap(MAX_PACKED_GAP).gap(0.0);
        assert_eq!(edge.widget.node.gaps.line_gap(), Some(MAX_PACKED_GAP));
        assert_eq!(edge.widget.node.gaps.gap(), Some(0.0));

        let invalid: [fn(Grid) -> Grid; 6] = [
            |grid| grid.line_gap(-1.0),
            |grid| grid.gap(-1.0),
            |grid| grid.gap(f32::NAN),
            |grid| grid.line_gap(f32::INFINITY),
            |grid| grid.gap(f32::NEG_INFINITY),
            |grid| grid.line_gap(MAX_PACKED_GAP + 1.0),
        ];

        for (index, case) in invalid.into_iter().enumerate() {
            assert!(
                std::panic::catch_unwind(|| case(Grid::new())).is_err(),
                "invalid gap case {index} must panic in debug builds",
            );
        }
    }
}
