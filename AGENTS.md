# Palantir

A Rust GUI crate: an **immediate-mode authoring API**, **WPF-contract
two-pass layout** (Hug items shrink as CSS 2.1 auto table columns do, Fill
items grow as WPF star tracks do), **wgpu rendering**.

## Posture

- **Rewrite freely, except the public API.** Big-bang renames and migrations
  take no deprecation shims, compat aliases, feature flags or migration
  helpers; the public API waits for a go-ahead (**Public API**).
- **Per-frame allocation is a regression.** After warmup a frame allocates
  nothing: push onto retained scratch, never rebuild a map per frame.
- **Micro-optimize freely** — struct packing, `const fn`, scratch reuse, cache
  layout. A _structural_ change with no workload that needs it is too early:
  shelve it with a note.
- **Docs are starting positions**, this file included. When one contradicts
  the user's intent or the code, flag the conflict and ask.

## Architecture

Each frame rebuilds the tree and runs **record → layout (measure, arrange) → cascade → damage →
encode + compose → paint**, one top-level module per step. Color is linear-RGB f32 on the CPU;
the GPU encodes sRGB at the swapchain write.

A top-level module imports only modules to its left:

```text
common → primitives → {animation, text, input, display, window, image_registry, diagnostics}
       → shape → icons → scene → layout → cascade → {damage, input_state}
       → renderer ⇄ gpu → theme → ui ⇄ app → widget_core → widgets → driver → host
```

- **`theme` is plain look data**: it knows no `Widget` or `Ui`, and `Widget::wear` applies a look.
- **`ui` records through `Node` and shapes**, never through a widget, so the recorder sits below
  the authoring layer.
- **`gpu` ⇄ `renderer` stays a cycle.** `GpuPaint` names the wgpu types `clippy.toml` keeps in
  `gpu`; moving it into `gpu` would make `ui` depend on `gpu`.
- **`app` ⇄ `ui` stays a cycle**: `App` is the frame cycle's callback contract.
- **A bench may import modules to its right**, so a whole-frame case sits beside the code it
  measures. Refused: a `src/bench/` away from that code.

## Swapchain

Facts and sources: `.notes/research/2026-10-29-swapchain.md`.

- **No frame reads the last frame from the swapchain.** wgpu hands out each image
  uninitialized, so a frame that keeps pixels paints through the backbuffer.
- **Never sample a swapchain image.** Metal, DX12 and GL offer no `TEXTURE_BINDING`; a pass that
  samples the frame samples the backbuffer. Refused: a Vulkan-only sampled path the m2 cannot
  run.
- **Ask only for usages the surface offers.** `COPY_DST` and `COPY_SRC` are optional, and each
  path that needs one checks it; a usage the surface lacks fails `configure`.
- **Nothing is keyed by an acquired image**, as wgpu does not stop a stale view after a
  reconfigure.
- **`configure` runs only on a change**, as it waits for the device to idle.

## Device loss

Facts and sources: `.notes/research/2026-10-30-device-loss.md`.

- **Recover, do not panic.** A loss opens a new device and rebuilds every GPU object; the UI
  state stays. Refused: a panic, which a TDR in a long shader turns into a crash (MDN
  `GPUDevice.lost`, Zed PR #50898).
- **A recovery runs the first attach again**, through the calls startup makes. Refused: a
  recovery path of its own, which drifts from the init.
- **Rebuilt, not repaired**: device-bound state is made again by its constructors. Refused: a
  `reset` on each GPU type, which a later field can miss (Zed PR #54349, DirectX
  DeviceResources).
- **The app uploads its images again** when `Ui::device_restored()` is true. Refused: a CPU copy
  of every image, which doubles the RAM of darkroom's 64 MP images (DirectX `OnDeviceRestored`,
  Android `onSurfaceCreated`).
- **`GpuPaint::init` runs again.** Refused: a `device_lost` hook, as Rust drops the old handles.
- **`Gpu` owns the device's health.** Refused: callbacks set in each host. Cost: a callback the
  app set is replaced, and a destroyed device reads lost only after a `Gpu::wait` (wgpu #9278).
- **No poll on the frame path**: each submit runs the callbacks. Refused: a loop of empty
  submits, which never ends on a destroyed device; `catch_unwind`, useless under
  `panic = "abort"`. Cost: a loss in a blocking wait ends the process, as in Zed.
- **The texture limit is shared state**, as the new adapter's can be smaller. Refused: asking for
  the old limits, which fails a recovery onto a smaller GPU.
- **Every window gets a new surface.** Refused: the old surface on the new device, which
  releases its swapchain through that device (wgpu-hal `vulkan::Surface::configure`).
- **Tests lose a real device**, an isolated one, through `Gpu::lose`, as wgpu's tests do. By
  hand, `cat /sys/kernel/debug/dri/0/amdgpu_gpu_recover` as root resets the GPU for every app.

## Harness

`Harness` runs one window with no desktop for unit tests, UI tests and agents; `Remote` plays the
same steps on a live window. The rules of a frame are in `host/harness/mod.rs`'s module doc.

- **The harness is a host**, on the core and window driver the desktop runs. Refused: a `Ui`
  field, a frame loop every app pays for; a separate crate, which cannot reach the core; a
  frame path of its own, which drifts
  ([egui_kittest](https://docs.rs/egui_kittest/latest/egui_kittest/struct.Harness.html),
  [masonry](https://docs.rs/masonry/latest/src/masonry/testing/harness.rs.html),
  [Slint](https://docs.rs/i-slint-backend-testing/latest/i_slint_backend_testing/),
  [Flutter](https://api.flutter.dev/flutter/flutter_test/WidgetTester-class.html),
  [iced_test](https://docs.rs/iced_test)).
- **Public, with no feature**, as it adds no dependency. Refused: masonry's `testing` feature.
- **Three depths, record by default.** Refused: a GPU for every test; a CPU rasterizer, a second
  renderer to keep equal to the GPU one.
- **A stepped clock and a warm start**, so a run repeats and pass counts hold.
- **A closure is an app**, and frames take `impl FnMut(&mut Ui)`, as rustc infers a closure's
  argument types only from an `Fn` bound. Refused: egui_kittest's owned state.
- **A locator hits what the input reaches**: the topmost widget that senses it. Refused: `dyn`
  locators; any sensing widget, which reaches a button a label covers.
- **Methods are input; `play` is a person.** A method queues events and panics on a missed
  locator; `play` settles, spaces presses and returns an error an agent reads.
- **`key` restores the modifiers held before.** Refused: a key that leaves them held.
- **Scroll names its kind** (`scroll_pixels`, `scroll_lines`). Refused: `ScrollDelta`, negated
  at ingest.
- **One window; its commands are recorded.** Refused: a panic, which ends an agent's session.
- **`Remote` queries between two frames inside the app**, as Flutter's driver extension and
  Slint's MCP server do.
- **No MCP server in palantir.** Refused for now: Slint's `slint/mcp` feature, which brings rmcp
  and an async runtime; a second app with a server would argue for a `palantir-mcp` crate.
- **A shot scales down in palantir**, so the shipped build needs no image crate. The showcase's
  long side is 2,576 px, which Claude reads with no downscale
  ([Claude vision](https://platform.claude.com/docs/en/build-with-claude/vision)).
- **The showcase's shot mode is a script**, as iced's `.ice` tests, rebuilt per call so a shot
  never shows a stale binary.
- **The visual suite stays on `OffscreenHost`**, as it tests presentation paths.
- **A settle waits out a busy app** through `App::is_busy` and `Ui::waker`. Refused: a
  `HostHandle` on the harness, which brings winit; a flag the record sets, stale when no frame
  records ([Playwright auto-wait](https://playwright.dev/docs/actionability)).

## Rounded-clip stencil

- **`Stencil8`, zeroed by a queue write when it is made.** On NVIDIA under Vulkan an `S8_UINT`
  image keeps 4 × 4 blocks of old memory that no clear resets, and a copy does. Refused:
  `Depth24PlusStencil8`, four times the memory; a zeroed buffer kept for the write;
  `clear_texture`, a render pass clear in wgpu. Facts:
  `.notes/research/2026-10-31-shared-device-tile-loss.md`.

## Public API

- **Never change it on your own.** Adding, renaming, removing or re-signing an
  exported item — a type, a method, an argument, a trait bound — waits for a
  go-ahead. Propose it, then stop.
- **Match its neighbors.** Before proposing new surface, read what it will
  sit beside — the widget's other setters, the sibling type's methods, the
  trait it joins, the `Response` its peers return — and match their names,
  argument shapes and order.

## Public API design

An exported item that breaks a rule below is a bug. `cargo drive surface` writes the surface to
`.notes/API_SURFACE.md`; regenerate it with each change, as CI compares it with the tree.

### Kinds of type

- **Builder** — consumed by `show`, `build` or `Ui::add_shape`. Setters are bare, take `self`,
  return `Self`.
- **Value** — stored, passed or compared. A chaining setter is `with_*`; a shorthand with no
  argument keeps its adjective (`TextStyle::bold`); a `with_*` without `self` constructs, as
  `Vec::with_capacity`.
- **Plain data** — public fields, no invariant, checked where it enters.
- **Checked value** — private fields behind a checked constructor; a newtype unwraps through
  `get()`, as `NonZeroU32`.
- **Flag set** — `flag_set!`: named flags, `NONE`, `ALL`, set operations, never its bits.
- **Sealed trait** — a public marker over a crate-private supertrait.
- **Error** — `*Error`, an enum of the cases a caller tells apart, or a struct for a single
  condition; `Display`, `Error`, a foreign cause as `source`.

### Names

`cargo drive vocabulary` lists every declared name and word for a review.

- **Spelling.** American, whole words. Abbreviations: the fixed short forms, and `id`, `x` `y`
  `w` `h`, `min` `max`, `dt`, `bbox`, `eps`, `inf`, `rgb` `rgba` `srgb` `hsv` `okhsv` `oklab`
  `hex`, `ctrl` `alt` `mods`, `vsync`, `gpu` `cpu`, `svg`, `fmt`, `config`, `spec`, `stats`. A
  setter's one argument may be a letter its type names (`padding(p)`), `px` for a length; a
  `bool` is named for the setting (`editable(editable)`, as `OpenOptions::read`).
- **Fixed short forms.** `idx`, `pos`, `ctx`/`Ctx`, `dir`, `bg`, `col`, `exec`, `desc`, `cur`,
  `prev`, plurals with `s`; prose, strings and test names keep whole words. A foreign name and a
  term of another meaning keep theirs (`GradientIndex`, `WindowDirectory`, `ContextMenu`).
- **`Chrome` is what a node paints behind its content**: fill, border, radii, shadow. `bg` is a
  background color alone. Refused: `bg` or `background` for a `Chrome`, which holds more than a
  fill; "chrome" for a widget's scaffolding.
- **Constructors.** `new` first; a named one names the kind or arrangement (`Panel::hstack`);
  `from_*` converts; `on(&snapshot)` attaches an overlay, `for_id(id)` by id. No widget
  constructor takes `ui`.
- **Setters** name the setting, not the act (`padding`, not `set_padding`). A boolean takes a
  `bool`; more values take an enum or config, with a shorthand for a common one (`visibility` /
  `hidden`). `default_*` is a fallback, `start_*` a first-frame state. Placement alone is a
  preposition: `at`, `at_origin`, `fixed_at`, `anchor`.
- **Getters** are the noun, never `get_*`. `Ui` state pairs `x()` / `set_x(v)`; an action is a
  verb; a read that wakes the next frame has a `peek_*` twin.
- **Conversions** as std: `as_*` views, `to_*` computes, `from_*` goes back.
- **Predicates.** `is_*`, `has_*`, `can_*` for a property; a third-person verb for a relation
  (`contains`, `approx_eq`).
- **Participles** for this frame's edges, the pointer's levels and derived values (`clicked`,
  `hovered`, `scaled_by`); a predicate where there is none (`Drag::is_live`). Algebra is a verb,
  as std names it (`union`, `clamp_to`).
- **Ids.** A widget's derived ids are `*_id` associated functions; a part's id is
  `id.with("part")`, documented on the owner.
- **Sentinels.** `NONE` paints or holds nothing, `ZERO` is geometric, `TRANSPARENT` a color or
  brush, `ONE`/`IDENTITY` multiplicative, `ALL` a full set, `EMPTY` empty text.
- **Units.** Lengths are logical pixels with no unit in the name; device pixels say `physical`.
  Angles are radians, a fraction of a circle a `turn` (or `hue`), time a `Duration`; other
  units are named in full (`refresh_millihertz`). A multiplier is a `factor`, a share a `ratio`
  or `fraction`. A color is linear `RgbaF32`; `SrgbaU8` is bytes in and out.
- **Lines and corners.** A line's width is `width`, a bar's cross size `thickness`, one rounding
  `radius`, four `Corners`.

### Argument shapes

- **Concrete types**, so a setter can be `const`. `impl Into<T>` only where a call site wants
  shorthand: numbers and tuples for geometry, a color for `Brush`, text, `Option<T>`, `Rc<T>`,
  a pair for `Vec2`, `SrgbaU8` for stored bytes.
- **Required input goes in the constructor**, optional input through setters.
- **Text** a widget reads during the frame takes `impl Into<TextInput<'a>>`.
- **Many results** from a per-frame call go into `&mut Vec<T>` or a borrowing slice or iterator;
  a cold query may return a `Vec`.

### Widgets

- **Lifecycle.** Constructor, setters, `show(self, ui)` or `show(self, ui, body)`. `Configure`
  carries every node setting; a widget does not repeat one.
- **Theme.** `style(s)` takes the slot as `impl Into<Option<&Theme>>`, a second slot
  `<slot>_style`; an override of one axis is a named setter. `chrome` and `default_chrome` are
  held in step by a test in `widgets::tests`.
- **Node values a widget supplies** go through `default_*` in `show`, so a caller's setter wins.
- **`Metrics` carries the type ramp**, Fluent 2 by default. Refused: a size a recipe sets by
  hand, which an app's ramp cannot reach
  ([Fluent 2 typography](https://fluent2.microsoft.design/typography)).
- **Frosted glass holds its ink at WCAG 4.5:1**; palantir owns the math, as it depends on its
  dither. Refused: `Chrome::with_frost` and a `Chrome` setter per `BackdropFilter` constructor
  ([WCAG 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum)).
- **Activation.** Space and Enter fold into one left click; a toggle takes Space alone, as
  WAI-ARIA leaves Enter to the form.
- **A wrapper finishes the widget it wraps through its public setters**, never a hook only the
  wrapper calls.
- **State between frames** lives in the state map under the widget's id, driven from outside by
  associated functions that take `ui` and the id (`open`, `close`, `is_open`).
- **Responses.** `show` returns `Response`, or a `*Response` that holds `response` beside the
  frame's edges; none derefs to `Response`. A bound value returns `ValueResponse`, and a
  discrete pick commits at once.
- **A `Response` borrows the `Ui` immutably**; an overlay attaches through a `ResponseSnapshot`.
  Refused: `Response::tooltip`, which needs `&mut Ui` and names a widget above `widget_core`.
- **A button holds shapes and shows a selected state**, as `aria-pressed`. Refused: a container
  button; a toggle bound to `&mut bool`; a `bool` pick
  ([WAI-ARIA button](https://www.w3.org/WAI/ARIA/apg/patterns/button/)).
- **A field can edit a draft of its own.** Refused: a read-only field, which reaches typing, the
  edit menu, undo and the input method for one caller
  ([Qt `editingFinished`](https://doc.qt.io/qt-6/qlineedit.html#editingFinished),
  [GTK4 `EditableLabel`](https://docs.gtk.org/gtk4/class.EditableLabel.html)).
- **A popup raised at a point stays where it landed** until it stops fitting; one beside a rect
  is placed each frame
  ([`TrackPopupMenu`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-trackpopupmenu)).

### Input validation

Every public input meets two questions, as WPF asks them: _validation_ of the value alone,
_coercion_ against its context. The kinds and their table are in `primitives::math::domain`.

1. **Coercion is total and silent**, and documented on the API, as its context is data: a list
   shrinks, a saved ratio is old. A min above its max raises the max, as in CSS and WPF.
2. **Validation depends on the source.** Per-frame authoring: a release `assert!` with
   `#[track_caller]` — a deliberate exception to "`debug_assert!` on hot paths", as a wrong value
   never draws quietly, at one comparison per value. Cold configuration: a release `assert!`.
   Data from outside the program: `Option` for one rule, `Result` with an error enum for
   several.
3. **Plain data stays plain**, as arithmetic passes through invalid values; it is checked where
   it enters a widget, a shape or a node. A negative size paints nothing.
4. **A theme file is checked on load** by the same predicates as a call site.
5. **An index into a list that changes is coerced**; a handle the crate minted that names
   nothing panics.

Every numeric parameter's doc names its kind, and every setter that can panic says so under
`# Panics`.

## Verification

`cargo drive` runs every check, from the root; the drive table's `version` names the driver
(`cargo install --locked --git https://github.com/xorza/drive`). Never run cargo by hand:
another command shape builds another copy of the crates.

```
cargo drive check          # after each change: fmt, then clippy
cargo drive test [args]    # the tests; a test name picks some
cargo drive bench [args]   # the benches; a filter, or --test to run each case once
cargo drive gate           # the full check; CI runs gate --ci on each OS
```

A run ends on one verdict line, so `| tail -1` is enough; on a failure, read the file it names
rather than run it again.

- **Every test run needs a GPU adapter**; a headless machine needs a software Vulkan driver
  (Mesa lavapipe).
- **`--all-features` is clippy's alone**: under `profile-with-tracy` a test binary opens the
  Tracy socket.
- **A change to bench code** adds `cargo drive bench --test`. Before measuring or `perf`, read
  `benches/AGENTS.md`.
- **A rendering change** runs the golden suite, `cargo drive run visual [filter]`. Goldens are
  local to an adapter: run the unchanged tree first and rewrite stale ones with
  `UPDATE_GOLDEN=1`. A failure leaves the images in `tests/visual/output/<name>/`.
- **A change a user can see** ends with `cargo drive run shot SCRIPT`
  (`examples/showcase/shot/script.rs`); `--against DIR` marks what moved since an earlier run.
  Goldens are a test, never a viewer.

## Non-shipping code

`internals` is the one feature for tests and benches, on through the self dev-dependency.
Homes beside a file's own `mod tests`:

- **`internals` at the end of a file** — a reach-in to private items, gated `#[cfg(test)]` or
  `#[cfg(feature = "internals")]` by its callers, never `any(…)`; a wrong gate shows as dead
  code.
- **`crate::internals`** — the subsystems and shared fixtures, public as `palantir::internals`.
  `gpu::test_gpu` and `gpu::bench_gpu` sit in `gpu`, which `clippy.toml` keeps the wgpu types
  in.
- **`bench.rs`** beside the code it measures, reached through the `bench` facade.
- **`crate::golden`** — golden-image comparison under `feature = "golden"`, public for suites
  outside the crate.

Items code outside the crate calls are `pub`; the rest are `pub(crate)`.
