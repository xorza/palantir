# Coding style

Generic Rust coding conventions for this project. Palantir-specific rules (paint predicates, `WidgetId` semantics, `padding_struct` for `Pod`, layout-pinning tests) live in `CLAUDE.md`.

## Code style

- **Comments:** none except non-obvious _why_. Code is short and self-explanatory; keep it that way. **Be terse.** One short line is the target — never multi-paragraph essays, never narration of what the code does, never "this used to…/we changed it because…" history. If a comment can't fit in one line and still earn its place, delete it.
- **Asserts:** default to release `assert!` for invariants, not `debug_assert!` — `debug_assert!` is stripped in release and hides logic bugs in the build users actually run. Reserve it for checks too expensive for release (e.g. O(n) inside a hot loop), and call out the tradeoff.
- **Edition 2024.** Dependencies pinned to `*` for now (lockfile pins actual versions) — fine for prototype, pin before publishing.
- **Visibility:** default to narrowest; demote `pub` → `pub(crate)` → private whenever nothing outside uses the item. `pub(crate)` on fields is fine — invariants live in the mutating methods, not in encapsulation theater. No `pub(in path)` / `pub(super)` — exotic noise; use `pub(crate)` for any cross-module access.
- **No trivial accessors — prefer direct field access.** If a method body is just `self.field` / `&self.field` / `self.field = v`, or a one-hop call into a built-in collection method (`self.foo.len()`, `self.foo.is_empty()`, `self.foo.contains_key(k)`), delete it and make the field `pub(crate)`. Same for the inner crate boundary: another module reaching for `cache.snapshots.len()` is fine — don't wrap it in `cache.snapshot_count()`. Inline accessors are fine when they do real work (computation, invariant enforcement, returning a derived view).
- **No tuple returns.** Give a named result struct next to the function. `Option`/`Result` excepted.
- **No inline `crate::foo::bar::Type` paths** in expressions or patterns. Add a `use` at the top — surface dependencies in the imports block, don't bury them.
- **No re-exports inside the crate.** Only `lib.rs` `pub use`s items to define the published surface. Intermediate `mod.rs` files don't re-export — make submodules `pub(crate)` and import via the canonical path (`use crate::primitives::color::Color`, not `use crate::primitives::Color`). One canonical path per item.

## Tests

- **Test/bench helpers live in gated mods at the end of the production file they reach into.** Use `#[cfg(any(test, feature = "internals"))] pub(crate) mod test_support { ... }` for items benches/integration tests need (the `internals` feature is set up in `lib.rs`); plain `#[cfg(test)]` for in-tree-only helpers. Colocate the helper with the type whose privates it reads — `text_shaper_measure_calls` lives in `src/text/mod.rs`, `damage_rect_count` in `src/ui/damage/mod.rs`, etc. No big `support::internals` aggregator that re-exports everything; one canonical path per item, callers `use crate::foo::bar::test_support::helper`. Production types stay clean (no `#[allow(dead_code)] pub(crate) fn` debug accessors hanging off them). `support/testing.rs` is reserved for genuinely cross-module fixtures (e.g. helpers that build a `Panel` _and_ drive a frame) that have no single natural home.
- **Prefer extending existing tests over adding atomic ones.** When pinning a new invariant, look for a nearby test exercising the same fixture or feature and add the assertion there. Combine related axes into table-driven sweeps (one fixture, multiple cases) instead of one test per case. Refactors that touch a feature then update one or two tests, not a dozen — fewer pin-points to migrate, less duplicated setup, the same coverage. Split into a new test only when a clean fixture for the new behavior would dominate the existing test, or when the failure mode is different enough that one assertion message wouldn't be useful.
- **Split fat-test files** into `foo/{mod.rs, tests.rs}` when tests dominate (>40% or >150 lines).

## Mechanical refactoring (large-scale renames, signature changes)

**`sd` / `perl -i` are the wrong tools at this scale.** Regex-based find/replace has no AST awareness, so it bites in predictable ways: `click_at\(` also matches `secondary_click_at(`; `run_at\((\w+),` re-matches its own output and produces `ui.X.run_at(Y)` on the second pass; multi-line `use {a,b,c};` blocks don't split cleanly. Reserve `sd` for prose, config files, and renames where the symbol is genuinely unique. For Rust code use AST-aware tools.

### Tool stack

- **`rust-analyzer ssr`** — structural search/replace via AST placeholders. Pattern `$path::foo($ui, $rest) ==>> $ui.foo($rest)` anchors on path / call nodes, so free-fn-to-method conversions can't accidentally match a method call of the same name, and the rewritten form is structurally distinct from the input (idempotent by construction). Best for signature rewrites and call-site rewrites.
- **`ast-grep`** — tree-sitter-based; YAML rule files with `inside` / `has` / `not` constraints. Strong Rust catalog. Best for AST-shape rewrites that ssr's placeholder grammar can't express — splitting `use foo::{a, b, c};` into N lines, restructuring nested expressions, applying rewrites only inside specific scopes.
- **`rerast`** — type-aware Rust pattern rules. Less maintained but works where ssr / ast-grep can't see types.
- **`cargo clippy --fix --all-targets --allow-dirty --allow-no-vcs`** — aftermath cleanup: unused imports, redundant `mut`, needless borrows. Run after every mechanical batch.
- **`cargo fix --broken-code`** — applies machine-applicable rustc suggestions; pairs with a lint that flags the old pattern.

### Workflow

Phase strictly, never interleave:

1. File moves (`git mv` for tracked files so rename history survives).
2. Signature rewrites (free fn → method, param reordering, type changes).
3. Call-site rewrites.
4. Import fixups.
5. `cargo clippy --fix` for unused-import cleanup.
6. `cargo fmt --all`.

### Rules to write idempotent rewrites

A rule is idempotent when its output can't match its input pattern. Examples:

- `foo($ui, $rest) ==>> $ui.foo($rest)` — output is a method call (different AST node from a free fn call); cannot re-match.
- `foo($a) ==>> bar($a)` — safe if `bar` doesn't appear elsewhere.
- `foo($a, $b) ==>> foo($b, $a)` — **not idempotent**; running twice un-does it. Add a sentinel marker or do it as a one-shot.

Always dry-run with `--debug-query` / preview output before applying. Both ast-grep and ssr support this.

### When to use what

- **Symbol rename inside one crate, type-driven** → IDE rename via rust-analyzer (`textDocument/rename` LSP request, scriptable). Most precise.
- **Free fn ↔ method, parameter reordering, deprecating an API** → `rust-analyzer ssr`.
- **Import-block restructuring, AST-shape rewrites, cross-language** → `ast-grep`.
- **Module path renames** (`crate::foo::bar` → `crate::baz::bar`) → `ast-grep` (anchored on `scoped_identifier`) or rust-analyzer's "Move module" refactor.
- **Cleanup leftovers** (unused imports, dead allows, format) → `cargo clippy --fix` + `cargo fmt`.
- **Prose, doc comments, TOML, shell scripts** → `sd` is fine; AST tools are overkill.
