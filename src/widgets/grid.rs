use crate::forest::element::{Configure, Element};
use crate::layout::types::layout_mode::GridDefId;
use crate::layout::types::track::GridDef;
use crate::layout::types::{sizing::Sizing, track::Track};
use crate::primitives::background::Background;
use crate::primitives::transform::TranslateScale;
use crate::ui::Ui;
use crate::widgets::{InnerResponse, Response, resolve_container_chrome};
use std::rc::Rc;
use std::sync::OnceLock;

/// WPF-style grid: explicit row + column track definitions, per-track
/// `Pixel`/`Auto`/`Star` sizing with optional `[min, max]` clamps, and
/// children placed by `(row, col)` with optional `(row_span, col_span)`.
///
/// Track sizing maps 1:1 to `Sizing`: `Fixed` = Pixel, `Hug` = Auto,
/// `Fill(weight)` = Star. Star tracks split the leftover after Fixed and Hug
/// tracks resolve, weighted, with bounded constraint resolution if any
/// `Track::min` / `Track::max` clamps fire.
///
/// Track lists are passed as `Rc<[Track]>`; the framework only refcount-
/// touches, never copies. Hoist a track list into app state and clone the
/// `Rc` in each frame for zero-alloc steady state at any track count. Inline
/// literals (`[Track::fixed(40.0), ...]`) are accepted via
/// `Into<Rc<[Track]>>` and allocate once per frame for that grid.
///
/// The layout driver documents the three-phase solver and its explicit
/// non-goals: no Auto-vs-Star cycle, `SharedSizeScope`, or auto-flow.
#[derive(Debug)]
pub struct Grid {
    element: Element,
    def: GridDef,
    chrome: Option<Background>,
}

impl Grid {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        Self {
            element: Element::grid(GridDefId::PENDING),
            def: GridDef {
                rows: empty_tracks(),
                cols: empty_tracks(),
                row_gap: 0.0,
                col_gap: 0.0,
            },
            chrome: None,
        }
    }

    pub fn rows(mut self, rs: impl Into<Rc<[Track]>>) -> Self {
        self.def.rows = rs.into();
        self
    }

    pub fn cols(mut self, cs: impl Into<Rc<[Track]>>) -> Self {
        self.def.cols = cs.into();
        self
    }

    /// Shorthand: `n` equal-weight `Fill` columns. Allocates the `Rc<[Track]>`
    /// each call — hoist into app state if you want zero-alloc reuse.
    pub fn equal_cols(self, n: usize) -> Self {
        self.cols(vec![Track::new(Sizing::FILL); n])
    }

    /// Shorthand: `n` equal-weight `Fill` rows. Same alloc note as
    /// `equal_cols`.
    pub fn equal_rows(self, n: usize) -> Self {
        self.rows(vec![Track::new(Sizing::FILL); n])
    }

    /// Uniform gap on both axes. See `gap_xy` for asymmetric gaps.
    pub fn gap(mut self, g: f32) -> Self {
        debug_assert!(g >= 0.0, "Grid gap must be non-negative, got {g}");
        self.def.row_gap = g;
        self.def.col_gap = g;
        self
    }

    /// Asymmetric gaps: `row_gap` between rows, `col_gap` between columns.
    pub fn gap_xy(mut self, row_gap: f32, col_gap: f32) -> Self {
        debug_assert!(
            row_gap >= 0.0 && col_gap >= 0.0,
            "Grid gaps must be non-negative, got row={row_gap}, col={col_gap}",
        );
        self.def.row_gap = row_gap;
        self.def.col_gap = col_gap;
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
    /// `ui.theme.panel_background` when unset.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> {
        let active_layer = ui.forest.current_layer();
        let id = ui.forest.trees[active_layer].push_grid_def(self.def);
        let mut element = self.element;
        element.set_grid_def(id);

        // Theme fallback for chrome / clip — see `Panel::show`.
        let chrome = resolve_container_chrome(
            &mut element,
            self.chrome,
            ui.theme.panel_background.as_ref(),
            ui.theme.panel_clip,
        );
        let id = ui.widget_id(&element);
        let inner = ui.node(id, element, chrome.as_ref(), body);
        InnerResponse {
            // Decorative: skip eager `response_for`.
            response: Response::lazy(id, ui),
            inner,
        }
    }
}

impl Configure for Grid {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

fn empty_tracks() -> Rc<[Track]> {
    thread_local! {
        static EMPTY: OnceLock<Rc<[Track]>> = const { OnceLock::new() };
    }
    EMPTY.with(|cell| cell.get_or_init(|| Rc::from(Vec::<Track>::new())).clone())
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::Grid;

    #[test]
    fn gaps_validate_and_store_values() {
        let uniform = Grid::new().gap(0.0);
        assert_eq!(uniform.def.row_gap, 0.0);
        assert_eq!(uniform.def.col_gap, 0.0);

        let asymmetric = Grid::new().gap_xy(3.0, 5.0);
        assert_eq!(asymmetric.def.row_gap, 3.0);
        assert_eq!(asymmetric.def.col_gap, 5.0);

        let invalid: [fn(Grid) -> Grid; 5] = [
            |grid| grid.gap(-1.0),
            |grid| grid.gap(f32::NAN),
            |grid| grid.gap_xy(-1.0, 0.0),
            |grid| grid.gap_xy(0.0, -1.0),
            |grid| grid.gap_xy(0.0, f32::NAN),
        ];

        for (index, case) in invalid.into_iter().enumerate() {
            assert!(
                std::panic::catch_unwind(|| case(Grid::new())).is_err(),
                "invalid gap case {index} must panic in debug builds",
            );
        }
    }
}
