use crate::*;

pub struct Grid {
    frag: Fragment,
    cols: u32,
    rows: u32,
}

pub struct GridPosition {
    row: u32,
    column: u32,
    row_span: u32,
    column_span: u32,
}
impl Default for GridPosition {
    fn default() -> Self {
        Self {
            row: 0,
            column: 0,
            row_span: 1,
            column_span: 1,
        }
    }
}
impl GridPosition {
    pub fn from_row_col(row: u32, column: u32) -> Self {
        Self {
            row,
            column,
            ..Default::default()
        }
    }
    pub fn row_span(mut self, span: u32) -> Self {
        self.row_span = span;
        self
    }
    pub fn column_span(mut self, span: u32) -> Self {
        self.column_span = span;
        self
    }
}

impl From<(u32, u32)> for GridPosition {
    fn from((row, column): (u32, u32)) -> Self {
        Self {
            row,
            column,
            ..Default::default()
        }
    }
}
impl From<(u32, u32, u32, u32)> for GridPosition {
    fn from((row, column, row_span, column_span): (u32, u32, u32, u32)) -> Self {
        Self {
            row,
            column,
            row_span,
            column_span,
        }
    }
}

impl Default for Grid {
    fn default() -> Self {
        Self {
            frag: Fragment::default(),
            cols: 1,
            rows: 1,
        }
    }
}

impl Grid {
    pub fn rows_cols(self, rows: u32, cols: u32) -> Self {
        Self {
            cols,
            rows,
            ..self
        }
    }
    pub fn add_item<P, T: View>(self, grid_pos: P, item: T) -> Self
    where
        P: Into<GridPosition>,
    {
        self
    }
}

impl View for Grid {
    fn frag(&self) -> &Fragment {
        &self.frag
    }
    fn frag_mut(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}

impl ItemsView for Grid {}
