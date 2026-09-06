# Open issues

- `Slider::new(value, range)` takes the range as a constructor argument while
  `DragValue::new(value).range(..)` takes it as a builder step. The two widgets
  share `DragNum` and `ValueResponse` but not this shape.
- `Sizing::fixed` and `Sizing::fill` panic on a negative or non-finite `f32`,
  while `Sizing::share` and `Sizing::split` in the same file are total.
- `ImageLoadError` lives in `renderer/texture_limit.rs`, while the crate's other
  error types live in an `error.rs` of their own.
