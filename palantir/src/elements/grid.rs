use crate::*;

pub struct Grid {
    style: Style,
    columns: u32,
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
            style: Style::default(),
            columns: 1,
            rows: 1,
        }
    }
}

impl Grid {
    pub fn columns_rows(self, columns: u32, rows: u32) -> Self {
        Self {
            columns,
            rows,
            ..self
        }
    }
    pub fn add<P, T: View>(self, grid_pos: P, item: T) -> Self
    where
        P: Into<GridPosition>,
    {
        self
    }
}

impl View for Grid {
    fn get_style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl ItemsView for Grid {
    fn items(&self) -> impl Iterator<Item = &dyn View> {
        unimplemented!();

        std::iter::empty()
    }
}

