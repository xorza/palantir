# Proposal: one hash type

Status: waits for a go-ahead, because it changes the public API. Delete this file when the work
lands, and keep each lasting decision in the doc comment of the type it governs.

## Result

- `ContentHash` becomes the one 64-bit hash type, and it becomes public.
- `TabKey` and `AnimationKey` stay as separate public types, and each wraps a `ContentHash`.
- `WidgetId` stays separate, with its own `NonZeroU64`.
- `WidgetId::as_u64` and `TextShapeKey::content_hash` go away.

## Public API change

```text
+ widget::ContentHash
+     struct ContentHash: Clone, Copy, Debug, Default, DockTab (blanket), Eq, Hash, NoUninit, Ord, PartialEq, PartialOrd
~ Mesh::content_hash(&self) -> ContentHash    (was u64)
```

This is the whole difference in `.notes/API_SURFACE.md`. The signatures of `TabKey`, `AnimationKey`
and `WidgetId` do not change.

## Target shape

```rust
#[repr(transparent)]
pub struct ContentHash(u64);

impl ContentHash {
    pub(crate) fn of(value: impl Hash) -> Self;
    pub(crate) fn finish(hasher: &impl Hasher) -> Self;
    pub(crate) const fn from_u64(bits: u64) -> Self;
    pub(crate) const fn as_u64(self) -> u64;
    pub(crate) const fn xor(self, rhs: Self) -> Self;
}

pub struct TabKey(ContentHash);
pub struct AnimationKey(ContentHash);

pub struct WidgetId(NonZeroU64);

impl WidgetId {
    pub(crate) const fn as_hash(self) -> ContentHash;
}
```

## Changes by site

| Site | Now | Proposal |
|---|---|---|
| `TabKey` | `u64` from `WidgetId::from_hash(value).as_u64()` | `ContentHash::of(value)` |
| `AnimationKey` | `u64` from a local hasher | `ContentHash::of(cause)` |
| `Mesh::content_hash` | `u64`, then `ContentHash::from_u64` in `shape/mesh.rs` | Returns `ContentHash` |
| Tree hash, `scene/tree/mod.rs` | `write_u64(id.as_u64())` | `id.hash(&mut hasher)` |
| Child paint row, `paint_rect.rs` | `ContentHash::from_u64(id.as_u64())` | `id.as_hash()` |
| Sort keys: `PaneKey`, `RootOrder`, `RowKey` | `id.as_u64()` | `id.as_hash()` |
| `NodeSnapshot.parent_key` | `u64`, widget bits or plane bits | `ContentHash`: `id.as_hash()` or `ContentHash::of(plane)` |
| `CascadeInputHash` | `pub(crate) u64`, `pack(u64, bool)` | Private field, private `pack(ContentHash, bool)` |
| `TextShapeKey.text_hash`, `ShapeIdentity.text_hash` | `NonZeroU64` | `ContentHash` |
| `FaceBits` | `u16` | `NonZeroU16`, which holds the niche of `Option<TextShapeKey>` |
| `ContentHash` XOR | `impl BitXor` | `pub(crate) const fn xor` |
| `ContentHash` bytes | `Pod`, `Zeroable` | `NoUninit`, as `WidgetId` |

## Decisions

- **`TabKey` and `AnimationKey` stay separate types.** The compiler then rejects a tab key given as
  an animation key, and each type keeps the doc comment for its meaning. Each type keeps its `of`.
  Refused: `ContentHash` in their signatures, which removes the two types but makes them
  ambiguous.
- **`WidgetId` does not wrap `ContentHash`.** `Option<WidgetId>` must stay 8 B, and `IdHasher` needs
  the avalanche mix that only `WidgetId::finalize` applies. egui's `Id(NonZeroU64)` holds the same
  niche for the same reason. Refused: `WidgetId(Hash64)` over `u64`, which makes `Option<WidgetId>`
  16 B. Refused: a `NonZeroU64` base for all hashes, which adds a zero branch to each `finish` and
  removes the zero that `Default` and `xor` need. rustc's `DefPathHash(Fingerprint)` is a wrapper
  over a base type, but that base has no niche to keep.
- **`WidgetId` converts through `as_hash`, not through `From`.** A `From` impl between two public
  types is public, so it blurs the separation that this proposal keeps. `as_hash` is crate-private
  and `const`, and its name follows the `as_*` rule of the public API design.
- **`ContentHash` has no public constructor apart from `Default`.** Mesh callers compare the hash,
  and no caller needs to make one. `NoUninit` replaces `Pod` so that a caller cannot cast chosen
  bits into a hash. `Default` stays, because structs that hold a hash derive it, and its zero is the
  identity of `xor`.
- **`ContentHash` promises equality within one process.** Equal content gives an equal hash. The
  doc comment says that the value is not for storage, as `Hasher::pod_slice` already warns.
- **The name stays `ContentHash`.** `Mesh::content_hash` already uses the word, and the name gives
  the purpose. Refused: `Hash64`, rustc's name, which gives only the width.
- **`from_u64` and `as_u64` stay crate-private.** `common` is a leaf and cannot name `WidgetId`, so
  `as_hash` in `primitives` needs a crate-private constructor. After this work, production calls of
  `from_u64` are `as_hash` and `hash::str`. Production calls of `as_u64` are the gradient bucket
  index, `BrushHash`, `keep_spread` and `CascadeInputHash::pack`.
- **`hash::str` stays where it is.** It holds the reason for its seeded hasher, and its five callers
  need no change.
- **The `TextShapeKey` niche moves to `FaceBits`.** `FontWeight` is `1..=1000`, so the face bits are
  never zero and `Option<TextShapeKey>` stays 24 B. A text with hash zero keeps its true hash, and
  four sites no longer need one shared mapping. Cost: one zero check when a key is made or takes a
  new bound. Refused: `NonZeroU16::new_unchecked`, which adds `unsafe` for one comparison.
- **`parent_key` is a hash, not an enum.** `NodeSnapshot` is pinned at 40 B, and an enum of a widget
  id and a plane takes 16 B, so the pin moves to 48 B. A hash compare has the same promise as the
  other hashes that the damage walk reads. Cost: one small hash for each root visit.
- **`IdHasher` stays with `WidgetId`.** It trusts the mix of `finalize`, and `ContentHash` does not
  have that mix. A search found no map with a `ContentHash` key. egui's `nohash_hasher` maps and
  Bevy's `NoOpHash` make the same promise: the key is already a high-quality hash.

## What stays raw, and why

- **`ramp_hash` and `row_hash`:** hashbrown's `HashTable` takes `u64` hashes.
- **`AnimationSlot.hash`:** a private FNV value from a `const fn`, which never leaves the type.
- **`BrushHash.payload`:** it holds color bits or a hash, written as one whole word.
- **`WidgetId::finalize` input:** the raw output of `Hasher::finish`, private to the type.

## Out of scope

`TextureId`, `RenderOwnerId`, `TextEpoch`, `ScreenshotId`, `TabGroupId` and `WindowToken` are
counters or caller values. They are unique by construction, and a hash is only probably unique, so
they do not join this type.

## Proof in a scratch copy

The whole plan was applied to a scratch copy of the current tree, uncommitted edits included. The
repository did not change.

| Check | Result |
|---|---|
| `cargo gate-lint` | Pass |
| `cargo gate-lint-nowinit` | Pass |
| `cargo gate-ship` | Pass |
| `cargo gate-test` | Pass: 2430, 4, 26, 44 and 9 tests, no failures |
| `cargo drive surface` | Pass, with the difference shown above |
| Size of the change | 22 files, +100 and -116 lines, before new tests |

Not run yet: `gate-ship-nowinit`, `gate-test-nowinit`, `gate-doc`, `gate-doctest`, the benches, the
golden suite and the Darkroom gate. A search found one Darkroom use of the changed items, a test
that compares two `Mesh::content_hash` values. Performance was not measured. The layouts do not
change, and the one added check is in the `FaceBits` decision.

## Uncommitted work

The 22 files do not overlap the files with uncommitted edits today. The damage edits are in
`src/damage/mod.rs`, `src/damage/bench.rs`, `src/damage/region/` and `src/damage/sweep/`. This plan
changes `plane_walk.rs`, `node_snapshot.rs`, `root_order.rs` and one damage test.

## Steps

1. Change `ContentHash`: make it public, add `of`, change `BitXor` to `xor`, change `Pod` to
   `NoUninit`, and export it from `palantir::widget`.
2. Make `TabKey` and `AnimationKey` wrap `ContentHash`, and make `Mesh::content_hash` return it.
3. Replace `WidgetId::as_u64` with `as_hash`, and write ids to hashers through `Hash`.
4. Change `parent_key` and `CascadeInputHash` to `ContentHash`.
5. Change `text_hash` to `ContentHash`, change `FaceBits` to `NonZeroU16`, and delete
   `TextShapeKey::content_hash`.
6. Add the tests below, and update the doc comments of `ContentHash`, `WidgetId` and `TextShapeKey`.
7. Run `cargo drive surface` and the whole palantir gate, then update the submodule's callers in
   Darkroom if a build needs it, and run the Darkroom gate.

## Tests to add

The scratch copy only adapted existing tests. The work adds these assertions to tests nearby:

- **`ContentHash::of`:** `of(7u64)` equals a hasher fed `write_u64(7)`, and `of(7u64)` differs from
  `of(8u64)`.
- **`WidgetId::as_hash`:** the hash holds the id's exact bits, and an order by `as_hash` matches the
  order of those bits.
- **`TextShapeKey`:** a text with hash zero keeps zero as its `text_hash`, and
  `Option<TextShapeKey>` stays 24 B.
- **`FaceBits`:** weight 1 with every bound keeps its weight, slant, align and fit.
- **`parent_key`:** a root that moves to another plane does not read as unchanged. A search found
  no damage test named for this case.
- **`TabKey` and `AnimationKey`:** equal values give equal keys, and different values give different
  keys.

## Sources

- [rustc_hashes: `Hash64`](https://raw.githubusercontent.com/rust-lang/rust/master/compiler/rustc_hashes/src/lib.rs)
- [rustc: `DefPathHash`](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_span/def_id.rs.html)
- [egui: `Id`](https://raw.githubusercontent.com/emilk/egui/master/crates/egui/src/id.rs)
- [Bevy: hash types](https://raw.githubusercontent.com/bevyengine/bevy/main/crates/bevy_platform/src/hash.rs)
