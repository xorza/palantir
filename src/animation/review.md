# `src/animation/` review

Scope: production code under `src/animation/` (`mod.rs`, `animatable.rs`,
`easing.rs`, `spring.rs`, `paint.rs`), cross-referenced with consumers
(`ui/mod.rs::animate`, `widgets/theme/widget_look.rs`,
`forest/tree/paint_anims.rs`, encoder). Tests excluded.

Overall: a polished, well-reasoned module — most of what follows is
tuning/coupling smell, not breakage. Findings ordered by impact.

> **Status (applied):** shortlist items 1–4 implemented.
> 1. Duration snap floor split from the spring floor — duration now uses
>    `approx::EPS` (1e-4), spring keeps 0.01/0.1 (`spring.rs`,
>    `mod.rs::tick`). Pinned by `duration_floor_is_tighter_than_spring_floor`.
> 2. `paint.rs` merged into `forest/tree/paint_anims.rs`; `animation/` is
>    now purely the value-interpolation system.
> 3. `AnimMap::sweep_removed` now drops drained typed maps, restoring the
>    idle `Ui::animate` fast path.
> 4. `AnimSlot` hashing doc corrected; `PaintMod::is_identity` now
>    `approx_zero(alpha − 1.0)`.
>
> Item 5 (`AnimRow` dual-mode fields) deliberately left as-is — it was
> gated on `Animatable` growing more wide types, which hasn't happened.

## Architectural issues

### 1. One global settle epsilon serves three different roles across all types
`POS_EPS = 0.01` / `VEL_EPS = 0.1` (`spring.rs:17-18`) is consumed by
*three* distinct decisions, on *every* `Animatable` type:
- spring termination inside `step` (`spring.rs:76`),
- the spring-row "snap-if-close" fast path (`mod.rs:281`),
- **and the same fast path on Duration rows** (`mod.rs:281` runs before
  the `match spec`, so duration animations also snap when within
  `POS_EPS`).

The threshold was deliberately *raised* "for pixel-scale animations"
(`spring.rs:10-16`, 1e-3 → 1e-2). But the same constant now governs
`Color` animations, whose values live in 0..1 linear-RGB. `POS_EPS =
0.01` is 1% of a channel's full range; a subtle hover/press tint whose
target delta has magnitude < 0.01 will **snap instead of ease**, with no
way to opt out per-type. A tuning choice motivated by pixels silently
changed color-animation behavior.

Worse for compound types: `magnitude_squared` on a derived `Background`
sums squared components across *mixed units* — stroke width / corner
radius in pixels (hundreds²) and fill color in 0..1. A 0.005-linear
color residual riding under a settled pixel field is dwarfed and can be
snapped early; conversely the px term dominates the whole settle
decision. The threshold means something different for every compound
type.

`spring.rs:17-29`, `mod.rs:281-292`. Suggest: at minimum split the
duration snap-if-close threshold from the spring settle threshold (they
want different tolerances), and consider a per-magnitude-scale or
trait-provided settle epsilon so 0..1 color and pixel translate don't
share one floor. If kept global, document that the constant is in
"largest-component units" and that color deltas under 1% snap.

### 2. Two unrelated animation subsystems share only the module name
`animation/` houses two systems with **zero shared code**:
- value-interpolation: `Animatable` + `easing` + `spring` + `AnimMap`,
  keyed `(WidgetId, AnimSlot)`, sampled at *record* time via
  `Ui::animate` (`mod.rs`, `animatable.rs`, `easing.rs`, `spring.rs`);
- paint sampling: `PaintAnim` / `PaintMod`, sampled at *encode* time
  (`paint.rs`).

They don't share a trait, a key, a storage home, or a lifecycle. And the
paint subsystem is itself split: the enum + sampling math live in
`animation/paint.rs`, but its registry (`PaintAnims`, `PaintAnimEntry`,
`by_shape`) lives in `forest/tree/paint_anims.rs:43`. So `PaintMod` is
`pub(crate)` in `animation` purely to be read back by the encoder and the
forest registry.

This isn't wrong (it mirrors the Shape/Tree decoupling), but the grouping
is by the English word "animation," not by cohesion. `paint.rs`'s natural
home is next to its registry in `forest/tree/`. **Open question** below —
worth a deliberate call rather than drift.

## Simplifications

### 3. `AnimMap::by_type` never shrinks, permanently disabling the hot fast path
`Ui::animate`'s cheapest path is guarded by `self.anim.by_type.is_empty()`
(`ui/mod.rs:1036`) — it skips `slot.into()`, the filter, and the
TypeId-keyed probe. But `sweep_removed` only clears each typed map's
`rows`; it never removes the `TypeId` entry from `by_type`
(`mod.rs:413-420`). So the *first* animation of any `T`, ever, makes
`by_type` non-empty for the rest of the process — even after every row
sweeps away and the app goes fully idle. The fast path the comment calls
"the dominant case in static UIs" is dead the moment one hover fires.

Suggest: in `AnimMap::sweep_removed`, drop typed maps that became empty
(`self.by_type.retain(|_, t| !t.is_empty())` via an `is_empty` on
`AnyTyped`). Restores the fast path for idle frames and bounds `by_type`
to actually-live types. `mod.rs:345-420`, `ui/mod.rs:1036`.

### 4. `AnimRow` carries both spring-only and duration-only fields
`velocity` is "springs only; zero for duration rows"; `elapsed` +
`segment_start` are "duration only" (`mod.rs:119-145`). A row is always
exactly one mode, yet carries both field sets. For scalar `T` this is
free, but for heavy types (`Background` ≈ 168 B) a *duration* row pays a
dead 168 B `velocity`, and a *spring* row pays a dead 168 B
`segment_start`. Row counts are tiny (dozens), so this is a note, not a
fix — but if `Animatable` ever covers more wide types it's an easy enum
split (`enum RowState { Duration{elapsed, segment_start}, Spring{velocity} }`).
Flagging so the dead-field cost is a conscious choice.

## Smaller improvements

- `mod.rs:36` — `AnimSlot` doc claims equality/hashing "falls through to
  pointer-then-bytes." `str`'s `PartialEq`/`Hash` compare/hash *bytes*
  only (length + memcmp); there is no pointer fast-path in std's
  guarantee. The doc oversells the perf and is misleading — drop the
  "pointer-then-bytes" clause.
- `paint.rs:54-58` — `PaintMod::is_identity` is `#[allow(dead_code)]`,
  used only by tests today. The "consumed once Pulse/Marquee land"
  comment satisfies the keep-it rule, but it's a production item alive
  only for tests right now; fine to keep, worth a glance when the
  follow-up lands or doesn't.
- `paint.rs:56` — `is_identity` is `self.alpha >= 1.0`. For `BlinkOpacity`
  (alpha ∈ {0,1}) it's correct, but as a general predicate `>= 1.0`
  treats over-bright `alpha > 1.0` as "identity / pass-through," which it
  isn't. Tighten to `== 1.0` (or `approx`-eq) before non-binary variants
  arrive.
- `mod.rs:281` + `spring.rs:76` — `within_settle_eps` runs twice per
  spring tick (snap-if-close, then again at the end of `step`). Cheap,
  but the snap-if-close pre-check duplicates work `step` already does;
  could be skipped for the spring arm since `step` settles internally.
- `easing.rs:17` — `apply` re-clamps `t` to 0..1, but the only in-tree
  caller (`mod.rs:309`) already clamps. Harmless defensive double-clamp;
  fine to leave for external callers / `OutBack` safety.

## Open questions

- **Should `animation/paint.rs` move to `forest/tree/`?** Its only
  consumers are the encoder and the forest registry it's split from;
  nothing in the `Animatable`/spring value-animation world touches it.
  Keeping it under `animation/` reads as "all animation lives here," but
  the cohesion says otherwise. Move it next to `paint_anims.rs`, or is
  the umbrella grouping intentional for discoverability?
- **Is the `POS_EPS = 0.01` snap on Duration color animations intended?**
  i.e. do you actually want subtle (<1% linear-RGB) theme transitions to
  skip animation entirely, or is that an accidental side effect of the
  pixel-scale bump leaking onto the color path (finding #1)?
- **`spring::step`'s `T::lerp(cur, target, 0.0)` snap-field trick**
  (`spring.rs:74`) is a hidden contract with the `#[derive(Animatable)]`
  macro's snap-field semantics. Duration gets this "for free" via its
  hot-path lerp; spring needs the explicit no-op lerp. Is this asymmetry
  documented anywhere the derive macro's authors would see it? It's a
  cross-file invariant that a future lerp change could silently break.

## Prioritized shortlist (if you say "go")

1. **Split the duration snap-if-close threshold from the spring settle
   threshold** (#1) — the one finding with user-visible behavior (color
   animations snapping under a pixel-tuned floor).
2. **Prune empty typed maps in `AnimMap::sweep_removed`** (#3) — restores
   the idle fast path with a one-line retain + `is_empty` on `AnyTyped`.
3. **Decide `paint.rs`'s home** (#2 / open Q) — move next to its registry
   or document the umbrella grouping on purpose.
4. **Fix the `AnimSlot` hashing doc and `is_identity` `>= 1.0`** — two
   quick correctness/clarity nits.
5. Revisit `AnimRow`'s dual-mode fields (#4) only if `Animatable` grows
   more wide types.
