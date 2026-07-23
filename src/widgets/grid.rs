use crate::layout::types::limits::valid_gap;
use crate::layout::types::track::Track;
use crate::primitives::background::Background;
use crate::primitives::transform::TranslateScale;
use crate::scene::element::{Configure, ConfigureElement, Element};
use crate::ui::Ui;
use crate::widgets::{InnerResponse, resolve_container_chrome};

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
    element: Element,
    rows: Rows,
    cols: Cols,
    row_gap: f32,
    col_gap: f32,
    chrome: Option<Background>,
}

impl Grid {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            element: Element::grid(),
            rows: [],
            cols: [],
            row_gap: 0.0,
            col_gap: 0.0,
            chrome: None,
        }
    }
}

impl<Rows, Cols> Grid<Rows, Cols> {
    pub fn rows<NewRows: AsRef<[Track]>>(self, rows: NewRows) -> Grid<NewRows, Cols> {
        Grid {
            element: self.element,
            rows,
            cols: self.cols,
            row_gap: self.row_gap,
            col_gap: self.col_gap,
            chrome: self.chrome,
        }
    }

    pub fn cols<NewCols: AsRef<[Track]>>(self, cols: NewCols) -> Grid<Rows, NewCols> {
        Grid {
            element: self.element,
            rows: self.rows,
            cols,
            row_gap: self.row_gap,
            col_gap: self.col_gap,
            chrome: self.chrome,
        }
    }

    /// Uniform gap on both axes. See `gap_xy` for asymmetric gaps.
    pub fn gap(mut self, g: f32) -> Self {
        debug_assert!(
            valid_gap(g),
            "Grid gap must be finite and non-negative, got {g}",
        );
        self.row_gap = g;
        self.col_gap = g;
        self
    }

    /// Asymmetric gaps: `row_gap` between rows, `col_gap` between columns.
    pub fn gap_xy(mut self, row_gap: f32, col_gap: f32) -> Self {
        debug_assert!(
            valid_gap(row_gap) && valid_gap(col_gap),
            "Grid gaps must be finite and non-negative, got row={row_gap}, col={col_gap}",
        );
        self.row_gap = row_gap;
        self.col_gap = col_gap;
        self
    }

    /// See [`Panel::transform`](crate::Panel::transform) — same contract:
    /// applies to body (children + direct shapes), not to chrome;
    /// scale anchors at the grid's own origin.
    pub fn transform(mut self, t: TranslateScale) -> Self {
        self.element.transform = t;
        self
    }

    /// Paint chrome (fill / stroke / corner radius / shadow). `None` is
    /// the default; theme fallback in [`Self::show`] fills it in from
    /// `ui.theme.panel_background` when unset. Pass [`Background::NONE`]
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
        let active_layer = ui.forest.current_layer();
        let id = ui.forest.trees[active_layer].push_grid_def(
            self.rows.as_ref(),
            self.cols.as_ref(),
            self.row_gap,
            self.col_gap,
        );
        let mut element = self.element;
        element.set_grid_def(id);

        // Theme fallback for chrome / clip — see `Panel::show`.
        let chrome = resolve_container_chrome(
            &mut element,
            self.chrome,
            ui.theme.panel_background.as_ref(),
            ui.theme.panel_clip,
        );
        let widget = ui.widget(element);
        let inner = widget.node(ui, chrome.as_ref(), body);
        InnerResponse {
            // Decorative: skip eager `response_for`.
            response: widget.response(ui),
            inner,
        }
    }
}

impl<Rows, Cols> Configure for Grid<Rows, Cols> {
    fn element_mut(&mut self) -> ConfigureElement<'_> {
        self.element.element_mut()
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::Grid;
    use crate::layout::types::limits::MAX_PACKED_GAP;

    #[test]
    fn gaps_validate_and_store_values() {
        let uniform = Grid::new().gap(0.0);
        assert_eq!(uniform.row_gap, 0.0);
        assert_eq!(uniform.col_gap, 0.0);

        let asymmetric = Grid::new().gap_xy(3.0, 5.0);
        assert_eq!(asymmetric.row_gap, 3.0);
        assert_eq!(asymmetric.col_gap, 5.0);

        let above_f16 = Grid::new().gap(MAX_PACKED_GAP + 1.0);
        assert_eq!(above_f16.row_gap, MAX_PACKED_GAP + 1.0);
        assert_eq!(above_f16.col_gap, MAX_PACKED_GAP + 1.0);

        let invalid: [fn(Grid) -> Grid; 9] = [
            |grid| grid.gap(-1.0),
            |grid| grid.gap(f32::NAN),
            |grid| grid.gap(f32::INFINITY),
            |grid| grid.gap(f32::NEG_INFINITY),
            |grid| grid.gap_xy(-1.0, 0.0),
            |grid| grid.gap_xy(0.0, -1.0),
            |grid| grid.gap_xy(0.0, f32::NAN),
            |grid| grid.gap_xy(f32::INFINITY, 0.0),
            |grid| grid.gap_xy(0.0, f32::NEG_INFINITY),
        ];

        for (index, case) in invalid.into_iter().enumerate() {
            assert!(
                std::panic::catch_unwind(|| case(Grid::new())).is_err(),
                "invalid gap case {index} must panic in debug builds",
            );
        }
    }
}
