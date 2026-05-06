# `IntoWidgetId` trait

Replace `with_id(impl Hash)` / `WidgetId::from_hash(impl Hash)` with a dedicated `IntoWidgetId` trait. Status: design, not yet implemented.

## Motivation

Today there are two hash spaces:

- **Auto ids** (`WidgetId::auto_stable`) — FNV-1a `const fn` over `(file, line, column)`. Folds to a `u64` literal at every `Foo::new()` call site. One `mov`.
- **Explicit ids** (`with_id(impl Hash)`) — FxHasher at runtime. State machine, doesn't fold.

The `widget_id.rs` doc comment already calls this a "small smell." Unifying the two paths under FNV-1a:

- Lets explicit string-literal keys const-fold the same way auto ids do (`with_id("root")` → one `mov`).
- Removes the two-hash-space caveat.
- Makes `WidgetId` pass through identity instead of getting re-hashed.
- Tightens the surface — `Hash` accepts `()`, `Option<()>`, anything; the trait makes "valid widget keys" deliberate.

## Shape

```rust
pub trait IntoWidgetId {
    fn into_widget_id(self) -> WidgetId;
}

impl IntoWidgetId for WidgetId            // identity
impl IntoWidgetId for &'static str        // FNV const-fold
impl IntoWidgetId for &str                // FNV runtime
impl IntoWidgetId for u32 / u64 / usize   // FNV fold
impl<A, B> IntoWidgetId for (A, B)        // .mix() chain
impl<A, B, C> IntoWidgetId for (A, B, C)
impl<A, B, C, D> IntoWidgetId for (A, B, C, D)
```

`WidgetId` gains an inherent `const fn mix(self, other: Self) -> Self` (FNV xor+imul) used by tuple impls and the existing `with` helper.

## Const-fold story

Trait methods can't be `const fn` on stable (needs `#![feature(const_trait_impl)]`). Doesn't matter — LLVM folds at runtime call sites because:

1. Bodies are small + `#[inline]`.
2. No hasher state machine, just xor+imul.
3. Inputs are literals.

`with_id(("row", i))` lowers to ~3 instructions: `mov rax, <const "row">; xor rax, rdi; imul rax, FNV_PRIME`.

For const contexts (e.g. `const FOO: WidgetId = ...`), bypass the trait and call inherent constructors:

```rust
const ROW_BASE: WidgetId = WidgetId::from_static_str("row");
```

## Decisions

- **Integer impls fold via FNV**, not identity. `with_id(0)` and `with_id(WidgetId(0))` should not collide; identity-on-`WidgetId` is the only pass-through.
- **Tuples cap at arity 4.** Real-world keys are `("section", "row", index)` — 3 covers 99%, 4 is headroom.
- **`with` (parent → child id) routes through the same FNV `mix`.** No third hash space.
- **`from_hash` is deleted, not deprecated.** Project posture is break-things-freely; tests using `WidgetId::from_hash("a")` migrate to `"a".into_widget_id()`.
- **`WidgetId: Hash` stays** — it's used as a `HashMap` key in `StateMap` and the caches. That's the derive-`Hash` path, unaffected.

## Migration

- `with_id(key: impl Hash)` → `with_id(key: impl IntoWidgetId)`.
- `WidgetId::from_hash(x)` → `x.into_widget_id()`.
- Test fixtures (`src/ui/damage/tests.rs`, `src/input/tests.rs`, `src/tree/tests.rs`) swap call sites — same FNV function on both sides keeps fixtures stable.
- Production keys today are `&str` literals and small tuples; no caller hits the boilerplate cap.

## Tradeoffs accepted

- ~50 lines of trait impls vs. free `Hash` blanket.
- `String` / `&str` (non-static) keys still hash at runtime — FNV is leaner than FxHasher but not free.
- Arbitrary user types (`Vec<u8>`, custom enums) no longer "just work" as keys; caller hashes explicitly to a `u64` and passes that. In practice no current call site does this.

## Open

- Bench `with_id("root")` before/after to confirm the fold actually fires (LLVM is usually right but worth a `cargo asm` check).
- Decide whether `auto_stable`'s `(file, line, column)` triple stays separate from the explicit-id namespace, or whether collisions are now acceptable since the disambiguation counter in `Ui::node` handles them anyway.
