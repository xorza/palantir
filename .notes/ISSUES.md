# Open issues

- `GradientStops::deserialize` (`src/primitives/brush/gradient/stops/mod.rs:168`)
  constructs `Self(values)` straight from the parsed array. `GradientStops::new`
  sorts by `offset_u8`, and the type doc at `:75` states ascending offset order
  as an invariant of the type that `Eq`/`Hash` and the LUT bake both rely on.
  `bake_stops` (`src/renderer/gradient_atlas/bake.rs:16`) `debug_assert!`s it.
  A deserialized theme whose stops are written out of order keeps that order.

- A non-finite `InputEvent::ScrollPixels` / `ScrollLines` reaches
  `TranslateScale::new`'s release `assert!` and panics.
  `InputState::on_input` (`src/input/input_state/mod.rs:383`) screens only
  `InputEvent::Zoom`; `src/host/winit/input/mod.rs:41` forwards the OS wheel
  delta unchecked. `ScrollState::apply_wheel_pan`
  (`src/widgets/scroll/state.rs:167`) gates on `pan_delta.x != 0.0`, which NaN
  passes, and `f32::clamp` returns NaN for a NaN input — so `offset` becomes
  NaN and `ScrollState::transform` (`:188`) asserts on it.

- `bar_geometry` (`src/layout/scrollbars/mod.rs:150`) can return a negative
  `thumb_offset`. `thumb_size` is floored at `1.0` and `thumb_offset` is
  `.min(viewport.floor() - thumb_size)` with no lower bound, so a viewport
  under one logical pixel that still overflows yields `-1.0`. The early return
  gates on `viewport <= 0.0` only.

- `zoom::clamp` (`src/input/zoom.rs:26`) returns NaN for a NaN product: both
  its comparisons are false and the value falls through to `product as f32`.
  Its callers state the screen as a `debug_assert!`, so a release build has
  none.
