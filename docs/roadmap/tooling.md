# Tooling

## Next

- **Profiling spans (tracy / puffin).** `profile_function!` per pass.
- **Pixel-snapping audit at fractional scales** (1.25 / 1.5 / 1.75).
  Yoga shipped 1px gaps; Taffy fixed (aa5b296). Current golden coverage is scale 1.0 + 2.0 only.
- **Color-space verification.** Confirm Glyphon sRGB output on linear
  surface; pin a test.
- **HiDPI / scale-factor change handling.** Per-monitor DPI changes
  must invalidate atlas + text cache + (future) layout cache.

## Later — workload-gated

- **Per-frame scratch arena (`bumpalo`).** Replace per-pass capacity
  retention with one shared arena.
