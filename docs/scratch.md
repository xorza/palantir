
- `impl WidgetId` instead of `impl Hash`
- gradients, textures

b(crate) fn first_text(result: &L - strange


 let zero_area = local_rect
                    .map(|r| approx_zero(r.size.w) || approx_zero(r.size.h))
                    .unwrap_or(false);
                zero_area || text.is_empty() || approx_zero(color.a) rect - empty?


  7. MultiArena<T> extraction. src/layout/REVIEW.md explicitly names "fourth parallel arena" as the trigger. MeasureCache now
  has four node-indexed parallel columns (desired, text_spans, available, scroll_content) + two variable-length (hugs,
  text_shapes_arena). Right move is one shared live counter + one helper covering the per-snapshot copy/compact/release dance
  currently duplicated five times. Real refactor — 100+ lines, touches every cache path. Separate PR.
  8. Eliminate tmp_text_spans scratch buffer. Currently rebases into a Vec then memcpys into the cache (two O(subtree) passes).
  Could rebase directly into the cache via an iterator if SubtreeArenas.text_spans were impl Iterator<Item=Span> instead of
  &[Span]. Saves one O(subtree) memcpy per cached subtree per frame. Costs: breaks the uniform &[T] shape across SubtreeArenas
  fields, splits the in-place hot path. Marginal win, defer.
