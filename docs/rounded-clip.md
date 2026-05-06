# Rounded-corner clipping

Status: design — not implemented.

## Goal

Let containers clip their content to **rounded corners**, not just an axis-aligned
rect. Apply uniformly to quads **and** text (glyphon). Pay nothing if the app
never asks for rounded clip.

## Tri-state clip mode

Replace `PaintCore.clip: bool` with:

```rust
#[repr(u8)]
pub enum ClipMode {
    None    = 0,  // no clip established here
    Rect    = 1,  // axis-aligned scissor — fast path, current behavior
    Rounded = 2,  // SDF-rounded clip via stencil — slow path
}
```

`PaintAttrs` packs it into the existing 1-bit `clip` slot expanded to 2 bits
(steal one bit from the unused-extras nibble — `extras: u16` side table is the
fallback if the byte is full).

Builder API on `Configure`:
- `.clip(true)` keeps working → `Rect` (back-compat for the showcase / tests).
- `.clip_rounded()` → `Rounded`. Takes the corner radii from the element's own
  `Background` (the panel's painted radius); same shape used for the visible
  fill is the shape used for the clip — no second source of truth.

If `Rounded` is set but the element has no `Background` / zero radius, fall
back to `Rect` at encode time.

## Pipeline (zero-cost-when-unused)

The encoder counts `Rounded` pushes per frame into `FrameOutput.rounded_clip_count`.
Backend branches at frame start:

```
rounded_clip_count == 0   → "plain" path
                            - render pass: color attachment only
                            - quad pipeline: no depth_stencil
                            - text renderer: depth_stencil = None
                            - identical to today

rounded_clip_count > 0    → "stencil" path (lazy-init, kept warm after first hit)
                            - render pass: color + stencil attachment
                            - quad pipeline (stencil variant)
                            - quad pipeline (mask-write variant: writes stencil, no color)
                            - text renderer (stencil variant) — second TextRenderer
                                with depth_stencil = Some(...) over the same TextAtlas
```

Both `TextRenderer`s share the **same `TextAtlas`** (atlas caches pipelines by
`(format, multisample, depth_stencil)` — verified at
`tmp/glyphon/src/cache.rs:218`). No glyphon fork.

The stencil texture and stencil-variant pipelines are created the **first**
time a frame contains a rounded clip and kept thereafter — apps that never use
rounded clip never allocate either.

## Command-buffer changes

`CmdKind::PushClip(Rect)` becomes:

```rust
PushClip { rect: Rect, radius: Option<Corners> }
```

Composer-side clip stack carries `(URect, Option<Corners>)`. Existing
scissor-only logic is unchanged when `radius == None`. When `radius.is_some()`,
the composer additionally:

1. Emits a "stencil-write" group (mask-write pipeline, draws one rounded SDF
   quad into stencil, no color), with stencil ref = current nesting level.
2. Sets subsequent groups' stencil_ref to that same level until the matching
   `PopClip`, which emits a "stencil-clear-region" group (or decrements via
   the matching mask-write with op = Decrement).

## Stencil semantics

Stencil compare = `Equal`, ref = current nesting level.
Mask-write at level N: pass `Always`, op `Replace(N)`.
Pop at level N: redraw the same rounded shape with op `Replace(N-1)`.

This handles nested rounded clips up to 255 levels (single-byte stencil),
which is well past anything realistic. Two nested rounded clips don't
**intersect** (stencil is just "inside this shape"), but inside-of-inside is
correct because the outer frame already trimmed the outer; the inner only ever
sees pre-clipped pixels.

## Per-fragment SDF inside the stencil mask

The stencil resolves the cheap discrete "in/out" question. Anti-aliased edges
on the rounded boundary still need SDF — the mask-write pipeline reuses
`sdf_rounded_rect` from `quad.wgsl` and writes stencil only where coverage
> 0.5 (hard threshold). The visible fill of the panel is **separately** drawn
through the normal quad pipeline with full AA. Net effect: AA edges come from
the panel's own painted background; stencil clips child content with a 1-px
hard edge **inside** the panel's anti-aliased rim, which is invisible at any
realistic radius.

If that hard inner edge ever shows: upgrade the mask-write to write coverage
to a separate single-channel attachment and sample it in the child pipelines.
Defer until proven needed.

## Implementation slices

Ship in this order; each slice ends green on `cargo nextest run` + visual
goldens, with a showcase tab demonstrating the new state.

1. **`ClipMode` plumbing, no rendering change.**
   - Add the enum, widen `PaintAttrs` packing, update `Configure::clip` /
     `clip_rounded` builders, encoder/composer treat `Rounded` as `Rect`
     for now.
   - Pinning test: `clip_rounded` panel still scissors like `Rect`.

2. **Stencil attachment + plain-path bypass.**
   - Backend: lazy stencil texture, two render-pass variants, frame-level
     `if rounded_count == 0` branch picks plain path.
   - At this slice the stencil path exists but no shapes write to it — purely
     plumbing. Verify pipelines compile, no perf regression on plain path.

3. **Quad rounded clip.**
   - Mask-write pipeline (stencil-only quad), stencil-aware quad pipeline,
     `PushClip { radius: Some }` writes mask before child draws, `PopClip`
     restores. Showcase tab: a panel with `clip_rounded()` and child rects
     that overflow — verify children obey the rounded boundary.

4. **Text rounded clip.**
   - Second `TextRenderer` with `depth_stencil = Some(...)`, sharing the
     atlas. Backend selects which renderer to call based on the active
     stencil state of the current group. Same showcase tab gains a long
     text run that overflows the rounded boundary.

5. **Nested rounded clip.**
   - Stencil ref counter, push/pop bookkeeping, test fixture with two
     nested rounded panels.

## Open questions

- Is the text glyph atlas's existing `TextBounds` CPU cull still useful
  on the stencil path? Yes — keep it; stencil is per-fragment, CPU bound
  cull skips whole layout runs that don't touch the rect at all.
- Damage interaction: a partial-damage rect inside a rounded-clipped
  subtree currently scissors to the damage rect; with stencil, the
  stencil-write must be re-issued every frame the subtree paints. The
  encode cache already keys on subtree hash, so this falls out naturally
  — but verify no stale stencil leaks across the cached-subtree skip
  path.

## Cost summary

| App profile | Cost vs today |
|---|---|
| No rounded clips anywhere | **0** (plain path is the current path) |
| Rounded clip in one panel | One stencil texture, one extra mask-write draw per affected group, stencil-variant pipelines compiled lazily |
| Heavy use | Same as above; pipelines and texture amortize |

Estimate: ~400 lines mostly in `src/renderer/backend/` and
`src/renderer/frontend/composer/`, plus the enum widening through
`tree/element` and `cmd_buffer`.
