use crate::element::{Element, LayoutMode, UiElement};
use crate::primitives::{Color, Corners, GridDef, Sizing, Stroke, Track, TranslateScale, WidgetId};
use crate::shape::{Shape, ShapeRect};
use crate::ui::Ui;
use crate::widgets::Response;
use std::hash::Hash;

/// WPF-style grid: explicit row + column track definitions, per-track
/// `Pixel`/`Auto`/`Star` sizing with optional `[min, max]` clamps, and
/// children placed by `(row, col)` with optional `(row_span, col_span)`.
///
/// Track sizing maps 1:1 to `Sizing`: `Fixed` = Pixel, `Hug` = Auto,
/// `Fill(weight)` = Star. Star tracks split the leftover after Fixed and Hug
/// tracks resolve, weighted, with bounded constraint resolution if any
/// `Track::min` / `Track::max` clamps fire.
///
/// See `docs/grid.md` for the algorithm and explicit non-goals (no
/// Auto-vs-Star cyclic dependency, no `SharedSizeScope`, no auto-flow).
pub struct Grid {
    element: UiElement,
    fill: Color,
    stroke: Option<Stroke>,
    radius: Corners,
    rows: Vec<Track>,
    cols: Vec<Track>,
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
        // Mode is patched at `show()` time once we know the grid_def index.
        // Until then keep it as a placeholder — never observed by layout.
        Self {
            element: UiElement::new(id, LayoutMode::Grid(u32::MAX)),
            fill: Color::TRANSPARENT,
            stroke: None,
            radius: Corners::ZERO,
            rows: Vec::new(),
            cols: Vec::new(),
            row_gap: 0.0,
            col_gap: 0.0,
        }
    }

    pub fn rows<I, T>(mut self, rs: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Track>,
    {
        self.rows = rs.into_iter().map(Into::into).collect();
        self
    }

    pub fn cols<I, T>(mut self, cs: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<Track>,
    {
        self.cols = cs.into_iter().map(Into::into).collect();
        self
    }

    /// Shorthand: `n` equal-weight `Fill` columns.
    pub fn equal_cols(mut self, n: usize) -> Self {
        self.cols = (0..n).map(|_| Track::new(Sizing::FILL)).collect();
        self
    }

    /// Shorthand: `n` equal-weight `Fill` rows.
    pub fn equal_rows(mut self, n: usize) -> Self {
        self.rows = (0..n).map(|_| Track::new(Sizing::FILL)).collect();
        self
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

    pub fn fill(mut self, c: Color) -> Self {
        self.fill = c;
        self
    }
    pub fn stroke(mut self, s: impl Into<Option<Stroke>>) -> Self {
        self.stroke = s.into();
        self
    }
    pub fn radius(mut self, r: impl Into<Corners>) -> Self {
        self.radius = r.into();
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
        let idx = ui.tree.push_grid_def(GridDef {
            rows: self.rows,
            cols: self.cols,
            row_gap: self.row_gap,
            col_gap: self.col_gap,
        });
        let mut element = self.element;
        element.mode = LayoutMode::Grid(idx);

        let fill = self.fill;
        let stroke = self.stroke;
        let radius = self.radius;
        let node = ui.node(element, |ui| {
            ui.add_shape(Shape::RoundedRect {
                bounds: ShapeRect::Full,
                radius,
                fill,
                stroke,
            });
            body(ui);
        });

        let state = ui.response_for(id);
        Response { node, state }
    }
}

impl Element for Grid {
    fn element_mut(&mut self) -> &mut UiElement {
        &mut self.element
    }
}
