# `src/ui/` review

Scope: `mod.rs`, `cascade.rs`, `damage/`, `seen_ids.rs`, `state.rs`. Review-only — no fixes applied.

## Architectural issues

### A1. Trivial focus pass-throughs on `Ui` violate the no-trivial-accessor rule
`mod.rs:205-224` exposes four pure delegates to `self.input`:
- `focused_id() -> self.input.focused`
- `request_focus(id) { self.input.focused = id; }`
- `set_focus_policy(p) { self.input.focus_policy = p; }`
- `focus_policy() -> self.input.focus_policy`

CLAUDE.md is explicit: "If a method body is just `self.field`/`self.field = v`, delete it and make the field `pub(crate)`." But these are the **public** API, so demoting `input` to `pub` and writing `ui.input.focused = …` from app code is the wrong shape. Real fix: move `focused` and `focus_policy` from `InputState` onto `Ui` itself (they're not consumed by `InputState::on_input` — go check, you'll find they're plumbed back out via `cascades.result` in the response path), or accept these four as the "public projection" exception and document it once at the top of `mod.rs`. Currently it reads as boilerplate that violates the project's own rule.

### A2. `Pipeline` bundle is an organisational fiction
`mod.rs:23-42`. `Pipeline { text, layout, frontend }` exists to host `sweep_removed`, which is three independent calls. The doc-comment justifies it as "the rendering chain", but no method on `Pipeline` actually composes them — `end_frame` (`mod.rs:144-166`) reaches into each field individually anyway (`pipeline.layout.run`, `pipeline.frontend.build`). The grouping doesn't earn its keep; inline the three sweep calls into `end_frame` and hoist `text/layout/frontend` directly onto `Ui`. You also drop one layer of disjoint-field reborrow gymnastics.

### A3. `Damage::dirty` is test-only state on a production struct
`damage/mod.rs:60-62`, `124-157`. The field is `#[cfg(test)]`-gated, the population is `#[cfg(test)]`-gated, and there's a `#[cfg(not(test)) ] let _ = dirty;` to silence the warning. CLAUDE.md §"All test-only code lives in test modules" explicitly forbids this pattern ("creep, drift, signal a real consumer coming any day that never arrives"). The doc comment even confesses it ("Reintroduce to production when an identity-based consumer lands"). Fix: delete the field and the cfg branches; rewrite tests in `damage/tests.rs` to diff `damage.prev` snapshots before/after `compute` to reconstruct the dirty set. Tests stay pinned, production gets simpler, the cfg drift hazard goes away.

### A4. `Damage::prev` is not cleared on surface change
`damage/mod.rs:122-170`. On `surface_changed`, the function sets `prev_surface`, fills `prev` with this-frame entries, then early-returns `Full`. `prev` now contains snapshots whose `rect` was computed under the new surface but whose hash relationships to the *next* frame's geometry are arbitrary if scale/transform also changed. Today this is harmless (encoder ignores damage rect on `Full`), but the contract is muddy: `prev_surface` is the only barrier. Either clear `prev` on surface change, or document explicitly that surface-change frames seed `prev` from-scratch.

## Simplifications

### S1. `Ui::response_for` is pure delegation
`mod.rs:226-228` chains `self.input.response_for(id, &self.cascades.result)`. It's `pub(crate)` and called from widgets that already hold `&Ui`. Either inline at the call sites (widgets get to see the dependency on `cascades`, which is honest) or, if you want a façade, keep it but at least let it justify itself by hiding *all* of `input`. Currently widgets still poke `ui.input` directly elsewhere.

### S2. `SeenIds::record` + `Ui::node` split the collision protocol
`seen_ids.rs:49-70` returns `bool`; `mod.rs:236-246` checks the bool, asserts on explicit-id collisions, and calls `next_dup` on auto-id collisions. The protocol lives in two files and the caller can get it wrong. Collapse into one entry point on `SeenIds` — e.g. `fn record(&mut self, id, auto: bool) -> WidgetId` that returns the (possibly disambiguated) id, asserting internally on the explicit-collision path. `Ui::node` becomes one line.

### S3. `Damage::filter` is called from one place, inline it
`damage/mod.rs:180-189`. Only caller is `compute` (one line). Inline; the `Skip`/`Full`/`Partial` branch sits next to the threshold constant where it's already explained.

## Smaller improvements

- `damage/mod.rs:131` reads `tree.records.widget_id()` once outside the loop — good. But `tree.hashes.node[i]` and `cascade_rows[i]` are bounds-checked each iteration; the loop already asserts `n == tree.records.len()`, so it's a candidate for `iter().zip()` if perf ever matters. Don't bother today.
- `cascade.rs` (not re-read above; agent flagged): `CascadeResult` has `pub(crate)` mutable fields and a "downstream takes `&CascadeResult` and never mutates it" comment. Either accept the convention (project posture is "no external consumers") and drop the comment, or wrap mutation behind `Cascades` so the snapshot type is genuinely read-only.
- `seen_ids.rs:33` `dup: FxHashMap<WidgetId, u32>` — single counter per id is fine, but if `next_dup` is hot you could collapse to a `Vec<(WidgetId, u32)>` or even fold the disambiguator into `curr` via a sentinel. Don't unless a profile says so.
- `mod.rs:144` "Disjoint-field reborrow" comment becomes unnecessary if `Pipeline` is dissolved (A2).

## Open questions

1. Is there a real plan to bring `Damage::dirty` back as production state (the comment promises a per-node command cache / multi-rect damage)? If yes, gate the field on a `cfg(feature = "incremental-damage")` or similar — anything but `cfg(test)` — and pin the contract. If no, kill it.
2. Why does `Ui::on_input` (`mod.rs:175-177`) need `&self.cascades.result` *now* (between frames, before the next `end_frame`)? If input handling reads stale cascades from the prior frame, that's likely correct (input arrives between frames), but worth a one-line comment so the next reader doesn't try to "fix" it.
3. `request_focus(None)` and `request_focus(Some(id))` share one entry point — does the policy gate ever apply here? Doc says it doesn't ("Bypasses FocusPolicy"). Worth a debug assert that `id`, when `Some`, was actually `focusable` in the last cascade — silent acceptance of a non-focusable id will desync hit-test against focus.

## Top 5 if you say "go"

1. **Delete `Damage::dirty`** (and the cfg branches) and rewrite damage tests on `prev` diffs (A3).
2. **Dissolve `Pipeline`**: hoist `text`/`layout`/`frontend` onto `Ui`; inline the three `sweep_removed` calls (A2).
3. **Collapse the auto-id collision protocol** into `SeenIds` (S2) so `Ui::node` is one line.
4. **Fix focus accessors** — either move `focused`/`focus_policy` onto `Ui`, or carve out a single documented exception (A1).
5. **Clear `Damage::prev` on surface change** (A4) — small, but removes a latent contract trap.
