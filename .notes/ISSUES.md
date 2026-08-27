# Open issues

- `GradientStops::deserialize` (`src/primitives/brush/gradient/stops/mod.rs:168`)
  constructs `Self(values)` straight from the parsed array. `GradientStops::new`
  sorts by `offset_u8`, and the type doc at `:75` states ascending offset order
  as an invariant of the type that `Eq`/`Hash` and the LUT bake both rely on.
  A deserialized theme whose stops are written out of order keeps that order.
