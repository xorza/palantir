# Review: `src/renderer/frontend/composer/`

Scope: `mod.rs` (475L), `tests.rs` (652L), `compose-cache.md` (79L,
historical). Post the overlap-aware kind-transition refactor.

## Architectural issues

### 1. `GroupBuilder` no longer earns its keep — inline into `Composer`

Before the overlap-rect work, `GroupBuilder` owned the entire flush
decision (the `last_kind` rule). Now the flush trigger lives in
`Composer::compose` (the `any_overlap(...) → group.flush(...)` checks
at `mod.rs:249-253` and `mod.rs:425-427`), and the *state* that drives
those decisions (`text_rects`, `mesh_rects`) lives on `Composer`. The
builder is reduced to five plain `u32`/`Option` fields plus two
methods (`flush`, `set_clip`) that take the rect scratches as `&mut`
parameters and clear them inside.

That's inverted ownership: the helper reaches into the caller's
scratch to mutate it. Every call site reads
`group.flush(out, &mut self.text_rects, &mut self.mesh_rects)` —
three args of plumbing for one bool of useful info.

Fix: delete `GroupBuilder`. Move `current`, `rounded`, `quads_start`,
`texts_start`, `meshes_start` onto `Composer`. `flush` and `set_clip`
become `&mut self` methods that touch their own fields directly. The
"scratch state lives on the engine" pattern matches `clip_stack`,
`transform_stack`, `polyline_scratch`.

References: `mod.rs:24-79`, `mod.rs:160`, `mod.rs:197-218`,
`mod.rs:252`, `mod.rs:426`, `mod.rs:440`.

### 2. `scissor_from_logical` and `urect_from_aabb` are the same function

Both clamp a 2D extent into a viewport-bounded `URect`. Only the
input shape differs: one takes a `Rect` + `scaled_by`, the other
takes already-scaled `Vec2 min/max`. The clamp arithmetic
(`saturating_sub` between two viewport-clipped corners) is identical.

Fix: keep one `urect_from_phys(min: Vec2, max: Vec2, viewport: UVec2)`
that does the clamp; callers do the `scaled_by` (already explicit in
the `_aabb` variant). The `NaN`/`is_finite` guard in
`urect_from_aabb` should fold into the shared helper, since polyline
verts could in principle be NaN too if user geometry is malformed.

References: `mod.rs:444-458`, `mod.rs:460-472`.

### 3. Overlap test runs in clamped-to-viewport space

`quad_urect` at `mod.rs:234` and the text `bounds` at `mod.rs:414`
have already been clamped to `viewport_phys` (and the parent scissor,
for text). So has the mesh AABB at `mod.rs:324`. The overlap test
then asks `URect::intersect`.

The semantic question is: do two off-screen rects that *would*
overlap if extended still overlap after clamping? Usually yes
(intersection commutes with clamp on the same viewport), but the
edge case of a quad that pokes outside on the *opposite* side from a
text run is fragile to reason about. Cheaper-to-trust path: do
overlap detection in float-space `Rect`s (intersect the unclamped
physical-px rects) and reserve `URect` for what actually rides on
the GPU (scissor, glyph bounds). Costs an extra `Rect` field on the
scratch vecs but removes a class of "did the clamp eat the overlap?"
worries.

Lower-priority than #1/#2 — no known bug, just brittle.

### 4. Mesh AABB doesn't account for AA fringe; polyline does

`DrawPolyline` inflates by the tessellator's outer fringe
(`max(w/2, 0.5) + 0.5` phys px, `mod.rs:346`) and uses the inflated
rect for both cull *and* overlap (`mod.rs:355`, `mod.rs:403`).

`DrawMesh` uses the raw vertex AABB (`mod.rs:287-324`). That's
correct *only if* user meshes paint exactly within their vertex hull
— no per-fragment AA, no vertex displacement. The current mesh
pipeline does premultiplied-alpha blend with vertex colors, so a
mesh with semi-transparent edge verts can light pixels outside the
geometric AABB (subpixel coverage). Today that probably doesn't
matter; tomorrow when someone adds AA-fringed mesh, the overlap test
will silently produce paint-order bugs because false negatives
*reorder pixels*.

Fix: inflate the mesh AABB by 0.5 phys px when storing into
`mesh_rects` (cheap, conservative; matches polyline policy). Pin
with a comment.

References: `mod.rs:287-325`.

### 5. `compose-cache.md` is historical, lives next to active code

Per `CLAUDE.md`: "`docs/*.md` for in-flight notes". `encode-cache.md`
sits in `src/renderer/frontend/encoder/` for the same historical
reason. Both are >75 lines of "we removed this, here's why" — useful
to keep, but they clutter the module directory and aren't
discoverable from a `tree src/`.

Fix: move to `docs/cache-history/{compose,encode}.md`, leave a
one-line `// see docs/...` breadcrumb at the top of each `mod.rs`.

## Simplifications

### 6. Cull + overlap-flush sequence in `DrawRect` is order-sensitive

`mod.rs:235-253`: cull-by-scissor first, then overlap-flush, then
push. If the quad is culled, we `continue` before computing the rest
of the per-quad math — good. But the overlap-flush check happens
*before* the gradient atlas registration — if it flushes, the
gradient row still ends up in the new group's atlas. That's
correct (atlas is per-buffer, not per-group), but reading the code
top-down you have to know that. Adding a one-line comment "atlas is
buffer-scoped — flush order doesn't matter" would save the next
reader a minute.

### 7. `0..cmds.kinds.len()` index loop with paired `[i]` accesses

`mod.rs:162-164`: `for i in 0..cmds.kinds.len() { let kind =
cmds.kinds[i]; let start = cmds.starts[i]; }`. Trivially a paired
iterator: `for (&kind, &start) in cmds.kinds.iter().zip(&cmds.starts)`.
Same codegen, removes one bounds-check pair.

### 8. `if p.v_len > 0` guard on mesh AABB push but always-emit `MeshDraw`

`mod.rs:318-325`: `out.meshes.draws.push(MeshDraw { v_len = 0 ... })`
unconditionally, then guard the `mesh_rects.push`. Asymmetric with
polyline at `mod.rs:396` which `continue`s on empty. Either trust the
encoder to never emit `v_len == 0` (delete both guards) or treat
empty as drop (early-return before pushing `MeshDraw`).

### 9. Composer's five scratches need a `reset()`

`mod.rs:155-158` clears four scratch vecs at the top of `compose`;
the fifth (`polyline_scratch`) gets cleared per-cmd at `mod.rs:373`.
Adding a sixth means remembering to clear in two places. A single
`self.reset_scratch()` (or just acknowledging it in a comment block)
would prevent the next contributor from missing one.

## Smaller improvements

- `mod.rs:82-86`: doc on `any_overlap` says "inflate by 1px when AA
  fringe matters" but no caller inflates. Either do the inflation
  (mesh, see #4) or drop the guidance.
- `tests.rs:177-198` (`cull_handles_culled_text_then_quad_split`):
  comment still refers to `last_was_text` which no longer exists.
  Rename → `cull_does_not_taint_overlap_state` or similar.
- `tests.rs:531-559`: pin name says "splits group on text→quad
  transition" — with the new rule it's specifically "overlapping
  text→quad". Worth a rename to keep the contract honest.
- `mod.rs:249-253`: could collapse to a single
  `if self.overlaps_higher_kinds(quad_urect) { ... }` helper for
  symmetry with the text-side check.

## Open questions

1. **Mesh AA semantics.** Does the renderer guarantee meshes paint
   only within their vertex hull? If yes, document it (and the
   overlap-AABB invariant). If no, fix #4.
2. **Same-clip Push/Pop with no content between.** `set_clip` at
   `mod.rs:74` early-returns when the resolved clip matches, so
   the rect scratches *aren't* cleared. That's correct for a
   nested clip that resolves identically to the parent — the
   parent's accumulated overlap state should keep applying — but
   the behavior isn't pinned by a test. Worth one.
3. **Worst-case overlap cost.** `text_rects` / `mesh_rects` are
   linear-scanned on every push. With a few thousand text runs in
   one clip group the test goes quadratic. Real workloads
   (hundreds, not thousands) are fine; a node editor with 10k
   nodes inside one canvas clip is not. Threshold for adding a
   1D-sorted skip or quadtree? Probably "when a profile shows it"
   — but worth a TODO.
4. **`compose-cache.md` "bring it back if" trigger.** The doc says
   "compose >5% of frame time" — measured how, on what workload?
   The trigger should be a `cargo bench` target someone can run,
   not a wall-clock judgement.

## Shortlist (in order I'd fix)

1. **Inline `GroupBuilder` into `Composer`** (#1). Net deletion,
   call sites get readable, no behavior change.
2. **Merge `scissor_from_logical` + `urect_from_aabb`** (#2).
   Net deletion, removes a near-duplicate.
3. **Inflate `DrawMesh` AABB by 0.5px when storing into
   `mesh_rects`** (#4). One line + a comment, prevents a future
   silent paint-order bug.
4. **Move `compose-cache.md` → `docs/`** (#5). Cleans the module
   directory.
5. **Pin same-clip Push/Pop preserves overlap state** (open Q #2).
   Two-line test, catches a regression class no existing test
   covers.
