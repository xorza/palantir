# Crate review

Scope: the whole `palantir` crate and `palantir-anim-derive`, read in twelve
slices. The host / window slice had a dedicated design pass. Items marked
**[public]** change the exported surface and wait for a go-ahead (AGENTS.md
"Public API"). The owner's decisions are in "Decisions".

These bugs were checked against the code: the linear gradient axis, the
nested-overlay panic, the window command order, the input queue admission,
the Scroll disabled fold, the PaintOnly gate, the rounded-clip mask radius and
the measure-cache hit test.

# Part 1: Proposal

## Layout

- **Per-node layout state has one copy per frame.** Forest-wide columns that
  measure and arrange write directly, indexed by a layer base. A hit copies
  prev → cur once, and `end_frame` swaps. (Structural: after the bugs.)

## Tests and visibility

- **An item is `pub` only when code outside the crate calls it.** Test-only
  reach-ins move under `#[cfg(test)]`, so the dead-code lint can find a wrong
  gate.
- **A test lives in the suite of the layer it asserts.** Suites under
  `primitives`, `text`, `theme` and `layout/cache` that drive `Ui` or widgets
  move to the layer they test.

## Decisions

Sources: `.notes/research/2026-10-26-review-decisions.md`.

- **Foreign crates are re-exported whole, only when public signatures name
  their types.** `pub use wgpu;` and `pub use glam;` stay. The partial glam
  re-exports (`IVec2`, `UVec2`, `Vec2`) and the second paths of `prelude`
  go. Refused: no re-exports (each caller must track palantir's exact wgpu
  version by hand); the current partial set (two paths for about 60 items).
  Source: Effective Rust Item 24, egui-wgpu and eframe practice.
- **`golden` reads and writes palantir's `Image`.** Its `pub use image` goes,
  and tests/visual takes `image` as a dev-dependency for its crops. Refused:
  keeping `RgbaImage` in the `golden` signatures.
- **`missing_docs` stays on for the library**, with its reason beside it: a
  published crate documents every exported item. Refused: removing it.
- **A stroke's line is `width`, also on a spinner.** Refused: a shape-based
  exception for rings. Source: Compose Material 3 and Flutter `strokeWidth`.
- **Interruption follows the canonical models.** A retargeted spring keeps
  its velocity. A duration retarget back to the segment's start value uses
  the CSS reversing-adjusted start and shortening factor. Refused: zeroing an
  opposing velocity (a C¹ break); a full restart on reversal. Sources:
  SwiftUI (WWDC23), CSS Transitions 2.
- **`Platform` stays public as the input-convention selector**, for app
  labels such as "⌘S" vs "Ctrl+S". Refused: making it crate-private.
- **The solver keeps the table-layout shrink, and the docs name it.** Hug
  shrink is the CSS 2.1 §17.5.2.2 interpolation, grow is WPF star, and
  overflow aligns to the leading edge. Refused: CSS Flexbox §9.7 scaled
  shrink.
- **A shaped-text cache hit is verified by a byte compare** against the
  text the cached buffer holds; the key stays a hash. Refused: std
  `RandomState`, foldhash and FxHash, none of which is exact; a source copy in
  the key, which would make it neither `Copy` nor small.
- **Double-click time and radius stay constants** (500 ms, 5 px). Refused:
  platform queries and an app-set option.

# Part 2: Plan

Each batch leaves the build and tests green. A batch that needs another one
comes after it. Delete an item when it is addressed.

## Batch 16: GPU resources

Files: `src/gpu/{raster/, wgpu_backend/, resource/}`,
`src/renderer/gradient_atlas/`,
`src/renderer/frontend/composer/{rect_grid/, occlusion_pruner.rs}`.

- [ ] **Shelved: atlas growth by pages.** `raster_atlas/side.rs` `grow`. A
  grow frame clears and copies up to about 20 MiB. Structural, with no
  measured workload; the `text_atlas` bench is the trigger.

## Batch 19: Test infrastructure and visibility

Files: `src/internals/`, `src/bench/`, `src/golden/`, `tests/`,
`src/renderer/frontend/composer/{bench.rs, tests/, mod.rs}`,
`src/primitives/layout/visibility/tests.rs`, `Cargo.toml`.

- [ ] **`Goldens::orphans` hides read errors.** `golden/mod.rs:158-160`.
  Target: empty only on `NotFound`; panic with the path otherwise.
- [ ] **About 30 `UiHarness` methods and `Passes` are `pub` for crate tests
  only.** `ui_harness/mod.rs:166-516` and below, `ui_harness/passes.rs`,
  `bench/mod.rs:16`. Target: `pub(crate)` under `#[cfg(test)]`; `bench::fixture`
  `pub(crate)`; rewrite the tier paragraph at `mod.rs:52-55`.
- [ ] **Each golden comparison leases the GPU for the adapter name.**
  `tests/visual/goldens.rs:17-19`. Target: build the suite once.
- [ ] **Dead warmup floor in the alloc probe.**
  `tests/alloc/harness/mod.rs:256-260`.
- [ ] **Copied fixtures.** One `ICON_SVG` and an icon-parking helper in
  `tests/alloc/fixtures/mod.rs`. `ComposeRig` moves to the composer's
  `internals` so `composer/bench.rs:44-60` uses it with
  `TEST_MAX_TEXTURE_DIM`.
- [ ] **A probabilistic race loop with file I/O.** `golden/tests.rs:199`.
  Target: a few iterations, or a direct test of the staged rename.
- [ ] **`rect_grid` widened for a bench.** `composer/mod.rs:28-29`. Target:
  private; `composer::bench` calls it.
- [ ] **Tests at the wrong layer.** `primitives/layout/visibility/tests.rs`
  cases move to the layout, encoder and input suites.
- [ ] **File names.** `internals/dock_fixture.rs`; `bench/kind.rs` and
  `bench/backends.rs`.
- [ ] **Docs.** `tests/alloc/gates/on_gpu.rs:10` (no CI skip by name),
  `ui_harness/tests/fixtures.rs:26`.
- [ ] **`[profile.dev] opt-level = 1` has no reason.** `Cargo.toml:106`.
- [ ] **[public] Partial glam re-exports and `prelude`.** `lib.rs:186-194,
  351-355`. Target: `pub use glam;` whole beside `pub use wgpu;`; the three
  glam items and `prelude` go; callers write `palantir::glam::Vec2`.
- [ ] **[public] `golden` names `image` types.** `golden/mod.rs:9, 41, 88,
  197, 214`. Target: `Goldens` and `DiffReport` take and hold palantir's
  `Image`; `pub use image` goes; tests/visual takes `image` as a
  dev-dependency for `imageops`.
- [ ] **`missing_docs` has no reason.** `lib.rs:9`. Target: a one-line
  reason beside it.
- [ ] **The global guide forbids the re-exports the crate keeps.**
  `~/.claude/CLAUDE.md` "No crate re-exports another crate". Target: one
  exception, "a crate whose public signatures name a foreign type
  re-exports that crate whole".
