# Visual Testing Suite — Plan

Headless wgpu renderer → PNG → diff against committed golden images.

Status legend: ✅ done · 🟡 partial · ⏳ not started

## Goals

- Pin visual output of representative scenes (widgets, layout drivers, text).
- Catch unintended rendering/layout regressions in PRs.
- Stay opt-in on CI initially (GPU/driver variance — see Step 6).

## Non-goals

- Bit-exact cross-GPU reproducibility.
- Replacing layout/unit tests.
- Animation / multi-frame capture (single static frame per fixture for now).

## Steps

### 1. Add dev-deps ✅
- `image` added with PNG-only features (`default-features = false, features = ["png"]`).
- `pollster` reused — already a regular dep.

### 2. Headless harness ✅
Lives at `tests/visual/harness.rs` (not `src/support/`; integration-test-only, no need to ship it).
- `Harness::new()` — `LowPower` adapter, no surface, `Rgba8UnormSrgb`, wires `CosmicMeasure` into both `Ui` and `WgpuBackend`.
- `Harness::render(physical, scale, clear, scene)` → `RgbaImage`.
- Private `readback()` honors the 256-byte row alignment, returns via `RgbaImage::from_raw`.

### 3. Diff utility ✅
`tests/visual/diff.rs`:
- `Tolerance { per_channel: u8, max_ratio: f32 }` — defaults `(2, 0.001)`.
- `DiffReport { max_channel_delta, differing_pixels, differing_ratio, diff_image }`.
- `diff(actual, expected, tol)` — passing pixels dimmed to 25%, failing pixels solid red.
- 6 unit tests cover identical / within-channel / sparse-outlier-ratio / saturated-fail / strict-zero / dimension-mismatch.

### 4. Test entry ✅ (with deviations)
Cargo's documented multi-file pattern: `tests/visual/main.rs` + sibling modules, auto-discovered as `--test visual`. No `[[test]]` or `#[path]`.

Deviations from the original plan:
- **Not `#[ignore]` by default.** One fixture passes deterministically on dev machines; revisit once we have more fixtures or a CI baseline.
- **Auto-create on missing golden** (in addition to `UPDATE_GOLDEN=1` force-rewrite). First run prints `NEW GOLDEN (no prior image)` and passes.
- No fixture table yet — fixtures are individual `#[test]` fns. Will introduce a table if/when count justifies.

### 5. Initial fixtures 🟡
- ✅ `button_hello` — 256×96, single `Button` with label.
- ⏳ `grid_3x3` — small grid with colored frames, 400×400.
- ⏳ `text_paragraph` — multi-line `Text`, 400×200.
- ✅ Bonus: `readback_returns_clear_color_for_empty_scene` — round-trips clear color through wgpu/sRGB pipeline (no golden, asserts pixel values directly).

### 6. CI posture ⏳
Local-only for now. No GitHub Actions job yet.

### 7. Later (deferred) ⏳
- Auto-import `examples/showcase/` tabs as fixtures.
- Per-fixture tolerance overrides.
- HTML diff report (gallery of failures).
- Multi-frame / interaction capture (hover, focus).
- Hidpi (scale 2.0) variants.

## Open questions

- **Adapter selection** — currently `LowPower`. Revisit if dev-machine vs CI runners diverge.
- **Golden storage** — raw PNG in repo (1.5 KB for current fixture). Re-evaluate at ~50+ fixtures or if any single golden exceeds ~100 KB.
- **Scale factor** — pinned at 1.0. No 2.0 fixtures yet.

## What's left, prioritized

1. Add `grid_3x3` and `text_paragraph` fixtures (Step 5).
2. Decide CI posture (Step 6) — probably gate behind `--ignored` once a second fixture exists, then wire one CI job.
3. Tackle deferred items (Step 7) opportunistically.
