//! [`CatalogView`]: the catalog as the table shows it, filtered and sorted, rebuilt only when the
//! query or the sort changes.

use std::cmp::Ordering;

use crate::app::model::catalog::{Catalog, Object, ObjectId};

/// A column the table sorts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Column {
    Name,
    Kind,
    Mag,
    Ra,
    Dec,
}

impl Column {
    /// How `a` and `b` order by this column.
    fn compare(self, a: &Object, b: &Object) -> Ordering {
        match self {
            Self::Name => a.name.cmp(&b.name),
            Self::Kind => a.kind.cmp(&b.kind),
            Self::Mag => a.mag.total_cmp(&b.mag),
            Self::Ra => a.ra.total_cmp(&b.ra),
            Self::Dec => a.dec.total_cmp(&b.dec),
        }
    }
}

/// The column and direction the table sorts by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sort {
    pub(crate) column: Column,
    pub(crate) descending: bool,
}

/// The rows the table shows: the objects whose name or kind holds the query, ignoring ASCII case,
/// in sort order, ties by id.
#[derive(Debug)]
pub(crate) struct CatalogView {
    /// What the search field holds; [`Self::mark_stale`] after the field changes it.
    pub(crate) query: String,
    sort: Sort,
    rows: Vec<ObjectId>,
    stale: bool,
}

impl CatalogView {
    /// Every object, brightest first.
    pub(crate) const fn new() -> Self {
        Self {
            query: String::new(),
            sort: Sort {
                column: Column::Mag,
                descending: false,
            },
            rows: Vec::new(),
            stale: true,
        }
    }

    pub(crate) const fn sort(&self) -> Sort {
        self.sort
    }

    /// Sort by `column`: a new column ascending, the sorted one turned around.
    pub(crate) fn sort_by(&mut self, column: Column) {
        self.sort = if self.sort.column == column {
            Sort {
                column,
                descending: !self.sort.descending,
            }
        } else {
            Sort {
                column,
                descending: false,
            }
        };
        self.stale = true;
    }

    /// The query moved; the next [`Self::rows`] rebuilds.
    pub(crate) const fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// The rows, rebuilt into the kept buffer if the query or the sort moved since the last call.
    pub(crate) fn rows(&mut self, catalog: &Catalog) -> &[ObjectId] {
        if self.stale {
            self.rebuild(catalog);
            self.stale = false;
        }
        &self.rows
    }

    fn rebuild(&mut self, catalog: &Catalog) {
        let query = self.query.trim();
        self.rows.clear();
        self.rows.extend(
            catalog
                .iter()
                .filter(|(_, object)| {
                    contains_ignoring_case(&object.name, query)
                        || contains_ignoring_case(object.kind.label(), query)
                })
                .map(|(id, _)| id),
        );
        let Sort { column, descending } = self.sort;
        self.rows.sort_by(|&a, &b| {
            let by_column = column.compare(catalog.get(a), catalog.get(b));
            let by_column = if descending {
                by_column.reverse()
            } else {
                by_column
            };
            by_column.then(a.cmp(&b))
        });
    }

    /// Where `id` sits among the rows, if it shows.
    pub(crate) fn position(&mut self, catalog: &Catalog, id: ObjectId) -> Option<usize> {
        self.rows(catalog).iter().position(|&row| row == id)
    }
}

/// Whether `haystack` holds `needle`, ASCII case ignored, with no allocation.
fn contains_ignoring_case(haystack: &str, needle: &str) -> bool {
    needle.is_empty()
        || haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
mod tests {
    use crate::app::model::catalog::{Catalog, ObjectId, SEED, STAR_COUNT};
    use crate::app::model::view::{CatalogView, Column};

    /// The query keeps exactly the objects whose name or kind holds it, case ignored; a sort
    /// orders them, the same column again turns it around, ties fall to the id.
    #[test]
    fn the_view_filters_and_sorts_exactly() {
        let catalog = Catalog::generate(SEED);
        let mut view = CatalogView::new();
        assert_eq!(view.rows(&catalog).len(), catalog.len());

        view.query.push_str("dso 0");
        view.mark_stale();
        let dso: Vec<u32> = (STAR_COUNT..STAR_COUNT + 9).collect();
        view.sort_by(Column::Name);
        assert_eq!(
            view.rows(&catalog)
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
            dso
        );
        view.sort_by(Column::Name);
        let reversed: Vec<u32> = dso.iter().rev().copied().collect();
        assert_eq!(
            view.rows(&catalog)
                .iter()
                .map(|id| id.0)
                .collect::<Vec<_>>(),
            reversed
        );

        view.query.clear();
        view.query.push_str("NEBULA");
        view.mark_stale();
        view.sort_by(Column::Mag);
        let rows = view.rows(&catalog).to_vec();
        assert_eq!(rows.len(), 20, "every third of the 60 deep-sky objects");
        assert!(
            rows.windows(2)
                .all(|pair| { catalog.get(pair[0]).mag <= catalog.get(pair[1]).mag })
        );
        assert_eq!(view.position(&catalog, ObjectId(0)), None);
        assert_eq!(view.position(&catalog, rows[3]), Some(3));
    }
}
