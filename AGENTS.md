# Palantir

A Rust GUI crate: an **immediate-mode authoring API**, **WPF-contract two-pass layout** (Hug
items shrink as CSS 2.1 auto table columns do, Fill items grow as WPF star tracks do), **wgpu
rendering**.

## Posture

- **Rewrite freely, except the public API.** No deprecation shims, compat aliases or migration
  helpers; the public API waits for a go-ahead (**Public API**).
- **Per-frame allocation is a regression.** After warmup a frame allocates nothing.
- **Micro-optimize freely**; a structural change with no workload that needs it waits, with a
  note.
- **One behavior in every build and on every platform**, unless the difference is speed alone or
  the system decides it: a person's settings, what a device can hold. Refused: a diagnostic on in
  debug alone.
- **A second pass is the last resort**: settle the state before it records, or in the pass that
  reads it. A relayout, a second record pass or another frame stays only where it is the
  idiomatic answer, with a comment at the site.
- **Docs are starting positions**, this file included. When one contradicts the user's intent or
  the code, flag the conflict and ask.

## Architecture

Each frame rebuilds the tree and runs **record → layout → cascade → damage → encode + compose →
paint**, one top-level module per step. Color is linear-RGB f32 on the CPU; the GPU encodes sRGB
at the swapchain write. A top-level module imports only modules to its left
(`internals::module_order` tests it):

```text
common → primitives → {animation, text, display, window, clipboard, image_registry,
                       diagnostics}
       → shape → icons → scene → layout → cascade → {damage, input}
       → renderer → gpu → theme → ui → widget_core → widgets → driver → host
```

A subsystem's decisions sit in an `AGENTS.md` beside it; read it before changing the subsystem:

| file | covers |
| --- | --- |
| `src/gpu/AGENTS.md` | rendering, `renderer` included: shapes on the GPU, the rounded clip, GPU memory, the swapchain, device loss |
| `src/shape/AGENTS.md` | shapes and their paints, effects |
| `src/widgets/AGENTS.md` | widgets |
| `src/input/AGENTS.md` | chords, labels, the wheel, activation |
| `src/theme/AGENTS.md` | themes |
| `src/ui/AGENTS.md` | reduced motion, transparency, power and frame rate |
| `src/host/AGENTS.md` | hosts and the harness |
| `palantir-showcase/benches/AGENTS.md` | benches, measuring, profiling |

## Platforms

- **Vulkan, Metal and GL; no Direct3D 12.** A machine with neither Vulkan nor GL compute gets a
  refusal that names why; CI runs Mesa's lavapipe on Windows, as
  [wgpu's CI](https://github.com/gfx-rs/wgpu/blob/trunk/.github/actions/install-mesa/action.yml)
  does. Refused: Direct3D 12, whose FXC compiler frees cached blobs across threads
  ([gfx-rs/wgpu#10547](https://github.com/gfx-rs/wgpu/issues/10547)), a lock around every pipeline
  build on every backend. Cost: no Direct3D fallback for old Intel GPUs and driverless VMs.
- **A transcendental float function goes through `primitives::math::libm_ext`**, as
  `clippy.toml` enforces, so a result has the same bits on every system, as `bevy_math::ops`
  routes them.

## Public API

- **Never change it on your own.** Adding, renaming, removing or re-signing an exported item waits
  for a go-ahead: propose it, then stop.
- **No caller is no reason to remove.** An item goes when it is wrong for the surface: a second
  spelling of the canonical one, obsolete, unidiomatic or ambiguous.
- **Match its neighbors**: read what new surface will sit beside and match its names, argument
  shapes and order.
- **`palantir::widget` holds what only a widget's author names**: an item sits there only when no
  root-exported method takes or returns it, so the shapes and paints `Ui::add_shape` takes are at
  the root. A trait that only bounds a root method's parameter (`Animatable` on `Ui::animate`)
  stays in `widget`, as an app names it only to implement it, and so does what only a `widget`
  type's methods hand out. `widget::domain` exports the kinds a `# Panics` doc links, item by
  item.
- **An exported item that breaks a rule below is a bug.** `cargo drive surface` writes the surface
  to `.notes/API_SURFACE.md`, which CI compares with the tree
  (`cargo install --locked --git https://github.com/xorza/drive`).

### Kinds of type

- **Builder**, consumed by `show`, `build` or `Ui::add_shape`: setters are bare, take and return
  `Self`.
- **Typestate builder**: a builder whose last type parameter names a mode that changes what
  `show` returns or which constructors and methods exist, its common mode the default, as
  `Menu<'a, O = TriggerOwned>`: the setters sit in one `impl` over every mode. Refused: a mode
  checked at run time, whose wrong branch is an `unreachable!`.
- **Value**, stored, passed or compared: a chaining setter is `with_*`, and one without `self`
  constructs, as `Vec::with_capacity`; a shorthand with no argument keeps its adjective
  (`TextStyle::bold`).
- **Plain data**: public fields, no invariant, checked where it enters.
- **Checked value**: private fields behind a checked constructor; a newtype unwraps through
  `get()`.
- **Flag set**, `flag_set!`: named flags, `NONE`, `ALL`, set operations, never its bits.
- **Sealed trait**: a public marker over a crate-private supertrait.
- **Error**, `*Error`: an enum of the cases a caller tells apart; a foreign cause as `source`,
  never also in `Display`
  ([error-handling project group](https://blog.rust-lang.org/inside-rust/2021/07/01/What-the-error-handling-project-group-is-working-towards.html)).
  A one-case error is a struct named for what failed, as std's `ParseIntError` and `RecvError`.
  A case that wraps another error is transparent where it adds no context, its `Display` the
  inner one's and its `source` the inner one's `source`, as `PlayError::Locate` is; one that
  adds context names its own failure and returns the inner error from `source`. A test walks
  every exported chain and holds that no message prints twice.

### Names

- **Spelling**: American, whole words. Abbreviations: the fixed short forms, and `id`, `x` `y`
  `w` `h`, `min` `max`, `dt`, `bbox`, `eps`, `inf`, `rgb` `rgba` `srgb` `hsv` `okhsv` `oklab`
  `hex`, `ctrl` `alt` `mods`, `vsync`, `gpu` `cpu`, `svg`, `fmt`, `config`, `spec`, `stats`. A
  setter's one argument may be a letter its type names (`padding(p)`), `px` for a length; a `bool`
  is named for the setting (`editable(editable)`).
- **Fixed short forms**: `idx`, `pos`, `ctx`/`Ctx`, `dir`, `bg`, `col`, `exec`, `desc`, `cur`,
  `prev`, plurals with `s`; prose, strings and test names keep whole words. A foreign name keeps
  its own.
- **A shape is geometry, a paint what covers it.** *Shape*: a rect, circle, arc, triangle, line,
  curve, polyline, text or mesh laid out in a rect. *Paint*: `Paint` for a box, `Stroke` for a
  line, a color for text, a tint for a mesh, `ImagePaint`, `IconPaint`, `EffectPaint`. *Fill* is
  a box's brush; *brush* a color or a gradient; *border* a box's edge, inside its rect; *stroke* a
  line's paint, centered on its path; *tint* multiplies an image, an icon or a mesh; *fit* is how a
  point shape, an icon or an image fills its rect.
- **`Chrome` is a node's box**, its corners and its `Paint`, a node's box and a modal's scrim alike,
  as Flutter's `BoxDecoration`; `with_fill`, `with_border` and the rest are sugar over `Paint`'s.
  `bg` is a background color alone. Refused: `background` for a `Chrome`, which holds more than a
  fill; "chrome" for a widget's scaffolding; a scrim type beside it.
- **What is behind**: *backdrop* is the pixels painted under a shape, which a backdrop effect
  reads; *material* a backdrop effect a box wears under its fill; *barrier* what stands between an
  overlay and the layers below. Refused: "pane" for any of them, the dock's word.
- **Constructors**: `new` first; a named one names the kind (`Panel::hstack`); `from_*` converts;
  `on(&snapshot)` attaches an overlay, `for_id(id)` by id. No widget constructor takes `ui`.
- **Setters** name the setting (`padding`, not `set_padding`). A boolean takes a `bool`; more
  values an enum, with a shorthand for a common one (`visibility` / `hidden`). `default_*` is a
  fallback, `start_*` a first-frame state; placement alone is a preposition (`at`, `anchor`).
- **Getters** are the noun, never `get_*`; `Ui` state pairs `x()` / `set_x(v)`; an action is a
  verb. A read of what the pointer or the modifiers hold wakes the frame its change lands in, and
  has a `peek_*` twin that does not; no other read has one.
- **Conversions** as std: `as_*` views, `to_*` computes, `from_*` goes back.
- **Predicates**: `is_*`, `has_*`, `can_*`; a third-person verb for a relation (`contains`).
- **Participles** for this frame's edges and derived values (`clicked`, `hovered`, `scaled_by`);
  a predicate where there is none (`Drag::is_live`). Algebra is a verb, as std names it (`union`,
  `clamp_to`).
- **Ids**: derived ids are `*_id` associated functions; a part's id is `id.with("part")`.
- **Sentinels**: `NONE` holds nothing, `ZERO` is geometric, `TRANSPARENT` a color, `ONE` or
  `IDENTITY` multiplicative, `ALL` a full set, `EMPTY` empty text.
- **Units**: lengths are logical pixels with no unit in the name, device pixels say `physical`;
  angles are radians, a fraction of a circle a `turn`, time a `Duration`, other units named in full
  (`refresh_millihertz`). A multiplier is a `factor`, a share a `ratio` or `fraction`. A color is linear `RgbaF32`; `SrgbaU8` is bytes.
- **Lines and corners**: a line's `width`, a bar's `thickness`, one `radius`, four `Corners`.

### Arguments

- **Concrete types**, so a setter can be `const`; `impl Into<T>` only for shorthand a call site
  wants: numbers and tuples for geometry, a color for `Brush`, text.
- **Required input goes in the constructor**, optional input through setters.
- **Text** a widget reads during the frame takes `impl Into<TextInput<'a>>`.
- **Many results** from a per-frame call go into `&mut Vec<T>` or a borrowing iterator.

### Input validation

Every public input meets two questions, as WPF asks them: *validation* of the value alone,
*coercion* against its context. The kinds are in `primitives::math::domain`.

1. **Coercion is total, silent and documented**: a list shrinks, a min above its max raises the
   max, as in CSS and WPF.
2. **Validation follows the source**: per-frame authoring takes a release `assert!` with
   `#[track_caller]`, an exception to `debug_assert!` on hot paths, as a wrong value never draws
   quietly; cold configuration a release `assert!`; data from outside the program an `Option` for
   one rule, a `Result` with an error enum for several. A rule a type can hold needs no assert:
   a count of at least one is a `NonZeroU16` or `NonZeroU32`, and a rate that may be unknown an
   `Option<NonZeroU32>`, so zero cannot be passed
   ([C-VALIDATE](https://rust-lang.github.io/api-guidelines/dependability.html#c-validate):
   static enforcement first).
3. **Plain data stays plain**, checked where it enters a widget, a shape or a node.
4. **A theme file is checked on load** by the same predicates as a call site.
5. **An index into a list that changes is coerced**; a handle the crate minted that names nothing
   panics.

Every numeric parameter's doc names its kind; every setter that can panic says so under
`# Panics`.

## Verification

The gate, from the root, each an alias in `.cargo/config.toml`, `gate-doc` beside the rest:

```
cargo fmt --all && { cargo gate-doc & cargo gate-lint && cargo gate-lint-nowinit && cargo gate-ship && cargo gate-ship-nowinit && cargo gate-test && cargo gate-test-nowinit && cargo gate-doctest && wait $!; }
```

`gate-doc` runs rustdoc on one core for most of a minute, so it starts after `fmt` and runs beside
the other lanes, in a build directory of its own, as cargo locks one for each command; the rest keep
one, as each already fills the cores. `wait $!` makes the chain's status `gate-doc`'s too; a chain
that fails before it leaves `gate-doc` to finish and print alone. Refused: a driver that schedules
the lanes, a program in each repository for one parallel lane; each lane in a directory of its own,
which competes for the cores it saves and builds the dependencies once more each.

**Run it before a turn that changed code ends, and before each push**: master takes direct
pushes, and CI runs the same lanes on three OSes. A filtered `cargo gate-test <name>` is the
cheapest check of an edit. Run the aliases, not cargo with flags of your own, which builds
another copy of the crates.

- **A public API change** runs `cargo drive surface` and commits what it writes.
- **A rendering change** runs `cargo visual [filter]`. Goldens are local to an adapter: run the
  unchanged tree first and rewrite stale ones with `UPDATE_GOLDEN=1`; a failure leaves its images
  in `palantir-showcase/tests/visual/output/<name>/`.
- **A change a user can see** ends with `cargo shot SCRIPT`, `--against DIR` marking what moved;
  goldens are a test, never a viewer.
- **A bench change** runs `cargo bench-once`; read `palantir-showcase/benches/AGENTS.md` before
  measuring.
- **CI runs on [`xorza/rust-ci`](https://github.com/xorza/rust-ci)**, x64 at x86-64-v3 as a local
  gate builds. Refused: a `RUSTFLAGS` env, which replaces the config's flags.
- **Every test needs a GPU adapter**; a headless machine a software Vulkan driver (lavapipe).
- **A slow first Metal call can be the build folder**, which Metal lists: `cargo clean`.
- **`--all-features` is clippy's alone**, as `profile-with-tracy` opens a socket in tests.

## Non-shipping code

`internals` is the one feature for tests and benches, on through the self dev-dependency.

- **A reach-in to private items** sits at the end of its file, gated `#[cfg(test)]` or
  `#[cfg(feature = "internals")]` by its callers, never `any(…)`.
- **`crate::internals`** holds the shared fixtures, **`bench.rs`** sits beside the code it
  measures, and **`crate::golden`** compares images under `feature = "golden"`. The test and
  bench GPUs sit in `gpu`, which `clippy.toml` keeps the wgpu types in; a process holds one test
  GPU instance (`gpu::test_gpu`).
- **Items code outside the crate calls are `pub`**; the rest are `pub(crate)`.
- **The showcase is a crate**, `palantir-showcase`, as egui's
  [`egui_demo_lib`](https://docs.rs/crate/egui_demo_lib/0.34.0/source/README.md): it reaches the
  public API alone and holds the benches and the integration tests; the nowinit lanes leave it out.
  Refused: the tour in `src` behind a feature, which reaches crate privates.
- **The frame benches run over the showcase's first page**, so a number comes from a screen a
  person opens; the page holds no bench code.
- **One oracle per property**, over every column it covers, and **one fixture per costly
  resource**, built once per test. No real sleep where the code takes a clock or a waker.
