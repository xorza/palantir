# Visual Testing Suite — Plan

Headless wgpu renderer → PNG → diff against committed golden images.

## Goals

- Pin visual output of representative scenes (widgets, layout drivers, text).
- Catch unintended rendering/layout regressions in PRs.
- Stay opt-in on CI initially (GPU/driver variance — see Step 6).

## Non-goals

- Bit-exact cross-GPU reproducibility.
- Replacing layout/unit tests.
- Animation / multi-frame capture (single static frame per fixture for now).

## Steps

### 1. Add dev-deps
- `image = "*"` (PNG encode/decode).
- `pollster = "*"` (block on async wgpu init).

### 2. Headless harness — `src/support/visual_test.rs`
- `async fn make_device() -> (Device, Queue)` — `Instance::request_adapter` with no surface.
- `struct Harness { backend: WgpuBackend, device, queue, format: Rgba8UnormSrgb }`.
- `fn render_scene(size: UVec2, scale: f32, scene: impl FnOnce(&mut Ui)) -> RgbaImage`:
  1. Create target `Texture` (`RENDER_ATTACHMENT | COPY_SRC`).
  2. `Ui::begin_frame(Display::from_physical(size, scale))` → run `scene` → `end_frame()`.
  3. `backend.submit(&frame_output, &target)`.
  4. `copy_texture_to_buffer` honoring 256-byte row alignment → map → strip padding → `RgbaImage`.

Gate behind `#[cfg(any(test, feature = "visual-test"))]`.

### 3. Diff utility
- `fn diff(actual: &RgbaImage, expected: &RgbaImage) -> DiffReport` with:
  - `max_channel_delta: u8`
  - `differing_pixels: u32`
  - `differing_ratio: f32`
  - `diff_image: RgbaImage` (red overlay where delta > threshold).
- `struct Tolerance { per_channel: u8, max_ratio: f32 }`; default `(2, 0.001)`.

### 4. Test entry — `tests/visual.rs`
- `#[ignore]` by default; run via `cargo test --test visual -- --ignored`.
- Fixture table: `&[(name, UVec2, fn(&mut Ui))]`.
- For each: render → load golden → diff → on failure write `actual/diff` to `tests/visual/output/<name>/`.
- `UPDATE_GOLDEN=1` env var → write `expected.png` instead of asserting.

### 5. Initial fixtures (3, minimal)
- `button_default` — single `Button::new("Click")`, 200×80.
- `grid_3x3` — small grid with colored frames, 400×400.
- `text_paragraph` — multi-line `Text`, 400×200.

Goldens committed under `tests/visual/golden/<name>.png`.

### 6. CI posture
- Local + manual runs only at first.
- Once stable, add a single GitHub Actions job pinned to one runner image, still `--ignored` opt-in.
- Revisit cross-GPU strategy (software adapter? per-platform goldens?) after a few weeks of use.

### 7. Later (deferred)
- Auto-import all `examples/showcase/` tabs as fixtures.
- Per-fixture tolerance overrides.
- HTML diff report (gallery of failures).
- Multi-frame / interaction capture (hover, focus).

## Open questions

- Adapter selection: prefer integrated GPU (`LowPower`) for determinism, or whatever the host has?
- Golden storage: raw PNG in repo, or git-lfs once count grows?
- Scale factor in goldens: pin at 1.0, or also test 2.0 (hidpi snapping)?
