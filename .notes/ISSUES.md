# Open issues

- `atomic/text_atlas/gpu/stable_keys/prepare` and `steady_warm/prepare`, which rasterize nothing,
  run 8 to 15 % slower on the atlas pool than on its base (`1851f60e2`) on every bench host, about
  0.7 ns a glyph replayed.
- Gradient rows registered by two windows whose ramps together pass the table's rows by a few miss
  on every registration of every frame, each evicting a row the other window draws, and the table
  never grows: `RowTable::register` evicts the tail whenever a flush came since its last
  use, and each window's submit is a flush.
- A memory warning's purge reaches neither the gradient row texture nor the CPU staging of
  `RowTable`, which keeps the size of its largest flush for the process's life; the pool
  `src/gpu/AGENTS.md` describes as owning every atlas byte holds the page lists and scratch alone.
- `RowTable::halve` under a row cap that is no power of two goes below
  `INITIAL_ROWS` (a cap of 3000 halves 375 rows to 187), and bakes every row live in the
  window in one frame, drawn or not, through a stable sort that allocates.
- `RowTable`'s index rehashes, and once a size resizes, inside a `register` under eviction
  churn, against the comment in `resize_rows`: its `reserve` leaves no room for tombstones.
- A cutout table a drain moved is baked again whole at its next frame, though the commit moved its
  texels: `PlacedTable::baked` compares the table's code, which holds its placement.
- On a device whose passes move their whole target, a frame that bakes a cutout table or blurs a
  backdrop result that lies on a page opens a load-and-store pass over the whole page, the cost
  `MoveRoute` avoids for a drain's moves.
- The spun raster pipelines compile with the flat ones for every pair of slot sets a session meets
  and at startup, whether or not a frame turns a glyph or an icon.
