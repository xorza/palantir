
- `impl WidgetId` instead of `impl Hash`
- gradients, textures

b(crate) fn first_text(result: &L - strange

  

   
  7. MultiArena<T> extraction. src/layout/REVIEW.md explicitly names "fourth parallel arena" as the trigger. MeasureCache now
  has four node-indexed parallel columns (desired, text_spans, available, scroll_content) + two variable-length (hugs,
  text_shapes_arena). Right move is one shared live counter + one helper covering the per-snapshot copy/compact/release dance
  currently duplicated five times. Real refactor — 100+ lines, touches every cache path. Separate PR.
