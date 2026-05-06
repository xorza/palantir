# Damage rendering

## Next

- **Multi-rect damage.** N disjoint regions instead of one union;
  avoids 50 % heuristic tripping on unrelated corners.
- **Incremental hit-index rebuild.** Only update `HitIndex` for dirty
  + cascade-changed nodes.
- **Debug overlay.** Flash dirty nodes + outline damage rect.
- **Damage-aware encode replay.** Today `damage_filter.is_some()`
  bypasses encode cache; gate replay on
  `screen_rect ∩ damage = ∅` instead.

## Later — lower-impact

- **Tighter damage on parent-transform animation.** Dedicated
  transform-cascade pass collapsing deep-subtree damage.
- **Manual damage verification.** Visual A/B against `damage = None`
  to catch missed diffs.
- **Damage × rounded clip fixture.** Partial-damage frames inside a
  `Surface::rounded(...)` panel are untested. Theory: `LoadOp::Clear(0)`
  per frame plus cmd-buffer replay handles it (every paint redraws
  the mask), but no fixture pins it. See `src/renderer/rounded-clip.md`.
