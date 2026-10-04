# Open issues

- `ImePreedit::cursor` is a public `Option<Span>`, but `Span`'s `start` and `len` are
  `pub(crate)` and it has no public getter or public `Range` conversion, so a widget outside the
  crate cannot read where the input method's cursor sits in the preedit text.
- A `Shadow::inset()` set on a `Background` (widget chrome) paints nothing visible over an
  opaque fill: `layer_ctx.rs` emits every chrome shadow before the fill, inset ones included,
  while CSS paints an inset box-shadow above the background. The same shadow pushed as a shape
  after the fill darkens the inner edge. The showcase's shadows page shows the two side by side.
