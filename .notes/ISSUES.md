# Open issues
- `GridCell` (`src/primitives/layout/grid_cell.rs`) is a checked value with getters, but its fields are `pub(crate)`, so crate code reads and can write them past the getters and the constructor's checks.
