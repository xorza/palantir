# Visual Testing Suite

Headless wgpu renderer → PNG → diff against per-machine golden images. The
eyeball-replacement for rendering changes: pins the pixel output of
representative scenes so an unintended render/layout regression surfaces as
a failing diff.

## Goals

- Pin visual output of representative scenes (widgets, layout drivers, text,
  shapes, gradients).
- Catch unintended rendering/layout regressions that `cargo test` (semantics,
  not pixels) can't see.

## Non-goals

- Bit-exact cross-GPU reproducibility — the diff is tolerance-based, not an
  exact match.
- Replacing layout/unit tests — this is a coarse backstop, not a substitute
  for `#[test]`s that pin semantics.
- General-purpose animation capture — fixtures normally capture one frame;
  targeted state-transition tests may compare multiple captures when the
  cross-frame behavior itself is the contract.

## Running

The suite is gated behind the `internals` feature (it reaches into
`test_support` helpers), so the bare `cargo test --test visual` errors —
always pass the feature:

```sh
cargo test --test visual --features internals                 # whole suite
cargo test --test visual --features internals gradient        # filter by name
UPDATE_GOLDEN=1 cargo test --test visual --features internals <filter>  # rewrite goldens
```

A missing golden is auto-created on first run (passes with a `NEW GOLDEN`
notice). When a render change is **intentional**, inspect the failure
artifacts under `tests/visual/output/<name>/{actual,expected,diff}.png`
first, then regenerate with `UPDATE_GOLDEN=1` and re-run to confirm green.

Cargo auto-discovers the single test binary from `tests/visual/main.rs` per
the [project-layout][] convention.

[project-layout]: https://doc.rust-lang.org/cargo/guide/project-layout.html

## Layout

```
tests/visual/
├── main.rs              entry: mod decls + harness sanity test
├── harness.rs           wgpu setup + render + readback
├── diff.rs              Tolerance, DiffReport, parallel diff
├── golden.rs            assert_matches_golden + auto-create + UPDATE_GOLDEN
├── fixtures.rs          mod decls + shared DARK_BG const
├── fixtures/
│   ├── widgets.rs       per-widget scenes + shape / curve / gradient fixtures
│   ├── layout.rs        vstack / grid / zstack drivers
│   ├── text.rs          text rendering + partial-damage smoke
│   ├── scroll.rs        scrollbar visuals + warm-cache parity
│   ├── damage.rs        DamageEngine visualization fixtures
│   ├── format_change.rs surface-format-change recreate path
│   ├── hidpi.rs         scale > 1.0 scenes
│   └── multi_window.rs  interleaved retained-frame ownership
├── golden/              gitignored — per-machine PNG references, auto-created on first run
├── output/              gitignored — diff artifacts written on failure
└── visual-testing.md    this file
```

## How it works

- **Harness** (`harness.rs`) — `LowPower` adapter, no surface, renders to an
  offscreen `Rgba8UnormSrgb` texture. `Harness::new()` clones a process-global
  `OnceLock<Gpu>` (device + queue) and a per-thread `COSMIC` `TextShaper`
  (fonts loaded once per worker thread). `Harness::render(physical, scale,
  clear, scene)` returns an `RgbaImage`. Helpers: `render_after_settle(N, …)`
  for fixtures that need warmup frames before capture (e.g. scrollbars reading
  populated state), `render_with_overlay(cfg, …)` for the damage-vis tests.
  `TwoWindowHarness` drives two `WindowRenderer`s through one backend for
  retained-state interleaving checks. Private `readback()` honors the 256-byte
  row alignment.
- **Diff** (`diff.rs`) — `Tolerance { per_channel, max_ratio }`, default
  `(2, 0.001)`: a pixel passes if every channel is within `per_channel`, and
  the image passes if the differing-pixel ratio stays under `max_ratio`.
  Diffing is row-parallel via rayon, reducing to `RowStats { max_delta,
  differing }`. The dumped diff image dims passing pixels to 25 % and marks
  failing pixels solid red. `diff.rs` carries 6 unit tests pinning the
  contract.
- **Golden workflow** (`golden.rs`) — `assert_matches_golden(name, &actual,
  tol)`. Missing golden → auto-write + pass. `UPDATE_GOLDEN=1` force-rewrites.
  On failure, dumps `actual.png` / `expected.png` / `diff.png` into
  `tests/visual/output/<name>/`.

## Fixtures

Grouped by file under `fixtures/`; **the files are the authoritative list.**
Most fixtures diff against a golden; some are **assertion-only** (no golden)
— they check a pixel-pattern invariant directly, which is robust across
machines. Per-fixture tolerance overrides via the `Tolerance` arg (text
fixtures loosen it for glyph AA).

- **`widgets`** — per-widget minimal scenes (button, stroked frame), the
  gradient family (linear / radial / conic, both `Frame` background and
  `add_shape` rounded-rect), rounded-clip surfaces (full-fill child,
  partially-offscreen, survives-resize smoke), shape/curve primitives (AA
  lines, polylines with bevel/round joins + caps, beziers), and record-order
  layering. A few are assert-only (translucent-polyline premultiply, resize
  smoke).
- **`layout`** — vstack fill-weights, grid mixed tracks, zstack centering.
- **`text`** — paragraph and batched row-list rendering, plus a partial-damage
  smoke using a glyph-ink heuristic (assert-only).
- **`scroll`** — vertical / horizontal / xy overflow (corner avoidance),
  no-bar-when-fits, bar-in-reserved-strip. Each renders the scene twice from
  one `Harness` so frame 2 sees the populated `ScrollState` and emits the bar
  (the golden captures frame 2). Plus a warm-cache invariant: three renders,
  asserting frame 3 is byte-identical to frame 2 (catches encoder-cache-replay
  corruption; assert-only).
- **`damage`** — `DamageEngine` visualizations via
  `DebugOverlayConfig::{dim_undamaged, damage_rect}` + a magenta clear that
  exposes the dirty region as a pixel pattern: Skip-path static scene,
  single-change repaint, dirty-region stroke, and multi-rect
  centre-stays-unpainted invariants. All assert-only.
- **`format_change`** — the per-format pipeline path. Flips the swapchain
  color format mid-session; the renderer auto-detects it, builds the new
  format's pipeline set lazily, self-heals the backbuffer, and asserts the
  output matches (and that the format-independent image texture survives
  the switch with no re-upload).
- **`hidpi`** — a complex multi-region dashboard at scale 2.0 (header /
  sidebar / 2×2 cards / footer).
- **`multi_window`** — records different mesh, polyline, and frame-local text
  payload lengths in two windows sharing one backend, then asserts window A's
  spinner-driven `PaintOnly` pixels exactly match its first render after
  window B records in between (assert-only).
- **`main`** — a clear-color readback sanity check (sRGB round-trip).

### Adding a fixture

```rust
#[test]
fn my_scene_matches_golden() {
    let mut h = Harness::new();
    let img = h.render(UVec2::new(W, H), SCALE, DARK_BG, |ui| {
        // build scene
    });
    assert_matches_golden("my_scene", &img, Tolerance::default());
}
```

Drop it in the appropriate `fixtures/*.rs` (or add a new file + a `mod` to
`fixtures.rs`). First run auto-creates the golden — eyeball it before relying
on it. Override tolerance via the `Tolerance` arg.

## Slow startup? `cargo clean`.

Symptom: a single visual test takes 15+ seconds, almost all in
`Harness::new()` → `gpu()` → `wgpu::Instance::request_adapter`.

Cause: on macOS, Metal's first-device init does
`MTLCopyAllDevices` → `IOSurfaceClientCopyGPUPolicies` →
`[NSBundle mainBundle]` localized-resource lookup, which `readdir`s
**every entry in the executable's parent directory**. Test binaries
live in `target/debug/deps/`, which accumulates `.rcgu.o` incremental
codegen shrapnel across rebuilds — once it crosses ~400k files,
adapter init is linear in dir size and stalls for 15+ seconds. Same
binary placed in a directory with fewer entries (e.g.
`target/debug/examples/`) initializes in 2-3 seconds.

Fix: `cargo clean`. Visual suite goes from ~18s → ~0.3s. If it
recurs, run `cargo clean` again, or set `CARGO_INCREMENTAL=0` for
test runs (trades incremental rebuild speed for not accumulating
rcgu files).

## Current limitations

- **Local-only, no CI.** No GitHub Actions job; the suite runs on dev
  machines only. It is **not** `#[ignore]`d — it's fast (~1 s) and
  deterministic on dev machines. If flakiness appears, the intended path is to
  gate behind `#[ignore]` + a pinned runner (`cargo test --test visual
  --features internals -- --ignored`).
- **Goldens are per-machine.** `.gitignore` excludes `tests/visual/golden/`,
  so goldens are local and auto-created: the suite catches regressions against
  *your own* last-accepted render, not a shared baseline. Committing them
  (likely per-arch, `golden/<arch>/...`) is the prerequisite for a shared CI
  baseline.
- **Adapter selection** — `LowPower`. Revisit if dev-machine vs CI runners
  diverge.
