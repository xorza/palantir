# Invalidation

## Next

- **Property tracker.** Per-widget input-bag hash so encode cache
  decides invalidation without `(NodeHash, cascade)` equality.
- **`request_discard` for first-frame size mismatch.** Re-run frame
  invisibly when measure differs from last frame (text reflow,
  shape miss). egui-style.
