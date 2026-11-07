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
- **One behavior in every build and on every platform**, unless the difference is speed alone or
  the system decides it: a person's settings, what a device can hold. Refused: a diagnostic on in
  debug alone.
- **A second pass is the last resort.** Do not solve an issue with a relayout, a second record
  pass or another frame. Settle the state before it records, or in the pass that reads it. A
  second pass stays only where it is the canonical, idiomatic answer and avoids a lot of
  complexity, and a comment at the site says why.
- **Docs are starting positions**, this file included. When one contradicts
  the user's intent or the code, flag the conflict and ask.

## Architecture

Each frame rebuilds the tree and runs **record → layout (measure, arrange) → cascade → damage →
encode + compose → paint**, one top-level module per step. Color is linear-RGB f32 on the CPU;
the GPU encodes sRGB at the swapchain write.

A top-level module imports only modules to its left, as `internals::module_order` tests:

```text
common → primitives → {animation, text, display, window, clipboard, image_registry,
                       diagnostics}
       → shape → icons → scene → layout → cascade → {damage, input}
       → renderer → gpu → theme → ui → widget_core → widgets → driver → host
```

## GPU memory

- **Scratch decays.** Per-frame GPU scratch (the blur textures, every `DynamicBuffer`, a window's
  stencil) grows to each frame's need, and at the end of an expiry window of 120 display frames
  (`ExpiryWindow`) steps down to that window's peak. A display frame is one round of every live
  window's submit (`DisplayFrames`). A cache shrinks back to its budget once its live set fits. An
  upload past the staging belt's chunk gets a buffer of its own, and a translucent image
  premultiplies through fixed CPU bands. Refused: a high-water mark kept for the window's life,
  which one 4K glass pane put at about 240 MiB; streaming an image's bands through one buffer, a
  GPU wait per band to bound a peak that lasts one submit.

## Swapchain

Facts and sources: `.notes/research/2026-10-29-swapchain.md`.

- **No frame reads the last frame from the swapchain.** wgpu hands out each image
  uninitialized, so a frame that keeps pixels paints through the backbuffer.
- **Never sample a swapchain image.** Metal and GL offer no `TEXTURE_BINDING`; a pass that
  samples the frame samples the backbuffer. Refused: a Vulkan-only sampled path an Apple GPU cannot
  run.
- **Ask only for usages the surface offers.** `COPY_DST` and `COPY_SRC` are optional, and each
  path that needs one checks it; a usage the surface lacks fails `configure`.
- **Nothing is keyed by an acquired image**, as wgpu does not stop a stale view after a
  reconfigure.
- **`configure` runs only on a change**, as it waits for the device to idle.

## Backends

- **Vulkan, Metal and GL; no Direct3D 12.** A Windows machine runs Vulkan, or GL where its driver
  offers compute, and one with neither gets a refusal that names why. CI runs Mesa's lavapipe on
  Windows, as [wgpu's own CI](https://github.com/gfx-rs/wgpu/blob/trunk/.github/actions/install-mesa/action.yml)
  installs it. Cost: GL on Windows has no CI run, and a machine with no Vulkan driver loses
  Direct3D's fallback: Intel's integrated GPUs before Skylake on Windows, and a VM with no GPU driver,
  which WARP served. Refused: Direct3D 12 as a fallback or first, as Chrome and Firefox use it on
  Windows: FXC, its compiler that needs no DLL shipped, frees cached blobs across threads
  ([gfx-rs/wgpu#10547](https://github.com/gfx-rs/wgpu/issues/10547)), which took a lock around every
  pipeline build on every backend, and HLSL's constant-buffer rules broke the immediates.

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
- **The app uploads an image again** when its `ImageHandle::is_resident()` is false. Residency is
  the image's own fact, so one check serves every window that shows it. Refused: a CPU copy of
  every image, which doubles the RAM of darkroom's 64 MP images (DirectX `OnDeviceRestored`,
  Android `onSurfaceCreated`); a per-window `device_restored` signal, which a two-window app
  answers twice for one host-wide image.
- **`GpuPaint::init` runs again.** Refused: a `device_lost` hook, as Rust drops the old handles.
- **`Gpu` owns the device's health.** Refused: callbacks set in each host. Cost: a callback the
  app set is replaced, and a destroyed device reads lost only after a `Gpu::wait` (wgpu #9278).
- **No poll on the frame path**: each submit runs the callbacks. Refused: a loop of empty
  submits, which never ends on a destroyed device; `catch_unwind`, useless under
  `panic = "abort"`. Cost: a loss in a blocking wait ends the process, as in Zed.
- **The texture limit is shared state**, as the new adapter's can be smaller. Refused: asking for
  the old limits, which fails a recovery onto a smaller GPU.
- **Tests lose a real device**, an isolated one, through `Gpu::lose`, as wgpu's tests do. By
  hand, `cat /sys/kernel/debug/dri/0/amdgpu_gpu_recover` as root resets the GPU for every app.

## Harness

`Harness` runs one window with no desktop for unit tests, UI tests and agents; `Remote` plays the
same steps on a live window. The rules of a frame are in `host/harness/mod.rs`'s module doc.

- **The harness is a host**, on the core and window frames the desktop runs, public with no
  feature. Refused: a frame path of its own, which drifts; a separate crate, which cannot reach the
  core ([egui_kittest](https://docs.rs/egui_kittest/latest/egui_kittest/struct.Harness.html),
  [masonry](https://docs.rs/masonry/latest/src/masonry/testing/harness.rs.html),
  [Flutter](https://api.flutter.dev/flutter/flutter_test/WidgetTester-class.html)).
- **Three depths, record by default, on a stepped clock**, so a run repeats and a test pays only
  for what it reads. Refused: a GPU for every test; a CPU rasterizer, a second renderer to keep
  equal to the GPU one.
- **One set of agent verbs on both hosts**, `AgentWindow`, so a driver writes a step once.
  Refused: matching inherent methods on each host; a harness that owns its app.
- **A step acts as a person does.** A locator hits what the input reaches, and `play` settles,
  spaces presses, refuses a step that would pass with no sign, and returns an error an agent reads
  ([Playwright actionability](https://playwright.dev/docs/actionability)).
- **A played step changes the modifiers only when it names some**, so a key a person holds at a
  live window stays held. Refused: clearing them around each step.
- **A settle waits for what the window owes now, within a bound**, as Flutter's `pumpAndSettle`
  pumps frames and fires no `Timer`. Refused: waiting out timers, or a busy app with no limit.
- **No MCP server in palantir.** Refused for now: a feature over rmcp, which brings an async
  runtime and a fast-moving public dependency; a feature written by hand, a protocol to keep up to
  date. A second app with a server would argue for a `palantir-mcp` crate.
- **The visual suite stays on `OffscreenHost` and wears a theme of its own**, as it tests
  presentation paths and no recipe or palette may move a golden.

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
- **No caller is no reason to remove.** An item a well-made API of its kind would offer
  stays, as `TextShape::weight` beside the font a run names and `Spacing::from_array` beside
  `to_array`. An item goes when it is wrong for the surface: bad, a second spelling of the
  canonical one, obsolete, inconvenient, unidiomatic or ambiguous, as a `(top, bottom)` tuple
  for `Corners` that `Spacing` and CSS each read another way.
- **Match its neighbors.** Before proposing new surface, read what it will
  sit beside — the widget's other setters, the sibling type's methods, the
  trait it joins, the `Response` its peers return — and match their names,
  argument shapes and order.

## Public API design

An exported item that breaks a rule below is a bug. `cargo drive surface` writes the surface to
`.notes/API_SURFACE.md`; regenerate it with each change, as CI compares it with the tree. The
driver serves that command alone; the drive table's `version` names it
(`cargo install --locked --git https://github.com/xorza/drive`).

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
  condition; `Display`, `Error`, a foreign cause as `source` and never also in `Display`, so a
  reporter that walks the chain prints each sentence once
  ([error-handling project group](https://blog.rust-lang.org/inside-rust/2021/07/01/What-the-error-handling-project-group-is-working-towards.html)).

### Names

- **Spelling.** American, whole words. Abbreviations: the fixed short forms, and `id`, `x` `y`
  `w` `h`, `min` `max`, `dt`, `bbox`, `eps`, `inf`, `rgb` `rgba` `srgb` `hsv` `okhsv` `oklab`
  `hex`, `ctrl` `alt` `mods`, `vsync`, `gpu` `cpu`, `svg`, `fmt`, `config`, `spec`, `stats`. A
  setter's one argument may be a letter its type names (`padding(p)`), `px` for a length; a
  `bool` is named for the setting (`editable(editable)`, as `OpenOptions::read`).
- **Fixed short forms.** `idx`, `pos`, `ctx`/`Ctx`, `dir`, `bg`, `col`, `exec`, `desc`, `cur`,
  `prev`, plurals with `s`; prose, strings and test names keep whole words. A foreign name and a
  term of another meaning keep theirs (`GradientIndex`, `WindowDirectory`, `Menu`).
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
  verb. A read of a level the pointer or the modifiers hold, `pointer_pos` and `modifiers`, wakes
  the frame its change lands in, and has a `peek_*` twin that does not, for a read another event
  already woke; no other read has one.
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

- **Lifecycle.** Constructor, setters, `show(self, ui)` or `show(self, ui, body)`. A widget that
  also shows alone keeps `show(self, ui)` and takes its body through `show_with(self, ui, body)`,
  as `Button::show_with` does; `show` calls it with an empty body. `Configure` carries every node
  setting; a widget does not repeat one.
- **Theme.** `style(s)` takes the slot as `impl Into<Option<&Theme>>`, a second slot
  `<slot>_style`; an override of one axis is a named setter.
- **A setter each widget repeats is written out on each widget**: `chrome` and
  `default_chrome` on the containers, the eight typography setters on `Text`, `TextEdit` and
  `EditableLabel`, each consuming `Configure` setter beside its `Configurator` form. The copies
  stay symmetrical, one name, argument and doc for one axis, and a test in `widgets::tests`
  holds the `chrome` copies in step. Refused: a macro that writes them, which hides the API from
  a reader of the source; for the container and text setters, a mixin trait of default methods
  as `Configure` is, which takes `const` off each setter and adds a trait each caller imports.
- **Node values a widget supplies** go through `default_*` in `show`, so a caller's setter wins.
- **`Metrics` carries the type ramp**, Fluent 2 by default. Refused: a size a recipe sets by
  hand, which an app's ramp cannot reach
  ([Fluent 2 typography](https://fluent2.microsoft.design/typography)).
- **Reduced motion and reduced transparency are preferences on the `Ui`**, each a
  `Preference<Reduction>` (`System` or `Fixed`) resolved against the system's setting, as the
  scheme's `Preference<ColorScheme>` is: the host reports the system's answer, and the app's pick
  overrides it. The three are one `UiPreferences`, which `Ui::preferences` and
  `Ui::set_preferences` read and write whole, as egui's `Context::options_mut` holds its options.
  One theme holds every transition and every frost; reduced motion snaps them and stops
  decorative paint motion, and reduced transparency paints every backdrop filter as its
  `fallback` or its fill made opaque, with no capture or blur, as WinUI's acrylic falls back to
  its `FallbackColor` ([acrylic](https://learn.microsoft.com/windows/apps/design/style/acrylic)).
  Refused: two switches in the `Theme`, a person's or a system's choice in a theme file that a
  transition would blend; a third "performance" flag, which reduced transparency already is, as
  Windows's transparency effects are.
- **The stock themes frost their floats** in `Metrics::float_material`: any `BackdropFilter`
  look under the float's tint, its luminance bound derived from each float's ink. Refused:
  opaque defaults, which made an app build its themes again to frost them and again to stop; a
  token per look (blur, frost, glass), three spellings of a choice one filter holds.
- **Frosted glass holds its ink at WCAG 4.5:1**; palantir owns the math, as it depends on its
  dither. Refused: `Chrome::with_frost` and a `Chrome` setter per `BackdropFilter` constructor
  ([WCAG 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum)).
- **One chord table**, Cmd standing in for Ctrl on macOS: Redo is Ctrl+Shift+Z; Ctrl or Alt acts
  on a word; Ctrl with Home, End, Up or Down goes to the document's ends. Refused: Ctrl+Y, a second
  chord for one action; macOS's Cmd+Left line chords and each platform's own table, as
  [Flutter](https://codebrowser.dev/flutter/flutter/packages/flutter/lib/src/widgets/default_text_editing_shortcuts.dart.html)
  binds, which a person who moves between machines learns twice.
- **A person's scroll setting moves a scroll, not a zoom.** `InputEvent::ScrollLines` carries the
  notches a wheel turned apart from the lines and pages they scroll, and a wheel zoom reads the
  notches. The Windows host reads `SPI_GETWHEELSCROLLLINES` and `SPI_GETWHEELSCROLLCHARS` at each
  wheel event, as [Microsoft's sample](https://devblogs.microsoft.com/oldnewthing/20030807-00/?p=42963)
  does, since winit reports no change of them. AppKit applies its own speed before the event, and
  the X11 and Wayland protocols state none. Refused:
  one lane, multiplied, which makes a wheel zoom jump
  ([GTK #1408](https://gitlab.gnome.org/GNOME/gtk/issues/1408)); ignoring the setting, which a
  person's settings decide.
- **Activation.** Space and Enter fold into one left click; a toggle takes Space alone, as
  WAI-ARIA leaves Enter to the form. Each is a `KeyClass` of its own, so a focused widget's scope
  takes those keys and no letter, as a Qt or GTK button accepts only the keys it acts on.
- **A wrapper finishes the widget it wraps through its public setters**, never a hook only the
  wrapper calls. Widgets of the crate that cooperate, not restyle, do so through a `pub(crate)`
  protocol entry point documented on the widget that offers it: the dock and its tab strip
  (`dock_drag`, `scan`, `drop_slot`), a menu and its popup (`show_as`, `settle`, `menu_look`).
  Refused: publishing those hooks, a protocol no app needs that each refactor would break.
- **State between frames** lives in the state map under the widget's id, driven from outside by
  associated functions that take `ui` and the id (`open`, `close`, `is_open`).
- **Responses.** `show` returns `Response`, or a `*Response` that holds `response` beside the
  frame's edges; none derefs to `Response`. A bound value returns `ValueResponse`, and a
  discrete pick commits at once.
- **A `Response` is owned**: `show` probes the widget's state once and hands it back, with no
  borrow of the `Ui`, so one type serves a read, an overlay's anchor and a composite's gathering.
  Measured on the frame bench's tree against the lazy probe it replaced, which read 2 of 896
  handles a pass: 9 µs a record pass, 6 to 10 µs and 2 to 9 % of a CPU frame. Refused: a lazy `Response`
  over `&Ui`, whose borrow makes an owned twin of each response, `ResponseSnapshot`, `ValueEdges`
  and `EditEdges`, and a lifetime on every response type; `Response::tooltip`, which needs
  `&mut Ui` and names a widget above `widget_core`.
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
- **A pick binds a reference into its options**: `ComboBox` writes `&'o T` among `&'o [T]`, so a
  pick clones nothing and `T` needs only `PartialEq`. Refused: an index, which each caller maps
  back to its value (egui's `show_index`); a bound value, which a caller that re-seeds the binding
  from its own state each frame clones each frame, as darkroom's const editor did against its
  settled-frame allocation test. The exception is a pick over a list the widget itself shortens or
  reorders: `TabbedView` binds the page index, as closing or moving a tab would leave a reference
  into the pages stale or naming another page.
- **A bound number is any of the crate's scalars**: `BoundNum` takes `f32`, `f64`, `i32` and `i64`,
  a closed set and so an enum, where egui reaches the same set with its `Numeric` trait. An `f32`
  is snapped in `f64`, then rounded once to the nearest `f32`.
- **A step is a snap and the arrow unit** on every bound-number widget, from `min` or from zero
  while `min` is unbounded, as HTML's step base is
  ([HTML `step`](https://html.spec.whatwg.org/multipage/input.html#the-step-attribute)). The grid
  sits in the store rule every write passes, so a drag, a key and typed entry all land on it.
- **A disabled color surface lies under a veil**: the field, the bars, a chip and a swatch show
  data colors no look can dim, so a disabled one paints `ColorPickerTheme::disabled_veil`, the
  panel's color at half alpha, over them, as libadwaita dims a disabled widget with
  `filter: opacity` ([libadwaita !307](https://gitlab.gnome.org/GNOME/libadwaita/-/merge_requests/307)).
  Refused: a subtree opacity in the renderer, a change on every backend for one widget family;
  a desaturated texture, a rebuild on each flip that leaves the chips without a rule. Cost: exact
  only over the panel's color.
- **A color chip wears a button's slot**: `ColorPickerTheme::chip` frames the color in a
  `ButtonTheme`'s look for each state through `wear`, and the color fills the chip inside the
  frame's border.
- **One pick from a few is a `RadioGroup`**, whatever it looks like: a pip list or a segmented
  track is a `RadioLook`, as SwiftUI's `pickerStyle` restyles one `Picker`, and Ark UI's segment
  group and Mantine's segmented control are radio groups. The group is one Tab stop and an arrow
  moves focus and the pick together ([APG radio group](https://www.w3.org/WAI/ARIA/apg/patterns/radio/)).
  Its body records the options, so they take any layout and a tooltip. Refused: a free-standing
  radio button, which answers no arrow; options from a slice, which leave no option's response;
  a segmented control of its own, a second widget for one pick.
- **One key map per role**: a slider takes the APG slider's keys, arrows both ways, Page and the
  ends ([APG slider](https://www.w3.org/WAI/ARIA/apg/patterns/slider/)); a drag value the APG spin
  button's, Up and Down, Page and a bounded end, leaving Left and Right to the text
  ([APG spin button](https://www.w3.org/WAI/ARIA/apg/patterns/spinbutton/)).

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

The gate is one chain of plain cargo commands, run from the root. Each is an alias in
`.cargo/config.toml`, and the chain stops at the first that fails:

```
cargo fmt --all && cargo gate-lint && cargo gate-lint-nowinit && cargo gate-ship && cargo gate-ship-nowinit && cargo gate-test && cargo gate-test-nowinit && RUSTDOCFLAGS="-D warnings" cargo gate-doc && cargo gate-doctest
```

Each command reports every error it finds, through `--keep-going` and `--no-fail-fast`. **Run the
gate before a turn that changed code ends, and before each push**: master takes direct pushes,
and CI runs the same lanes on three OSes. A filtered `cargo gate-test <name>` is the cheapest way
to see an edit work: it builds the gate's own test shape, so the gate finds that build fresh.
Run the aliases rather than cargo with flags of your own: another flag set builds another copy
of the crates.

- **A change to the public API also runs `cargo drive surface`** and commits the
  `.notes/API_SURFACE.md` it writes. The gate chain does not run it, and CI fails on any difference.
- **Every test run needs a GPU adapter**; a headless machine needs a software Vulkan driver
  (Mesa lavapipe).
- **A slow first GPU test can be the build folder, not the GPU.** The first Metal call lists the
  folder that holds the executable, through `NSBundle`: a `deps/` of 221,686 entries made it 800 ms
  on an Apple laptop, and the still gate took 0.26 s after `cargo clean`. Cargo leaves old build
  output there, so a `cargo clean` is the remedy.
- **`--all-features` is clippy's alone**: under `profile-with-tracy` a test binary opens the
  Tracy socket.
- **A change to bench code** adds `cargo bench-once`. Before measuring or `perf`, read
  `palantir-showcase/benches/AGENTS.md`.
- **A rendering change** runs the golden suite, `cargo visual [filter]`. Goldens are
  local to an adapter: run the unchanged tree first and rewrite stale ones with
  `UPDATE_GOLDEN=1`. A failure leaves the images in
  `palantir-showcase/tests/visual/output/<name>/`.
- **A change a user can see** ends with `cargo shot SCRIPT`
  (`palantir-showcase/src/shot/script.rs`); `--against DIR` marks what moved since an earlier run.
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
- **One test GPU instance a process**, closed in an `atexit` handler, so a test holds a handle or
  a resource in any order. `gpu::test_gpu`'s `PROCESS` doc gives the reasons and what was refused.

Items code outside the crate calls are `pub`; the rest are `pub(crate)`.

- **The showcase is a crate**, `palantir-showcase`, as egui's
  [`egui_demo_lib`](https://docs.rs/crate/egui_demo_lib/0.34.0/source/README.md): it reaches the
  public API alone, and it holds the benches and the integration tests, which run its pages.
  Refused: `#[path]` imports of an example's files; the tour in `src` behind a feature, which can
  reach crate privates and compiles into each test build of the lib; a dev-dependency cycle, whose
  unit tests see a second palantir. The nowinit lanes leave the crate out, as it needs winit.
- **The frame benches run over `bench::scene::Scene`**, the showcase's first page, so a number
  comes from a screen a person opens. The drivers add a counter and a transform around it, so the
  page holds no bench code, and it holds still at rest, so the spinners sit on the Motion page.
  Cost: 246 nodes, where the fixture it replaced held 1,247, and no mesh, image or icon. Refused:
  a fixture beside the tour, a second screen to keep; a bench-only flag on the page.

- **One oracle per property**, in `internals::oracle`, over every column the property covers.
  Refused: a comparator per suite, each blind to other columns, where a `scroll_content`
  restore bug would pass the suites that skip it.
- **One fixture per costly resource** (a window, a shaper, a GPU frame), built once per test, and
  a table over a contract's whole domain (`PaintTier::ALL²`, `PointerButton::ALL`) where tests
  checked it by sample. No real sleep where the code under test takes a clock or a waker.
