//! The WPF-style grid: explicit row and column tracks, children placed into named cells.

use crate::primitives::layout::track::Track;
use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::InnerResponse;
use crate::widget_core::widget::Widget;
use std::rc::Rc;

/// WPF-style grid: row + column tracks with `Pixel`/`Auto`/`Star` sizing (`Fixed`/`Hug`/`Fill(weight)`) and optional `[min, max]` clamps, children placed by `(row, col)` with optional spans. The layout driver documents the solver and its non-goals.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Grid<Rows = [Track; 0], Cols = [Track; 0]> {
    widget: Widget,
    rows: Rows,
    cols: Cols,
    chrome: Option<Background>,
}

impl Grid {
    /// A grid with no tracks. Give it some through [`Self::rows`] and
    /// [`Self::cols`].
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
    /// The row tracks, as anything that borrows a `[Track]`.
    pub fn rows<NewRows: AsRef<[Track]>>(self, rows: NewRows) -> Grid<NewRows, Cols> {
        Grid {
            widget: self.widget,
            rows,
            cols: self.cols,
            chrome: self.chrome,
        }
    }

    /// The column tracks. See [`Self::rows`].
    pub fn cols<NewCols: AsRef<[Track]>>(self, cols: NewCols) -> Grid<Rows, NewCols> {
        Grid {
            widget: self.widget,
            rows: self.rows,
            cols,
            chrome: self.chrome,
        }
    }

    /// Paint chrome; `None` takes `ui.theme().panel_background`, [`Background::NONE`] suppresses that.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn background(mut self, background: Background) -> Self {
        background.validate();
        self.chrome = Some(background);
        self
    }

    /// Paint `background` unless the caller set one; for wrappers that theme a widget after the caller's setters. An explicit [`Self::background`] wins either order.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn default_background(mut self, background: Background) -> Self {
        background.validate();
        if self.chrome.is_none() {
            self.chrome = Some(background);
        }
        self
    }

    /// Record the grid and its `body`; children name their slot with [`Configure::grid_cell`].
    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R>
    where
        Rows: AsRef<[Track]>,
        Cols: AsRef<[Track]>,
    {
        let mut widget = self.widget;
        widget.grid_tracks(ui, self.rows.as_ref(), self.cols.as_ref());

        // See `Panel::show` for why the handle is what gets cloned.
        let theme = Rc::clone(ui.theme());
        widget.configure().default_clip(theme.panel_clip);
        let chrome = self.chrome.as_ref().or(theme.panel_background.as_ref());
        widget.show(ui, chrome, body)
    }
}

impl<Rows, Cols> Configure for Grid<Rows, Cols> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests {
    use super::Grid;
    use crate::primitives::math::domain::MAX_GAP;
    use crate::widget_core::configure::Configure;

    /// Spacing goes through the node column's shared setters.
    #[test]
    fn gaps_validate_and_store_values() {
        let configured = Grid::new().line_gap(3.0).gap(5.0);
        assert_eq!(configured.widget.authored_line_gap(), Some(3.0));
        assert_eq!(configured.widget.authored_gap(), Some(5.0));

        let edge = Grid::new().line_gap(MAX_GAP).gap(0.0);
        assert_eq!(edge.widget.authored_line_gap(), Some(MAX_GAP));
        assert_eq!(edge.widget.authored_gap(), Some(0.0));
    }
}
