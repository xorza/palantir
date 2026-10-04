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

### Ryzen 7 6800U

Measured again on `c5a4b44c` (Zen3+, core 2 pinned, `performance`
governor, a quiet host). Back-to-back A/B runs of the same binary differ
by up to 1.5 %, so a smaller change needs a repeat before it counts.

| Arm | Default fixture | 10× fixture |
|---|---|---|
| `frame/cached_cpu` | 164 µs | 1.30 ms |
| `frame/partial_cpu` | 175 µs | 1.42 ms |
| `frame/scrolling_cpu` | 247 µs | 2.03 ms |
| `frame/resizing_cpu` | 331 µs | 2.60 ms |

IPC is 3.31 at default size and 3.22 at 10×. The L1 load miss rate is
2.9 %. IBS self-time on `cached_cpu` has the same order as the table
above: `open_node` 22 %, `post_record` 9.5 %, `Widget::resolve` 5.6–6 %,
`Shapes::push` 3.2–3.8 %, `HashMap<WidgetId, ()>::insert` 2.2 %. At 10×,
62 % of the load latency (IBS weight) hits L2 and 20 % hits DRAM.

On AMD, the Intel commands below map to `perf record -e ibs_op//` for
self-time and `perf mem record` for the load sources. Zen3 has no TMA and
no `ldlat` filter.

## Done

- **Item 1, the cascade refresh** (6800U, A/B against `c5a4b44c`):
  `scrolling_cpu` −7 to −9 % at both sizes, `resizing_cpu` −5 % at 10×,
  the other arms inside the noise. The fixture's scroll transform sits on
  the body panel, which holds nearly every node, so the refresh still
  recomputes nearly every row: `cascade/run/transform` stays at 56 µs,
  about 50 ns a node. A pan gets cheaper than that only when the rows
  under a transform stop holding screen space.
- **Item 2, one widget-id table** (6800U, A/B against item 1): every
  `frame/*_cpu` arm −1.2 to −2 % at default size, inside the noise at
  10×. The reserved set held about one id at a time, so the merge saved
  small-table work only. `resolve` and `record_endpoint` still probe the
  large table once each.
- **Item 3, blocked store forwarding** (6800U): confirmed.
  `ls_bad_status2.stli_other` (the Zen counter for a load that cannot
  forward from an older store) counts 2.5 G on `cached_cpu`, as many as
  the successful forwards (`ls_stlf`). The events follow self-time:
  `open_node` 22 %, `Widget::resolve` 15 %, `Text::show` 8 %. The hottest
  single site was the chrome hash, which built a 64-byte struct and read
  it back through `hash_bytes`. Every byte hash of a fresh value now
  feeds register words (`cached_cpu` −0.45 %, `partial_cpu` −1.0 % over
  three alternating rounds). The `Node` packing was not done. A setter
  still writes one field, and `columns` still reads the packed column in
  wider loads, so the seams stay. `Node::columns` is about 10 % of the
  events in `open_node`, and the background reads in `lower::background`
  about 18 %. The rest is the builder pattern: a widget writes a value
  field by field and the next pass reads it whole.
- **Item 4, visibility in `SubtreeEnd`: measured and dropped** (6800U,
  three alternating rounds against item 3). With visibility in bits
  29–30 and `ChildIter`, `TreeItems` and the cascade walk reading it
  from there, every arm got slower: +0.9 to +1.7 % at default size,
  −0.3 to +1.1 % at 10×. The second column a child step touched was not
  a cost on this CPU, so the change was reverted.

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

Items 5 and 6 are small and independent, and each one is worth more at 10× than at default size.
