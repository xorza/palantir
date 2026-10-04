# Memory locality and frame cost: audit and plan

An audit of the core per-node data structures (`NodeRecord`, the rollup
columns, the layout columns, the cascade, the shape buffer) and of how each
pass reads them. The plan ranks each change by the cost it removes, as
measured. Run the baseline again on a quiet machine before you change
anything, and compare each change against that baseline.

## Result

The CPU frame is not memory-bound. Locality work can recover about 6 % of a
cached frame on the default fixture, and 11–14 % at 10 600 nodes. The larger
costs are the instruction count during recording and a full cascade rebuild
on every scroll frame.

## Baseline

Measured on `50025ce6` (i9-13980HX, P-core 2 pinned, a busy desktop, so
expect some noise).

| Arm | Default fixture (`BENCH_SCALE` 32) | 10× fixture (10 604 nodes) |
|---|---|---|
| `frame/cached_cpu` | 121 µs | 1.02 ms |
| `frame/partial_cpu` | 124 µs | 1.19 ms |
| `frame/scrolling_cpu` | 187 µs | 1.79 ms |
| `frame/resizing_cpu` | 256 µs | 2.29 ms |

Top-down (Intel TMA), `cached_cpu`:

| Metric | Default | 10× |
|---|---|---|
| IPC | 3.74 | 3.81 |
| Retiring | 65 % | 60 % |
| Memory-bound | 5.8 % | 11.4 % |
| Core-bound | 14.4 % | 15.4 % |

Memory-bound split at 10×: L1 5.2 %, store 4.5 %, L3 3.7 %, DRAM 1.0 %,
L2 0.4 %. Inside L1-bound: `l1_latency_dependency` 38 %, `store_fwd_blk`
33 %, `dtlb_load` 6 %. Inside store-bound: `dtlb_store` 35 %.

Flat self-time, `cached_cpu` (same order at both sizes):

| Symbol | Share |
|---|---|
| `Ui::open_node` | 22 % |
| `FrameCycle::post_record` (rollups) | 10 % |
| `Button::show` | 6 % |
| `Widget::resolve` | 5 % |
| `Text::show` | 5 % |
| `WidgetId::auto` | 3 % |
| `Shapes::push` | 3 % |
| `HashMap<WidgetId, ()>::insert` | 2 % |
| libc `memmove` / `memset` (unattributed) | 3 % |

On `scrolling_cpu` and `resizing_cpu`, `CascadeEngine::run_full` (10–13 %) and
`compute_node_paint` (8–11 %) join the top.

Loads of 50 cycles or more (`mem-loads,ldlat=50`), `cached_cpu` at 10×:
`open_node` 32 %, `Widget::resolve` 13 %, `Shapes::push` 11 %,
`Text::show` 11 %, `HashSet<WidgetId>::insert` 7.6 %.

## How to measure

Read `benches/AGENTS.md` and `benches/profiling.md` first. The commands below
are the ones the baseline came from.

```sh
cargo bench -p palantir --features bench --bench criterion --no-run
BIN=$(ls -t target/release/deps/criterion-* | grep -v '\.d$' | head -1)

# Times. `frame` writes a row to the gitignored benches/results/.
"$BIN" --bench -d frame --arms cpu --note '<what changed>' \
    --sample-size 30 --measurement-time 4 --noplot

# A/B one arm.
"$BIN" --bench -d frame --arms cpu --note before --save-baseline before
"$BIN" --bench -d frame --arms cpu --note after --baseline before

# Top-down and memory leaves (Intel; AMD names differ, see bench-perf.sh).
taskset -c 2 perf stat -M TopdownL2 -- "$BIN" --bench -d frame --arms cpu \
    --note audit --profile-time 4 'frame/cached_cpu$'
for g in tma_memory_bound_group tma_l1_bound_group tma_store_bound_group; do
    taskset -c 2 perf stat -M $g -- "$BIN" --bench -d frame --arms cpu \
        --note audit --profile-time 3 'frame/cached_cpu$' 2>&1 >/dev/null \
        | grep -oE '[0-9.]+ %  tma_[a-z_0-9]+'
done

# Flat self-time and slow loads.
taskset -c 2 perf record -F 4000 -o tmp/perf.data -- "$BIN" --bench -d frame \
    --arms cpu --note audit --profile-time 4 'frame/cached_cpu$'
perf report -i tmp/perf.data --stdio --no-children -g none --percent-limit 1.2
taskset -c 2 perf record -e cpu_core/mem-loads,ldlat=50/P -o tmp/mem.data -- \
    "$BIN" --bench -d frame --arms cpu --note audit --profile-time 4 'frame/cached_cpu$'
perf report -i tmp/mem.data --stdio --no-children -g none --sort sym
```

`kernel.perf_event_paranoid=1` is enough for these user-space captures.

**The 10× fixture** is a local edit, never committed: set `BENCH_SCALE` in
`src/internals/frame_fixture/mod.rs` to `320`. To print the node count, add
this line in `run_cpu_arm` (`src/ui/bench.rs`), just before
`group.bench_function`:

```rust
eprintln!("nodes {}", h.frontend.harness.ui.forest.total_nodes());
```

Locality changes show at 10× and hide in the noise at the default size, so
measure both.

## Plan, highest value first

### 1. A transform change rebuilds the whole cascade

`Tree::compute_rollups` folds each node's layout hash into
`fingerprint.cascade_static`. That hash includes `PanelExtras::transform`.
A scroll therefore changes `LayerKey::structure`, so
`CascadeKey::differs_only_in_paint` fails and `CascadeEngine::run_full`
rebuilds every layer. The rebuild writes `entries`, `cascade_inputs`,
`subtree_paint_rects`, `subtree_ends`, every paint row, `arena_hashes`, and
refills `by_id` from `seen.curr` for every node.

- Cost: `scrolling_cpu` − `cached_cpu` = +66 µs (+55 %) at default size and
  +770 µs (+76 %) at 10×.
- Change: keep transforms out of the structure fingerprint. Give the cascade a
  third path between repair and rebuild, which walks again only under a panel
  whose transform changed. That walk rewrites the subtree's `entries`,
  `cascade_inputs`, `subtree_paint_rects` and paint rows. `HitRow`s carry
  screen rects, so the walk must also rewrite the subtree's hit rows, or the
  hit table must hold rows that it can patch by node.
- Check: `frame/scrolling_cpu`, `cascade` driver, the `full_rebuild` counter in
  `CascadeCounters`, and the damage tests for moved subtrees.

### 2. Five hash probes in two widget-id tables for each node

For each node, recording does:

- `SeenIds::resolve`: `curr.contains_key`, `reserved.contains`,
  `reserved.insert`.
- `SeenIds::record_endpoint`: `curr.entry` and `reserved.remove`.
- `rollover` then probes `curr` once for each `prev` key, and a cascade
  rebuild inserts every id into `by_id` again.

These ids are random hashes, so each probe is a random line in a table of
10 600 entries at 10×.

- Cost: the main slow loads (`open_node` 32 %, `Widget::resolve` 13 %,
  `HashSet<WidgetId>::insert` 7.6 %) and 2 % self-time in the out-of-line
  `insert`.
- Change: merge `reserved` into `curr` as one `WidgetIdMap<IdSlot>`, with
  `IdSlot` either `Reserved` or `Recorded(Endpoint)`. `resolve` does one
  `entry` probe and `record_endpoint` does one more. Keep the collision rules
  that `seen_ids/tests.rs` pins: an explicit id may claim its own
  reservation, and an auto id must step past it.
- Check: `frame/cached_cpu` at both sizes, the slow-load report, and
  `cargo test` on `scene::seen_ids`.

### 3. Blocked store forwarding when `Node::columns` reads the node

At 10×, the slow loads inside `open_node` land on reads of the 104-byte
`Node` in `Node::columns` and `LayoutCore::from_node`. An example is a
16-byte `vpshufd` over `0xc(%rbx)`. The builder writes those fields with
narrow stores just before. `store_fwd_blk` is 33 % of L1-bound.

- This is a hypothesis. Confirm it before you change code:

  ```sh
  taskset -c 2 perf record -e cpu_core/LD_BLOCKS.STORE_FORWARD/ppp \
      -o tmp/stfwd.data -- "$BIN" --bench -d frame --arms cpu --note audit \
      --profile-time 4 'frame/cached_cpu$'
  perf annotate -i tmp/stfwd.data -M intel '<palantir::ui::Ui>::open_node'
  ```

- Change, if it holds: let `Node` keep `LayoutCore` and `NodeFlags` in their
  packed form, written as whole fields by the setters. `columns` then copies
  whole fields and does not repack `Option`s. Alternatively, make the setters
  write the same widths that `columns` reads.
- Check: `frame/cached_cpu`, and the `LD_BLOCKS.STORE_FORWARD` count.

### 4. The child walks read `LayoutCore` only to get visibility

`ChildIter::next` and `TreeItems::next` load the 28-byte `LayoutCore` row of
each child to read 2 bits of `meta`. `compute_rollups` and
`compute_paint_rect` drive `TreeItems` and do not use that value. Children are
not adjacent in pre-order, so each child step touches a new line in two
columns (`subtree_end` and `layout`).

- Change: store visibility in `SubtreeEnd`. Only bit 31 (the grid flag) is in
  use, so bits 29–30 can hold it. The arena limit becomes 2^29, and the
  debug assert in `SubtreeEnd::new_open` moves with it. `ChildIter` then reads
  only `subtree_end`. 9 sites call `visibility()`. The layout sites read
  `LayoutCore` anyway and can stay as they are.
- Check: `frame/cached_cpu` (post_record), `frame/resizing_cpu` (cascade), and
  the tree tests.

### 5. The three rollup columns move together

`SubtreeRollups` keeps `node`, `subtree` and `layout_subtree` as three
separate `Vec<ContentHash>`. `compute_rollups` writes all three at the same
index. Its child loop reads `subtree` and `layout_subtree` of each child.
Damage reads `node` and `subtree` together.

- Change: one `Vec<NodeRollup>` with the three hashes in a 24-byte row. Writes
  become one stream, and a child read touches one line instead of two.
- Check: `frame/cached_cpu`, `post_record` self-time.

### 6. `ShapeRecord` is 88 bytes

Text, the common variant, uses 57 bytes. Curve (87), Quad (79), Image (78) and
Mesh (71) set the size. `Option<Rect>` costs 20 bytes, and `ShapeBrush` forces
8-byte alignment. A record never fits in one 64-byte line.

- Change: move the payloads of the rare large variants (Curve, Image, Mesh,
  Polyline) into `RecordStore` behind a 4-byte index, as meshes and polylines
  already do for their vertex data. Give the local rect an encoding without
  the 4-byte `Option` tag. The target is 64 bytes or less.
- Check: `frame/cached_cpu` at 10×, `Shapes::push` in the slow-load report,
  `tests/alloc`.

### 7. Low value

- `LayoutScratch::resize_for` and `LayerLayout::resize_for` fill about
  92 bytes of columns for each node every frame. On a cached frame the
  restore from the snapshot then overwrites them. This is about 1 % at 10×.
  The fill matters only for nodes that measure never visits (under a
  collapsed node), so a change here must keep those defaults.
- libc `memmove` / `memset` is 2.8–3.3 % of self-time. Take a capture with a
  call graph (`CALLGRAPH=lbr` in `benches/bench-perf.sh`) to find the callers
  before you act.
- `dtlb_store` is 35 % of a store-bound share of 4.5 %, which comes from the
  many column streams. It is less than 2 % of the frame.

## Keep as is

- **`NodeRecord` stays SoA.** 56 bytes in 7 columns, LLC misses are few, and
  each pass reads only the columns it needs. AoS would make the cascade and
  layout walks worse.
- **`ExtrasIdx` with 16-bit indices** stays. It costs 6 bytes for each node,
  and its side tables are sparse.

## Order

Start with items 1 and 2: they give the largest saving, and each is a
contained change. Item 3 needs its confirmation capture first. Items 4–6 are
small and independent, and each one is worth more at 10× than at default size.
