# Grid Panel — Design

WPF-flavored grid with explicit row/column definitions, per-track Pixel/Auto/Star sizing, per-track min/max clamps, and explicit cell placement.

## Authoring API

```rust
Grid::new()
    .cols([
        Track::fixed(120.0),
        Track::fill().min(200.0).max(400.0),
        Track::hug(),
    ])
    .rows([Track::hug(), Track::fill()])
    .gap(8.0)                        // uniform; .gap_xy(row, col) when needed
    .fill(panel_bg())                // inherits Panel visuals (fill/stroke/radius/clip)
    .show(ui, |ui| {
        Label::new("Title")
            .grid_cell((0, 0))       // (row, col), default (0, 0)
            .grid_span((1, 3))       // (row_span, col_span), default (1, 1)
            .show(ui);
        Frame::with_id("body").grid_cell((1, 1)).show(ui);
    });
```

Bare `Sizing` is accepted via `From<Sizing> for Track`, so quick grids stay terse:

```rust
Grid::new().cols([Sizing::FILL, Sizing::FILL, Sizing::FILL]).show(ui, |ui| { … });
```

## Types

```rust
pub struct Track {
    pub size: Sizing,        // Pixel = Fixed, Auto = Hug, Star = Fill(weight)
    pub min: f32,            // default 0.0
    pub max: f32,            // default f32::INFINITY
}

pub struct GridCell {
    pub row: u16,
    pub col: u16,
    pub row_span: u16,       // default 1
    pub col_span: u16,       // default 1
}
```

## Storage

- `LayoutMode::Grid(u16)` — index into `Tree::grid_defs: Vec<GridDef>`. Mode stays `Copy`. Cleared per frame with the rest of the tree.
- `Layout::grid: GridCell` — 8 bytes packed. Default `(0, 0, 1, 1)`. Inert when the parent isn't a Grid.
- `GridDef { rows: Vec<Track>, cols: Vec<Track>, row_gap: f32, col_gap: f32 }` lives only on the Tree side-arena, so per-node footprint stays small and `UiElement`/`Layout` stay `Copy`.

## Element trait additions

```rust
fn grid_cell(self, (row, col): (u16, u16)) -> Self;
fn grid_span(self, (rspan, cspan): (u16, u16)) -> Self;
```

Lives on `Element` (same as `position` for Canvas) — inert when parent layout doesn't read it.

## Algorithm

Two-pass, single resolution, no cyclic loop. Targets the "95% case" called out in `references/wpf.md` §7-8 and `references/SUMMARY.md` §3.

### Measure (post-order)

1. Resolve `Fixed` tracks — clamp to `[min, max]`. Final.
2. Walk children once. For each, build `available` per axis as the sum of currently-known sizes of its spanned tracks; tracks not yet resolved (`Hug`, `Fill`) contribute `∞` (the WPF infinity trick — children report intrinsic).
3. For each `Hug` track, take `max(child.desired)` over **span-1 children only** in that track, clamp to `[min, max]`. Span >1 children don't drive Auto sizes — drops the cyclic `c_layoutLoopMaxCount` loop entirely.
4. Grid's `desired` content size = sum of resolved track sizes (Fill contributes `0` here) + gaps. Outer wrap (margin / padding / min-max / Sizing) handled by the existing `measure` framework around the grid node.

### Arrange (pre-order)

1. Subtract gaps from the inner rect.
2. Subtract resolved `Fixed` and `Hug` track sizes → `remaining`.
3. **Resolve `Fill` tracks by exclusion** (CSS Grid / Flutter flex algorithm — bounded, no convergence question):
   ```
   flexible = all Fill tracks
   loop:
       candidate(t) = remaining * t.weight / Σ flexible.weight
       if any t with t.min > candidate(t):
           resolve t to t.min, remove from flexible, remaining -= t.min, restart
       if any t with t.max < candidate(t):
           resolve t to t.max, remove from flexible, remaining -= t.max, restart
       else:
           assign candidate to each remaining flexible, done
   ```
   Each iteration resolves at least one track → O(N²) worst case in track count, ~3-4 in practice.
4. Each child's slot = bounding rect of `[col..col+col_span] × [row..row+row_span]`, internal gaps included. Pass slot to existing `place_axis` → existing per-cell `Sizing::Fixed/Hug/Fill` and `Align` semantics work unchanged inside the slot.

### Cost

- Layout LOC: ~100 (tracks + grid measure + grid arrange + star exclusion).
- New types: `Track`, `GridCell`, `GridDef`, `Grid` builder, `LayoutMode::Grid`.
- No caching. Re-runs every frame. Matches the rest of the engine.

## Decisions

| Decision | Choice | Why |
|---|---|---|
| Track sizing | Reuse `Sizing` | `Fixed`/`Hug`/`Fill` already maps 1:1 to Pixel/Auto/Star. |
| Per-track min/max | Yes (`Track`) | Common ergonomic need (sidebar `Fill` clamped `[200, 400]`). +30 LOC, no algorithmic cost beyond the exclusion loop. |
| Auto sizing from spanning children | No — span-1 only contributes | Drops WPF's `c_layoutLoopMaxCount` cyclic iteration. Matches what most WPF code does in practice. |
| Auto-vs-Star cyclic dependency | Not handled | Requires a third measure pass (re-measure wrapping text given resolved Star width). Defer until wrapping text lands; revisit then. See `references/clay.md` §4. |
| Hug grid + Fill tracks | Fill contributes 0 to grid's Hug | Match WPF. Predictable. Use Auto tracks if you want hug-by-content. |
| Auto-flow / implicit cells | No | Explicit `grid_cell` only. Flow is a recorder concern, not a layout concern — can land later without changing layout. |
| `SharedSizeScope` (cross-grid column sync) | No | Almost-never-used WPF feature. A single outer Grid does the same thing. |
| Gap | Single `.gap(f32)` uniform; `.gap_xy(row, col)` when asymmetric | Matches existing `.gap` on stacks. |
| `Grid::new().cols(3)` shorthand for 3 equal Fill | Yes | Cheap; common case. |
| Builder shape | Separate `Grid` / `GridBuilder`, delegates to a wrapped `Panel` | Keeps `Panel` `Copy`-friendly and small. Builder owns the `Vec<Track>` until `show()`. |

## Out of scope (revisit later)

- **Wrapping text inside grid cells.** Needs the third-pass re-measure. Track in `CLAUDE.md` "Status" alongside glyphon work.
- **`MinHeight`/`MaxHeight` on the grid itself** as a function of resolved tracks. Already covered by the existing per-element `min_size`/`max_size`.
- **Track-level `Auto` that grows under spanning children.** Adds O(N) per Auto track and re-introduces the iteration question. Skip until a real use case shows up.

## Showcase

`examples/showcase/grid.rs`:

- Header row spanning all columns + body grid (Fixed sidebar / Fill content / Hug right rail).
- Min/max-clamped Fill column to demonstrate the exclusion algorithm.
- Asymmetric `gap_xy`.
- Mix of `Sizing::FILL` shorthand and explicit `Track` configurations.
