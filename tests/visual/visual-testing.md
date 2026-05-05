# Visual Testing Suite

Headless wgpu renderer → PNG → diff against committed golden images.

Status legend: ✅ done · 🟡 partial · ⏳ not started

## Goals

- Pin visual output of representative scenes (widgets, layout drivers, text).
- Catch unintended rendering/layout regressions in PRs.
- Stay opt-in on CI initially (GPU/driver variance — see CI posture).

## Non-goals

- Bit-exact cross-GPU reproducibility.
- Replacing layout/unit tests.
- Animation / multi-frame capture (single static frame per fixture for now).

## Layout

```
tests/visual/
├── main.rs              entry: mod decls + harness sanity test
├── harness.rs           wgpu setup + render + readback
├── diff.rs              Tolerance, DiffReport, parallel diff
├── golden.rs            assert_matches_golden + auto-create + UPDATE_GOLDEN
├── fixtures.rs          mod decls + shared DARK_BG const
├── fixtures/
│   ├── widgets.rs       per-widget minimal scenes
│   ├── layout.rs        vstack/grid/zstack drivers
│   ├── text.rs          text rendering
│   └── hidpi.rs         scale > 1.0 scenes
├── golden/              committed PNG references
├── output/              gitignored — written on failure
└── visual-testing.md    this file
```

Single test binary (`cargo test --test visual`); Cargo auto-discovers
`tests/visual/main.rs` per the [project-layout][] convention.

[project-layout]: https://doc.rust-lang.org/cargo/guide/project-layout.html

## Status

### Infrastructure ✅
- **Dev-deps** — `image` (PNG-only features). `pollster` reused from regular deps.
- **Harness** (`harness.rs`) — `LowPower` adapter, no surface, `Rgba8UnormSrgb`. `Harness::new()` clones a process-global `OnceLock<Gpu>` (device + queue) and a per-thread `SharedCosmic` (fonts loaded once per worker thread). `Harness::render(physical, scale, clear, scene)` returns an `RgbaImage`. Private `readback()` honors the 256-byte row alignment via `RgbaImage::from_raw`.
- **Diff** (`diff.rs`) — `Tolerance { per_channel, max_ratio }` defaults `(2, 0.001)`. `diff(actual, expected, tol)` is row-parallel via rayon; reduces to a `RowStats { max_delta, differing }`. Diff image dims passing pixels to 25%, marks failing pixels solid red. 6 unit tests pin the contract (identical / within-channel / sparse-outlier-ratio / saturated-fail / strict-zero / dimension-mismatch).
- **Golden workflow** (`golden.rs`) — `assert_matches_golden(name, &actual, tol)`. Missing golden → auto-write + pass with `NEW GOLDEN (no prior image)` notice. `UPDATE_GOLDEN=1` force-rewrites. On failure dumps `actual.png`, `expected.png`, `diff.png` into `tests/visual/output/<name>/`.

### Fixtures ✅ (12 + 1 sanity)
- `widgets`: `button_hello`, `frame_filled_with_stroke`.
- `layout`: `vstack_fill_weights`, `grid_mixed_tracks`, `zstack_centered_button`.
- `text`: `text_paragraph` (looser tolerance for glyph AA).
- `hidpi`: `dashboard` — complex multi-region scene at scale 2.0 (header / sidebar / 2×2 cards / footer).
- `scroll`: `scroll_vertical_overflow`, `scroll_horizontal_overflow`,
  `scroll_xy_overflow` (corner avoidance), `scroll_no_bar_when_fits`,
  `scroll_with_user_padding` (bar lands in reserved strip, not user
  padding). Each renders the scene twice from the same `Harness` so
  frame 2 sees the populated `ScrollState` and emits the bar — the
  golden captures frame 2. Plus
  `scroll_warm_cache_matches_cold_encoded_second_frame` — three
  renders, asserts frame 3 is byte-identical to frame 2 (catches
  encoder-cache-replay corruption like the `exit_idx` bug we hit;
  no golden, pure intra-test invariant).
- `main`: `readback_returns_clear_color_for_empty_scene` — sRGB round-trip sanity, no golden.

### CI ⏳
Local-only. No GitHub Actions job yet. Once we have a second hidpi fixture or any flake reports, gate the suite behind `#[ignore]` and wire one pinned-runner job that runs `cargo test --test visual -- --ignored`.

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

Drop in the appropriate `fixtures/*.rs` (or create a new file + add a
`mod` to `fixtures.rs`). First run auto-creates the golden — eyeball it
before committing. Per-fixture tolerance overrides via the `Tolerance`
arg.

## Deviations from the original plan

- Harness lives in `tests/visual/`, not `src/support/visual_test.rs`. Integration-test-only, no need to ship in the public crate.
- Not `#[ignore]` by default. Suite is fast (~1.2s) and currently deterministic on dev machines.
- Auto-create on missing golden (added to the original `UPDATE_GOLDEN=1` workflow).
- No fixture table — individual `#[test]` fns. Topical grouping under `fixtures/` covers organization without macros.

## Deferred

- Auto-import `examples/showcase/` tabs as fixtures.
- HTML diff report (gallery of failures).
- Multi-frame / interaction capture (hover, focus).
- Additional hidpi variants (scale 1.5, 3.0).
- Promote harness to its own internal crate if/when benches or other
  test binaries want to reuse it (current scale doesn't justify).

## Open questions

- **Adapter selection** — `LowPower`. Revisit if dev-machine vs CI runners diverge.
- **Golden storage** — raw PNG in repo. Largest current golden is `dashboard_hidpi` at 53 KB. Re-evaluate at ~50+ fixtures or if any single golden exceeds ~200 KB.
- **CI gating** — `#[ignore]` + pinned runner is the safe default; the alternative is per-platform goldens (`golden/<arch>/...`) which is more work but catches more.
