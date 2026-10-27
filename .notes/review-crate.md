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
- **`UiHarness` keeps its `pub` surface**, as README.md offers it to app
  authors for headless UI tests through `internals`. Refused: narrowing it to
  the methods the allocation suite calls; a supported `testing` module (a
  design pass of its own). Source: the owner, 2026-10-28.
- **`[profile.dev]` stays at `opt-level = 1`.** Source: the owner,
  2026-10-28.
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

- [ ] **The global guide forbids the re-exports the crate keeps.**
  `~/.claude/CLAUDE.md` "No crate re-exports another crate". Target: one
  exception, "a crate whose public signatures name a foreign type
  re-exports that crate whole".
