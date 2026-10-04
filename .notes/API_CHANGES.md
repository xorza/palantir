# Public API changes

This file holds every fix from `.notes/REVIEW.md`, `.notes/TEST_REVIEW.md` and `.notes/ISSUES.md`
that adds, renames, removes or re-signs an exported item. Under AGENTS.md, each one needs a
go-ahead before work starts. `.notes/REDESIGN.md` holds the rest, and names an interim step where
one exists.

When an item is done or rejected, delete it.

Each item gives the findings it closes, the options, a recommendation, and what it touches.
The recommendations are starting positions. Before you settle a signature, read its neighbours
(the sibling setters, the trait it joins, the `Response` its peers return) and match their names,
argument shapes and order, as AGENTS.md requires.

Line numbers are against `50b34a49`.

---

## A19. Features the docs imply: IME and focus traversal

**Blocked** on `QUESTIONS.md` Q2.

**Findings.** REVIEW "Stale input docs": docs mention IME text, but winit `Ime` is never enabled
or translated, and there is no Tab focus traversal. REDESIGN D16 fixes the docs.

**Recommendation.** Treat each as a feature with its own design: IME needs `InputEvent` variants
for preedit and commit; focus traversal needs a focus order (the `KeyClass::Focus` class it reads
exists since the key-class split). Out of scope for the defect work.

---

# Surface review (2026-10-04)

The items below come from a review of the whole exported surface, listed in `API_SURFACE.md`
(rustdoc JSON of `59b93e30`). Each item names what reads inconsistent, asymmetric, or
non-canonical, and what to do about it. Where a finding extends an earlier item, the earlier item
took it: A7, A9, A10, A11, A15, A18 and A19. The plan at the end of the file orders all
items, old and new.


# Implementation plan

## Phase 0 — decisions before any code

All seven are decided. Each item named here carries its decision in its own text.

| Decision | Item | Recommendation | Decided (2026-10-04) |
|---|---|---|---|
| Release behaviour of a per-frame contract violation | A50 | the kind's neutral, with a debug panic | **a release panic**, with an `is_*` predicate per kind |
| A stale selection index | A50 | clamp to the last option | **clamp to the last option for display; the bound index is not written back** |
| The attach-to-trigger verb and argument order | A14, A51, A23 | one verb for `Tooltip` and `ContextMenu`, the snapshot first and the text second | **`on(&snapshot)`**, text second, no `ui` in a constructor |
| Page identity in `TabbedView` | A12 | an optional key function, index keys by default | **`.keyed(\|page\| impl Hash)`**, index keys by default |
| Fallible render target conversion | A16 | reject: no application chooses a target format at run time | **a checked `RenderTarget::new` / `TargetFormat::new`**; the `From` impls go |
| GPU tests in every `cargo test` | A17 | option 1 | **option 1**; the test server needs lavapipe installed |
| How the chrome setters are shared | A39 | a `Chrome` trait | **no trait**: inherent `default_background` on all eight, kept in step by one test |

## Phase 1 — foundations the later phases build on

1. Done: the `domain` module (A50 mechanism, A36).
2. Done: the `const` sweep (A24).
3. Done: flag sets (A32).
4. Done: no strum on public types (A6).
5. Done: the chainer, wrapper, text and validation rules are in AGENTS.md, and
   `scripts/api_surface.py` regenerates `API_SURFACE.md`.

## Phase 2 — renames and removals

Each line is one commit; none depends on another inside the phase.

1. Done: names (A28, A29, A30, A42, A45, A46).
2. Done: chainers (A47, A44).
3. Done: removals (A8, A13, A34, A35, A38, A40, A43, A48, A49).
4. Done: argument types (A31, A33 with A9, A37).
5. Done: test features (A17).

## Phase 3 — structural API

1. Done: one value response (A27).
2. Done: wrappers (A39).
3. Done: overlays (A14 with A51, A41, A23).
4. Done: dock and tabs (A11, A12).
5. Done: colour button (A15).

## Phase 4 — the validation rollout (A50 rules 2–4)

Done. The rules are in AGENTS.md, the kinds in `widget::domain`, and
`widgets::tests::coercing_inputs_at_their_worst_paint_no_nan` holds the frame property.

## Phase 5 — goldens

Done (A7, A25, A26). The visual suite records its adapter; it does not assert `orphans` yet,
because no one test names every golden the suite draws.

## Phase 6 — features

1. Done: wheel sense per axis (A22).
2. Done: keyboard on toggles and ranges (A20).
3. **A19** IME and focus traversal, each its own design. Waits on `QUESTIONS.md` Q2.
4. Done: the `meta` modifier (A10).

## Order at a glance

Phase 0 decides; phase 1 must land first; phases 2 and 3 may interleave, except where a step names
a predecessor; phase 4 needs phase 1 step 1 and phase 3 step 1 (the index coercion touches
`ComboBox`, which A27 changes); phase 5 and phase 6 are independent of each other.
