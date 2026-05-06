# `src/renderer/` review

Scope: `mod.rs`, `gpu/`, `frontend/{mod,cmd_buffer,encoder,composer}/**`. Backend was reviewed separately and is excluded. ~5.7 kLoC total, ~3.2 kLoC of which is tests.

## Architectural issues

### 1. Encode cache and compose cache duplicate ~80% of their machinery, but their guards diverge

Both caches have the same shape: per-`WidgetId` snapshot + three `LiveArena`s + `try_*` (hit) + `write_subtree` (miss-with-rewrite-or-append) + `sweep_removed`. The hot-path / slow-path probe (`encoder/cache/mod.rs:177–221` vs `composer/cache/mod.rs:160–181`) is structurally identical, with identical comments and identical compaction constants (`COMPACT_RATIO=2`, `COMPACT_FLOOR=64`).

The key divergence is silent: encoder gates the in-place rewrite on `prev.subtree_hash == subtree_hash && prev.available_q == available_q` (`encoder/cache/mod.rs:186`); composer rewrites in place whenever `same length` and updates the key fields after the fact (`composer/cache/mod.rs:161–167`). The encoder comment at `encoder/cache/mod.rs:178–185` explains why the guard is needed (kinds may swap legitimately at the same length under a different hash, e.g. TextEdit placeholder → focused caret), and there's an explicit regression test (`encoder/cache/tests.rs:207–248`). The composer has no equivalent guard and no equivalent test. Nothing in the composer's data layout makes the guard unnecessary — `Quad`/`TextRun` slots can also hold structurally different content of the same length.

Suggestion: factor the snapshot-table + hot/slow probe + sweep into a generic `SnapshotTable<Snap, Arenas>`. At minimum, port the hash guard to the composer and add a same-length-different-hash regression test.

### 2. Encode-cache bypass on damage-filtered frames is a load-bearing invariant enforced only by code path

`Encoder::encode_node` only consults the cache when `damage_filter.is_none()` (`encoder/mod.rs:126`). The doc-comment block (`encoder/mod.rs:113–124`) explains the reason: a partial-paint frame writes back lies (the snapshot wouldn't cover the full subtree). But the cache itself doesn't know about damage, so a future caller could legally call `cache.try_replay` / `cache.write_subtree` from a partial frame and silently corrupt every subsequent full frame. The contract belongs *on the cache* — either by carrying the filter into the API (and asserting `None` on write), or by funneling all writes through a single `encode` entry on `Encoder` with the filter baked in.

### 3. `cmd_buffer` payload-layout invariants are spread across three files with no compile-time link

`bump_rect_min` (`encoder/cache/mod.rs:329–351`) and `bump_exit_idx` (`encoder/cache/mod.rs:364–380`) match on `CmdKind` and assume the leading word(s) of each rect-bearing payload are `rect.min`. The pinning is split: const asserts in `encoder/cache/mod.rs:47–51` pin payload field offsets, const asserts in `cmd_buffer/mod.rs:104–109` pin `EnterSubtreePayload` size/align/offset. Adding a new rect-bearing kind silently corrupts coordinates if either set is missed.

Suggestion: encode the "rect-bearing" property as a trait or a single `match` on `CmdKind` returning `Option<RectFieldOffset>` next to the `CmdKind` enum itself, so all consumers of payload geometry route through one source of truth.

### 4. Cascade-fingerprint asymmetry creates a silent thrash hazard

The encoder cache's key is `(WidgetId, subtree_hash, available_q)`; the composer cache's key adds `cascade_fp` (`composer/mod.rs:383–405`). When parent transform/scale/snap/viewport change but the encoder subtree is still hash-stable, the encoder hits and the composer always misses on the same subtree. That's the documented design (compose-cache.md), but it means a uniformly-animated parent invalidates the *entire* compose cache while leaving the encoder cache fully warm — a ~2× compose cost with no warning. Worth a one-line comment on `Frontend::build` explaining the staircase, and ideally a counter so a perf regression is detectable.

## Simplifications

### 5. Drop `EnterSubtreePayload`'s `#[padding_struct::padding_struct]`

The struct is `#[repr(C)]` of `(WidgetId(u64), NodeHash(u64), AvailableKey(u32×?), u32)` — already explicitly 32 bytes (asserted at `cmd_buffer/mod.rs:105`). The proc macro is idempotent here. Either keep it for consistency with other `Pod` structs and *delete the const assert* (one source of truth), or delete the macro and keep the assert. Currently both pay the cost.

### 6. `SubtreeFrame` is duplicated between encoder and composer

`encoder/mod.rs:19–26` (carries `cmd_lo`, `data_lo`, `enter_patch`) and `composer/mod.rs:23–31` (carries `quads_lo`, `texts_lo`, `groups_lo`, `cascade_fp`) — same role, sibling shapes, both private, both ~7 fields, both written once and consumed once. Acceptable as-is; flag for the unified cache abstraction in (1).

### 7. `RenderBuffer::has_rounded_clip` has only one consumer

Set in `composer/mod.rs:170,193`, read by the wgpu backend to decide whether to lazy-init the stencil buffer. The same bit could be derived from `groups.iter().any(|g| g.rounded_clip.is_some())` in the backend — it walks groups anyway. Keeping the precomputed bit is fine if it's used in a hot path (it isn't — once per frame at submit), so consider dropping it for fewer cross-cutting invariants.

### 8. `gpu/mod.rs` is a 2-line shim

`pub(crate) mod buffer; pub(crate) mod quad;` — both are referenced as `crate::renderer::gpu::{buffer, quad}::…` exactly once each at the canonical use sites. The "gpu" name is also misleading: `buffer.rs` and `quad.rs` contain pure CPU data structures with no GPU handles. Consider flattening to `renderer/{render_buffer,quad}.rs` (or moving them under `renderer/frontend/`, since they ARE the frontend↔backend contract). The current placement is documented in `mod.rs:9–14` but reads as if `gpu/` should hold device code.

### 9. Composer `cascade_fingerprint`'s `parent_scissor` discriminator is overkill

`composer/mod.rs:392–400` writes a leading `0u8` or `1u8` plus four packed `u32`s. `URect::default() == URect::ZERO` so `parent_scissor.unwrap_or(URect::ZERO)` would hash unambiguously (no real scissor is `(0,0,0,0)` because that's empty content). Saves a branch and a discriminator byte; not hot, but reads cleaner.

### 10. `read_pod` API forces every caller to compute byte arithmetic for `PushClipRounded`

`composer/mod.rs:194–197` hardcodes `start + RECT_WORDS` to read the trailing `Corners`. This is the only payload split across two reads — every other variant has a single struct. Defining a `PushClipRoundedPayload { rect: Rect, radius: Corners }` next to the others removes the manual offset and removes one ad-hoc compile-time const from the composer.

## Smaller improvements

- `composer/mod.rs:194` — `const RECT_WORDS: u32 = (size_of::<Rect>() / 4) as u32;` would be cleaner as `Rect::WORDS` if `Rect` had a `pub const WORDS: usize`. Or fold into the new payload struct above.
- `cmd_buffer/mod.rs:230` — `let exit_idx = (self.kinds.len() - 1) as u32;` runs *after* `record_start` pushed the close, so the `- 1` is correct but easy to misread. Comment or extract.
- `encoder/mod.rs:200–223` — the `ClipMode::Rounded` branch unconditionally `expect`s chrome; the rounded-without-chrome state is unreachable per builder, but the panic message ("builder invariant violated") is the only thing standing between an internal API misuse and silent corruption. Consider lifting the invariant into the type — e.g. `ClipMode::Rounded(Corners)` carrying the radius — so the encoder doesn't have to consult `chrome_for(id)` at all for the mask geometry.
- `frontend/mod.rs:100–103` — `Frontend::sweep_removed` pokes the encoder's and composer's `cache` fields directly. Either expose `Encoder::sweep_removed` / `Composer::sweep_removed` for symmetry with the other pipeline subsystems, or document why this one breaks the pattern.
- `cmd_buffer/mod.rs:198–222` — `push_enter_subtree` writes `exit_idx: 0` then `..bytemuck::Zeroable::zeroed()`. With every field explicitly initialized except `_pad`, the spread fill is correct but redundant for non-pad fields. Fine as-is; flag only because it pairs with #5 above.
- `gpu/buffer.rs:14–47` — `RenderBuffer` has a hand-written `Default` because `viewport_phys_f: Vec2::ZERO` and `scale: 1.0` differ from `derive(Default)`. Both could come from `derive(Default)` if `scale` was a newtype with `Default = 1.0`, but the friction outweighs the win — leave as-is.
- `encoder/mod.rs:281` — `tree.read_extras(id).transform.filter(...)` allocates an `ElementExtras` lookup even when the field is `None`. If `read_extras` is already O(1) (side-table lookup by index), fine; worth a glance at `tree/element/mod.rs` to confirm it isn't doing a linear scan.

## Open questions

- **Should `EncodeCache` and `ComposeCache` share an abstraction?** The duplication is real, the divergence is a bug surface, but the two snapshot shapes don't compose without a generic-arena trait. Worth doing once `try_splice`/`try_replay` semantics fully stabilize (right now `try_splice` returns `bool`, `try_replay` returns `bool`, but neither commits to whether the in-place vs append distinction is part of the contract). User: how attached are you to the current per-cache test coverage when the underlying probe machinery merges?

- **Is `damage_filter.is_some()` the only state that should bypass `EncodeCache`?** Resize, theme change, and first frame are all expressed today as "nothing in the cache hits because the keys differ" rather than as explicit invalidation. That's elegant when it works, but means the *only* invariant test is the cache-key set itself. A bug there silently splices stale snapshots. Should there be a frame-level `cache_epoch` companion to `removed`?

- **Is it intentional that the composer's `cascade_fingerprint` doesn't include the encoder's `available_q`?** Composer key is `(wid, hash, avail, cascade_fp)`. `avail` is logical-px and `cascade_fp` includes `scale`/`viewport`. Logically the available-rect change is already covered by `avail` and the physical projection by `cascade_fp`, but `avail` is `AvailableKey` (quantized) while the actual `available` consumed by the layout might differ — confirm there's no path where `AvailableKey` agrees and `cascade_fp` agrees but the physical output diverges.

- **`gpu/`'s name**: confirm whether the directory is meant to hold GPU-handle types in the future (justifying the shim) or whether it's a misnomer left over from an earlier layout.

## Prioritized shortlist

1. **Composer hash-guard (#1)** — *withdrawn on second look*. The encoder's guard exists to prevent a `debug_assert_eq!` from firing on same-length-different-hash kind swaps. The composer has no such assertion and stores typed slots (`Quad`/`TextRun`/`DrawGroup`) that accept any value, then eagerly rewrites the snapshot key after the in-place copy. Adding the guard would *de-optimize* the hot path (forcing arena churn whenever cascade or hash changes at the same length) without buying correctness.
2. **Lift `ClipMode::Rounded` to carry `Corners`** — *deferred*. `ClipMode` is packed into 2 bits in `PaintAttrs`; carrying a payload requires either widening the bitfield or a separate side-table. Wants a design pass.
3. **Add `PushClipRoundedPayload`** — *done*. Single struct replaces the manual `start + RECT_WORDS` arithmetic in the composer.
4. **`EnterSubtreePayload` padding** — *done*. Kept the `padding_struct` macro (matches the project convention for `Pod` structs); dropped the redundant size/align asserts. Kept the rect-leading-field asserts and moved them next to the payload structs.
5. **Centralize payload-layout knowledge in `cmd_buffer`** — *done*. New `CmdKind::has_leading_rect()` method; const asserts on `offset_of!(payload, rect) == 0` for every rect-bearing kind live next to the payload definitions. `bump_rect_min` consumes the method instead of matching kinds itself.

Bonus: **dropped `RenderBuffer::has_rounded_clip` field (#7)** — replaced by a one-line `has_rounded_clip()` method that walks `groups` (called once per frame in `submit`). Removes a cross-cutting invariant the composer had to maintain.
