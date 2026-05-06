# Layering

## Now

- **Overlay / popup layer.** Tooltips, dropdowns, menus, modals draw
  outside parent clip + above siblings. Separate "always on top" tree
  merged into encoder. See `docs/popups.md`.

## Later — workload-gated

- **Explicit z-order beyond pre-order.** Clay's `zIndex` model;
  relevant once popups exist.
- **Multi-window / multi-viewport.** egui's `Viewport` +
  `IdMap<PaintList>`. Single-surface today.
