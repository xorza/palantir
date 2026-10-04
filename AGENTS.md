# Palantir

A Rust GUI crate. **Immediate-mode authoring API**, **WPF-contract two-pass
layout with flex-shrink sizing**, **wgpu rendering**.

## Posture

- **Break things freely, once asked.** Big-bang renames and migrations are
  welcome — no deprecation shims, compat aliases, feature flags, or migration
  helpers. Who decides is **Public API**, below.
- **Per-frame allocation is a regression.** Steady state is heap-alloc-free
  after warmup; push onto retained scratch with capacity reuse, never rebuild
  a map per frame.
- **API ergonomics matter.** Builder chains read like prose, defaults are
  right, surprising behavior gets a pinning test. When in doubt, favor
  call-site readability.
- **Micro-optimize freely** — struct packing, const fns, scratch reuse, cache
  layout — even without a workload demanding it.
- **Ship in measurable slices.** One feature with tests and a showcase section
  beats a half-finished cluster. A structurally complex change with no
  motivating workload is "too early": shelve it with a note.
- **Docs are starting positions**, this file included. When one contradicts
  user intent or current code, flag the conflict and ask rather than defer.

## Public API

**Never change it on your own.** Adding, renaming, removing, or re-signing an
exported item — a type, a method, an argument, a trait bound — waits for my
go-ahead. Propose it, then stop.

**Design it against its neighbours.** Before proposing new surface, read what
it will sit beside — the widget's other setters, the sibling type's methods,
the trait it joins, the `Response` its peers return — and match their names,
argument shapes, and order. A new item that reads unlike its neighbours is a
bug, however good it looks alone.

**Widgets use only the public API.** A widget here reaches nothing an outside
crate could not: no `pub(crate)` helper, private field, or crate-only trait or
macro. When it needs more, make that public first, with the docs a stranger
needs. The test: someone could reimplement the widget outside the crate, line
for line.

The design rules every exported item follows are in **Public API design**,
below. An item that breaks one is a bug. `python3 scripts/api_surface.py`
renders every exported declaration into `.notes/API_SURFACE.md`, over every
public feature; regenerate it with any change to the surface, and audit
against it.

## Public API design

### Kinds of type

- **Builder** — consumed once, by a terminal `show` or `build`, or by
  `Ui::add_shape`: widgets, shapes, `LayerScope`, `GradientBuilder`, the host
  builders. Its setters are bare (`.padding(4.0)`), take `self` and return
  `Self`.
- **Value** — stored, passed or compared. A chaining setter that takes an
  argument is `with_*`. A shorthand that takes none keeps its adjective
  (`TextStyle::bold`, `Shadow::inset`). A `with_*` with no `self` is a
  constructor that takes one setting, as `Vec::with_capacity` is
  (`Mesh::with_capacity`, `TextShaper::with_fonts`).
- **Plain data** — public fields and no invariant: `Rect`, `Size`, `Spacing`,
  `Span`, `Corners`, `RgbaF32`, `Stroke`, `Shadow`, `Background`, `Brush` and the
  gradients, `GlyphFont`, `TextStyle`, `PaintAnimation`, the theme structs.
  Arithmetic may pass through values no widget takes, so the check runs where
  the value enters (**Input validation**).
- **Checked value** — a consumer relies on an invariant, so the fields are
  private behind a checked constructor, and getters read them: `Sizing`,
  `Track`, `GridCell`, `TranslateScale`, `Stop`, `DockSplit`, `ZoomConfig`. A
  single-value newtype unwraps through `get()` (`UserScale`, `ZoomFactor`,
  `FontWeight`), as `NonZeroU32` does.
- **Flag set** — a `flag_set!` type: its named flags, `NONE`, `ALL`, the set
  operations and `|`, and never its bits (`Sense`, `KeyFilter`,
  `PointerWake`, `KeyboardWake`). A set of booleans a caller writes as a
  struct literal is plain data with the same named constants (`Modifiers`,
  `ShortcutMods`).
- **Sealed trait** — a trait whose implementors the crate enumerates is a
  public marker over a crate-private supertrait (`Lower`, `GradientGeometry`).
- **Error** — an enum of the cases a caller can tell apart, named `*Error`. A
  single condition is a struct named for the condition (`ClipboardUnavailable`,
  `HostDisconnected`). Each implements `Display` and `Error`, and keeps a
  foreign cause as its `source`.

### Names

- **Spelling.** American (`center`, `color`) and whole words
  (`AnimationSpec`, `Interpolation`, `SplitDirection`, `saturation`). The
  abbreviations allowed are the ones geometry, colour and input already use:
  `id`, `x` `y` `w` `h`, `min` `max`, `pos`, `dt`, `bbox`, `col` (beside
  `row`), `eps`, `inf`, `rgb` `rgba` `srgb` `hsv` `okhsv` `oklab` `hex`,
  `ctrl` `alt` `mods`, `vsync`, `gpu` `cpu`, `svg`, `fmt` after
  `format_args!`, and the words `config`, `spec` and `stats`. Parameter names
  follow the rule too (`press`, `shortcut`, `context`, `top_left`), except a
  single letter for a setter's one argument when its type names it
  (`padding(p)`, `style(s)`), and `px` for a length.
- **Constructors.** `new` is the primary one. A named constructor names the
  kind or the arrangement it builds (`Panel::hstack`, `Sizing::fixed`,
  `Anchor::below`, `Splitter::row`, `Shape::rect`, `Background::rounded`).
  `from_*` converts another representation (`Image::from_srgba8`,
  `Rect::from_min_max`, `ButtonTheme::from_palette`). `on(&snapshot)` attaches
  an overlay to a trigger, with the snapshot first and required text second;
  `for_id(id)` attaches by id. No widget constructor takes `ui`; a
  `Response` borrows the `Ui`, so `Response::new` does.
- **Setters.** Named for the setting, not the act (`padding`, not
  `set_padding`). A boolean setting takes a `bool`, so a caller can bind it to
  state. A setting with more than two values takes its enum or its config, and
  a common value may get a shorthand that takes nothing beside it
  (`visibility` / `hidden`, `bar_mode` / `hide_bars`, `zoom_config` /
  `zoomable`, `weight` / `bold`). `default_*` is a fallback used only where the
  caller set nothing (`ThemeDefaults`, `default_background`). `start_*` is a
  first-frame state the widget owns after that (`Expander::start_open`).
  Placement is the one setting named by a preposition: `at(rect)` on a shape,
  `at_origin(point)` for text, `fixed_at(point)` on a layer; its anchored
  form is `anchor(anchor)` (`Popup`, `LayerScope`).
- **Getters.** The noun of what they return, never `get_*`. State on `Ui`
  reads through `x()` and writes through `set_x(v)`, and the pair shares its
  noun (`theme` / `set_theme`, `focus` / `set_focus`); a value that mutates in
  place pairs them the same way (`ColorCoords::hue` / `set_hue`,
  `Clipboard::text` / `set_text`). An action is a verb (`open_window`,
  `request_repaint`, `clear_focus`). A read that also wakes the next frame on
  a change has a `peek_*` twin that does not (`pointer_pos` /
  `peek_pointer_pos`).
- **Conversions** follow std: `as_*` is a free view (`as_str`, `as_solid`),
  `to_*` computes a new value (`to_color`, `to_srgba_u8`, `to_animated`), and
  `from_*` is the constructor that goes the other way.
- **Predicates.** A property of a value, or of `Ui` state, is `is_*`, `has_*`
  or `can_*` (`is_noop`, `is_approx_zero`, `is_window_open`,
  `is_focus_within`, `has_font`, `can_split`). A relation to the argument is a
  verb in the third person (`contains`, `intersects`, `matches`, `takes`,
  `allows`, `passes`, `approx_eq`).
- **Edges and derived values.** What happened this frame is a past participle:
  response fields and methods (`clicked`, `changed`, `committed`, `dismissed`,
  `repaint_requested`). The pointer's state on a widget reads the same way, as
  a level (`hovered`, `held`, `pressed`, `focused`); a state with no
  participle is a predicate (`Drag::is_live`). A value derived from another
  is a past participle too (`inflated`, `scaled_by`, `stepped_up`,
  `anchored_at`). Algebra on values is a verb, as std names it (`union`,
  `intersect`, `min`, `max`, `clamp_to`, `compose`, `combine`).
- **Ids.** A widget's derived ids are `*_id` associated functions on the
  widget type (`TabStrip::chip_id`, `DockView::pane_id`). A part's id is its
  owner's `id.with("part")`, documented on the owner.
- **Sentinels.** `NONE` is a value that paints nothing or a set that holds
  nothing (`Background::NONE`, `Stroke::NONE`, `Shadow::NONE`, `Sense::NONE`,
  `Modifiers::NONE`, `TextStyleOverrides::NONE`). `ZERO` is a geometric zero
  (`Rect`, `Size`, `Spacing`, `Corners`). `TRANSPARENT` is a colour or a brush
  that paints nothing. `ONE` and `IDENTITY` are the multiplicative identity.
  `ALL` is a full set, and `EMPTY` is empty text.
- **Units.** A length is in logical pixels, and its name carries no unit
  (`font_size`, `thickness`, `min_thumb`). A device-pixel quantity says
  `physical` (`Display::physical`, `GpuFrameContext::physical_size`). An angle
  is in radians. A fraction of a full circle is a *turn*, named `turn` or
  `hue`.
  Time is `std::time::Duration`, never a float of seconds or milliseconds. Any
  other unit is named in full in the identifier (`refresh_millihertz`,
  `percent`). A multiplier is a `factor` (`line_height_factor`,
  `scaled_by(factor)`), and a share of a whole is a `ratio` or a `fraction`
  (`DockSplit::ratio`, `thickness_ratio`, `ProgressBar::new(fraction)`). A
  colour is linear `RgbaF32`; `SrgbaU8` is for bytes in and out.
- **Lines and corners.** The width of a line — a stroke, a border, a caret — is
  `width` or `*_width` (`Stroke::width`, `border_width`, `caret_width`,
  `arrow_width`). The cross size of a bar, a track or a band is `thickness` or
  `*_thickness` (`Separator::thickness`, `track_thickness`,
  `rule_thickness`). One corner rounding is `radius` or `*_radius`
  (`TabsTheme::radius`, `preview_radius`), and four are `Corners`.

### Argument shapes

- **Concrete types**, so a setter can be `const`. `impl Into<T>` only where
  `T` has shorthand spellings a call site wants: numbers and tuples for
  `Spacing`, `Corners`, `Size`, `Sizing`, `SizeSpec` and `GridCell`; a colour
  or a gradient for `Brush`; text for `TextInput` (borrowed, owned, interned,
  or `fmt!` output) and for `String` where the text outlives the frame (window
  titles); `Option<T>` for an optional argument (`style`,
  `TabStrip::selected`); `Rc<T>` for a shared value (`Ui::set_theme`,
  `Ui::load_icons`); a pair or array for `Vec2` (`Configure::position`);
  `SrgbaU8` where a colour is stored as bytes, so a linear `RgbaF32` encodes
  once at the boundary (`Mesh` vertices); a `&'static str` for a name
  (`AnimationSlot`); and a source enum (`FontSource`, `DragNum`, `PathBuf`).
- **Required input goes in the constructor**: the bound value (`&'a mut T`),
  the options, required text. Optional input goes through setters. An
  optional binding takes `&'a mut T` in a setter named for what it binds
  (`Expander::open`).
- **Text.** Required text goes in the constructor (`Text::new(text)`,
  `MenuItem::new(label)`, `Tooltip::on(&snapshot, text)`); optional text goes
  through `.label(..)`. Every text argument a widget reads during the frame
  takes `impl Into<TextInput<'a>>`. `TabItem` holds an `InternedStr`, because
  it is `Copy` data in a slice.
- **Many results.** A call that can run every frame and yields many items
  writes into the caller's `&mut Vec<T>`, or returns a borrowing slice or
  iterator, never a fresh `Vec`. A cold query may return a `Vec`
  (`Ui::font_families`).
- **`const`** on every function whose body allows it.

### Widgets

- **Lifecycle.** Constructor, then setters, then `show(self, ui)` or
  `show(self, ui, body)`. `Configure` carries every node setting: identity,
  size, spacing, placement, sense, focus, input scope, visibility, clip. A
  widget does not repeat one as an inherent setter.
- **Theme.** `style(s)` takes the widget's theme slot as
  `impl Into<Option<&Theme>>`; a second slot is `<slot>_style`
  (`ComboBox::button_style`). An override of one axis of the resolved slot is
  a named setter (`color`, `thickness`, `font_size`). A widget with chrome has
  `background(background)` and `default_background(background)`; no trait
  holds them in step, a test in `widgets::tests` does.
- **Wrappers.** A wrapper holds the widget it wraps, forwards `Configure` to
  it, and finishes it through that widget's public setters. It never reaches
  past them through a hook only it calls.
- **State between frames** lives in the state map under the widget's id. Code
  that drives it from outside calls associated functions on the widget type
  that take `ui` and the id: `open`, `close`, `is_open` (`ContextMenu`,
  `PopupTrigger`).
- **Responses.** `show` returns `Response`, or a `*Response` struct that holds
  the widget's own `response` beside what the frame did, as participles. No
  `*Response` derefs to `Response`. A widget that writes a bound value returns
  `ValueResponse { response, changed, committed }`, and a discrete pick
  commits at once (`committed == changed`). A text editor returns
  `TextEditResponse`: the same `changed` and `committed`, beside the cancel,
  submit and focus edges a scrub has no equivalent of. An overlay records in another
  layer and returns `OverlayResponse<R>`: the body's value as `inner`, and
  `dismissed` and `close_requested`. A tooltip senses nothing, so it returns
  `TooltipResponse { visible }`.

### Input validation

Every public input is asked two questions, as WPF asks them of a property
value. *Validation* is about the value alone: finite, not negative, a power of
two. *Coercion* is about the value against its context: inside a range, an
index that exists, a `min` below its `max`.

1. **Coercion is total and silent.** It never asserts, because its context is
   data: an option list shrinks, a saved ratio comes from an older layout. Its
   result is documented on the API. A stale selection shows as the last option
   and is not written back; reversed ranges are ordered; a min above its max
   raises the max, as in CSS and WPF; a fraction is clamped to `0..=1`; a turn
   wraps; a non-finite fraction or turn takes the kind's neutral. A range
   keeps its order only where the direction is the meaning: a `Slider` over
   `10.0..=0.0` runs from right to left.
2. **Validation depends on where the value comes from.**
   - *Per-frame authoring* — builder setters and the value constructors a
     record pass calls: a release `assert!` with `#[track_caller]`, whose
     message is the kind's rule, under `# Panics`. **This is a deliberate
     exception to the global guide's "`debug_assert!` on hot paths"**, for
     public input validation only: a wrong value is never drawn quietly, and
     the cost is one comparison per value. A value computed at run time goes
     through the kind's `is_*` predicate or a coercing kind first.
   - *Cold configuration* — host builders, render targets, theme scaling,
     dock configuration: a release `assert!` under `# Panics`.
   - *Data from outside the program* — files, persisted settings, decoded
     images, icon sources, font names, typed numbers: `Option` when one rule
     can fail, `Result` with an error enum when several can.
3. **Plain data stays plain.** Its fields are public, because arithmetic
   passes through invalid intermediate values. It is checked where it enters
   a widget, a shape or a node. Shape geometry — rects, points, mesh vertices,
   text origins — is checked as *offsets*: finite. A negative size is not an
   error; it paints nothing.
4. **Themes.** A theme file is checked when it loads, by the same predicates
   as a call site, so the two cannot disagree. A theme built in code is plain
   data, checked where its values enter: a value a widget hands to a checked
   setter (`gap`, `padding`, a shape's colour) panics there on a value outside
   its kind, and a length the widget floors goes through `length_at_least`.
   A font is how theme text reaches paint, so a face the shaper cannot use
   shapes nothing, at `Shape::text` as in every text widget.
5. **Indices and handles.** An index into a list that changes at run time
   (options, pages, tabs) is coerced. A handle the crate minted (`IconId`,
   `TabGroupId`) that names nothing is a caller bug and panics.

The kinds live in `widget::domain`, public because a widget outside the crate
validates its setters the same way; `domain::vec2` and `domain::f64` hold the
twins for `Vec2` and for the `f64` values `Slider` and `DragValue` bind. Each
validating kind is an `is_*` predicate and a checker that returns its
argument; each coercing kind is one total `const fn`. Every numeric
parameter's doc names its kind ("`px`: a *length*"), and every setter that can
panic says so under `# Panics`. The table is in the `domain` module doc.

## Architecture

Five passes per frame over a tree rebuilt every frame: **record → measure →
arrange → cascade → encode + compose + paint**. Colour is linear-RGB f32 on
the CPU side; sRGB encoding happens on the GPU at swapchain write.

Each pass has one top-level module: `scene` holds what record writes,
`layout` measures and arranges it, `cascade` derives the tables input and
paint read, `damage` decides what repaints, `renderer` encodes and
composes, and `gpu` paints. `layout`, `cascade` and `damage` each keep their
result type in `mod.rs` and their engine in `engine.rs`.
`primitives` is the value vocabulary below all of them, `widget_core` the
framework the bundled `widgets` are written on.

Read `benches/AGENTS.md` before measuring or reaching for `perf`: it holds the
A/B protocol and the traps that cost a wasted capture each.

## Verification

```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --test alloc --features bench,golden,gpu-debug-markers
```

Every test run needs a GPU adapter: the self dev-dependency turns
`internals` on in every test build, and the GPU tests it brings panic
without one. A headless machine needs a software Vulkan driver (Mesa
lavapipe, `mesa-vulkan-drivers`), as CI installs.

`--all-features` is clippy's alone: `profile-with-tracy` starts the Tracy
client before `main`, so a test binary under it opens the profiler's socket
and prints `SymInitialize FAILED with code 87`. The test run names `alloc`
rather than `--tests` because `golden` would pull in `visual`.

Rendering changes (shaders, encoder/composer, atlases, colour pipeline, layout
that moves pixels) also run the visual suite:

```
cargo test --test visual --features golden
```

Its goldens in `tests/visual/golden/` are local and show whatever tree last
wrote them; a missing one is written and then failed. Run the suite on the
unchanged tree first — if it fails there, rewrite the stale goldens with
`UPDATE_GOLDEN=1` before changing anything. The goldens record the adapter
that wrote them in `golden/adapter.txt`; a run on another adapter fails with
that reason until `UPDATE_GOLDEN=1` adopts it. A failure leaves `actual.png`,
`expected.png`, and `diff.png` in `tests/visual/output/<name>/`.

A change a user can see ends with a look at `cargo run --example showcase`.

## Non-shipping code

Code that does not ship has four homes, apart from a file's own
`#[cfg(test)] mod tests` and the helpers only that module uses.

- **`internals` at the end of a file** — a reach-in: test or bench code
  that needs that file's private items. One per file, the last item before
  `mod tests`. Its `cfg` is exactly the builds its callers exist in —
  `test`, `feature = "bench"`, `feature = "internals"`, or a mix — since
  anything wider is dead code that `-W dead_code` reports. A wider module
  narrows single items with their own `cfg`, and never needs a lint allow.
- **`crate::internals`** (`src/internals/`) — the subsystems: the frame
  harness, the paint capture, the panic probe, the shared fixtures. It is
  `palantir::internals` under `any(test, feature = "internals")`, and each
  submodule carries its own narrower gate. Test code that is a subsystem
  rather than a reach-in goes here, never among production modules.
- **`bench.rs`** beside the code it measures, under `feature = "bench"`,
  reached through the `bench` facade in `src/lib.rs`.
- **`crate::golden`** (`src/golden/`) — golden-image comparison, under
  `feature = "golden"`. It is public API rather than a reach-in, because
  suites outside this crate that draw through Palantir use it too.

Visibility says who reaches in: `pub` when code outside the crate calls it
(`tests/visual`, `tests/alloc`, the showcase, `benches/`), `pub(crate)` when
only the crate's own tests and benches do.

Two subsystems stay outside `src/internals/`. `gpu::test_gpu` and
`gpu::bench_gpu` hold wgpu types, which `clippy.toml` keeps inside
`crate::gpu`, so `crate::internals` re-exports the test GPU. `text::mono` is
a measurement backend beside `cosmic`, gated `any(test, feature =
"internals")`.
