# Open issues

- `Shape::quadratic_bezier` (public, `src/shape/mod.rs`) takes four parameters.
- `Popup`, `Modal` and `Tooltip` (`src/widgets/{popup,modal,tooltip}/mod.rs`) record through the crate-private `OverlayScope` (`src/widget_core/overlay_scope.rs`), which a widget written outside the crate cannot reach.
