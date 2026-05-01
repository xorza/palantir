use crate::element::{Configure, Element, LayoutMode};
use crate::primitives::{Sizing, Track, TranslateScale, WidgetId};
use crate::ui::Ui;
use crate::widgets::{Background, Response, Styled};
use std::hash::Hash;
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
/// See `docs/grid.md` for the algorithm and explicit non-goals (no
/// Auto-vs-Star cyclic dependency, no `SharedSizeScope`, no auto-flow).
pub struct Grid {
    element: Element,
    background: Background,
    rows: Option<Rc<[Track]>>,
    cols: Option<Rc<[Track]>>,
    row_gap: f32,
    col_gap: f32,
}

impl Grid {
    #[track_caller]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::for_id(WidgetId::auto_stable())
    }

    pub fn with_id(id: impl Hash) -> Self {
        Self::for_id(WidgetId::from_hash(id))
    }

    fn for_id(id: WidgetId) -> Self {
        // Mode is patched at `show()` time once `push_grid_def` returns the
        // real index. Initialize with a placeholder that `Tree::push_node`'s
        // bounds-check rejects, so any code path that reaches the tree
        // without going through `show()` panics loudly.
        Self {
            element: Element::new(id, LayoutMode::Grid(PENDING_GRID_IDX)),
            background: Background::default(),
            rows: None,
            cols: None,
            row_gap: 0.0,
            col_gap: 0.0,
        }
    }

    pub fn rows(mut self, rs: impl Into<Rc<[Track]>>) -> Self {
        self.rows = Some(rs.into());
        self
    }

    pub fn cols(mut self, cs: impl Into<Rc<[Track]>>) -> Self {
        self.cols = Some(cs.into());
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
        self.row_gap = g;
        self.col_gap = g;
        self
    }

    /// Asymmetric gaps: `row_gap` between rows, `col_gap` between columns.
    pub fn gap_xy(mut self, row_gap: f32, col_gap: f32) -> Self {
        self.row_gap = row_gap;
        self.col_gap = col_gap;
        self
    }

    pub fn clip(mut self, c: bool) -> Self {
        self.element.clip = c;
        self
    }
    pub fn transform(mut self, t: TranslateScale) -> Self {
        self.element.transform = Some(t);
        self
    }

    pub fn show(self, ui: &mut Ui, body: impl FnOnce(&mut Ui)) -> Response {
        let id = self.element.id;
        let rows = self.rows.unwrap_or_else(empty_tracks);
        let cols = self.cols.unwrap_or_else(empty_tracks);
        let idx = ui
            .tree
            .push_grid_def(rows, cols, self.row_gap, self.col_gap);
        let mut element = self.element;
        element.mode = LayoutMode::Grid(idx);

        let background = self.background;
        let node = ui.node(element, |ui| {
            background.add_to(ui);
            body(ui);
        });

        let state = ui.response_for(id);
        Response { node, state }
    }
}

impl Configure for Grid {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

impl Styled for Grid {
    fn background_mut(&mut self) -> &mut Background {
        &mut self.background
    }
}

/// `LayoutMode::Grid(PENDING_GRID_IDX)` marks a `Grid` whose `grid_def` index
/// has not yet been bound. `show()` overwrites it; if it ever reaches
/// `Tree::push_node` unpatched, the bounds check there panics.
const PENDING_GRID_IDX: u16 = u16::MAX;

fn empty_tracks() -> Rc<[Track]> {
    thread_local! {
        static EMPTY: OnceLock<Rc<[Track]>> = const { OnceLock::new() };
    }
    EMPTY.with(|cell| cell.get_or_init(|| Rc::from(Vec::<Track>::new())).clone())
}
