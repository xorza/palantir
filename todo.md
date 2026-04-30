transform



3. **Per-child cross-axis `align: Start | Center | End | Stretch`** — currently `arrange_stack` always pins cross-axis to top-left for non-Fill. Mirrors WPF `HorizontalAlignment`.

## Real simplifications/generalizations

1. **`Decoration` trait/struct.** Frame and Panel hold identical `{ fill: Color, stroke: Option<Stroke>, radius: Corners }` triples and identical `.fill()/.stroke()/.radius()` builders. Same `Element`-trait pattern: `Decoration` struct + `Decorated` trait with default builders. ~30 lines collapsed across both files. (I offered this earlier; it's still on the table.)

2. **Frame always pushes a shape, even when invisible.** `frame.show()` unconditionally adds `Shape::RoundedRect` even when `fill=TRANSPARENT && stroke=None`. Panel guards this via `paints_bg`. Either lift the guard into Frame too, or let Frame inherit from Panel via composition. Wastes one renderer instance per invisible Frame (e.g. a `Frame::new().sense(CLICK)` hit-area).

3. **`Spacing::all(8.0)` lives in two places** — Button's `with_id` sets it on the element; if Button gains more padding-aware logic later this hardcoding will spread. Consider `ButtonStyle` carrying default padding too.
