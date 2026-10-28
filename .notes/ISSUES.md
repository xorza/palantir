# Open issues
- A `Grid` cell that spans a `Fill` column still grows the `Hug` columns it spans to fit its min and max content (`distribute_span`, `src/layout/drivers/grid/mod.rs`), where CSS Grid §12.5, which the function cites, leaves an item that spans a flexible track out of that step: a long label across `[Hug, Fill, Hug]` widens both `Hug` columns and leaves the `Fill` one narrow.
