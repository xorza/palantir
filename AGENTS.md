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

Each frame rebuilds the tree and runs **record → layout (measure, arrange) →
cascade → damage → encode + compose → paint**. Color is linear-RGB f32 on
the CPU side; sRGB encoding happens on the GPU at swapchain write.

Each step has one top-level module: `scene` holds what record writes, over
the shapes, payload store and paint animations of `shape` below it, `layout`
measures and arranges it, `cascade` derives the tables input and paint read,
`damage` decides what repaints, `renderer` encodes and composes, and `gpu`
paints. `layout`, `cascade` and `damage` each keep their result type in
`mod.rs` and their engine in `engine.rs`.
`primitives` is the value vocabulary below all of them, `widget_core` the
framework the bundled `widgets` are written on, and `theme` the look they
read. `input` holds the event vocabulary, and `input_state` the state
machine that routes events over the cascade.

### Layering

A top-level module imports only modules to its left:

```text
common → primitives → {animation, text, input, display, window, image_registry, diagnostics}
       → shape → icons → scene → layout → cascade → {damage, input_state}
       → renderer ⇄ gpu → theme → ui ⇄ app → widget_core → widgets → host
```

- **`theme` is plain look data.** It holds the look vocabulary a theme file
  serializes and knows no `Widget` or `Ui`; `Widget::wear` applies a look.
- **`ui` records through `Node` and shapes**, never through `Widget` or a
  bundled widget, so the recorder core sits below the authoring layer.
- **`gpu` ⇄ `renderer` stays a cycle.** `GpuPaint` names `GpuFrameCtx` and
  `GpuInitCtx`, which hold the `wgpu` types `clippy.toml` keeps in `gpu`;
  moving the trait into `gpu` would make `ui` depend on `gpu` for `GpuViews`.
- **`app` ⇄ `ui` stays a cycle.** `App` is the callback contract of the frame
  cycle; the two are one concept.
- **A bench may import modules to its right.** A `bench.rs` sits beside the
  code it measures (**Non-shipping code**), and a whole-frame case drives it
  through `host`, `ui` and `widgets`, as the desktop does. Refused: moving
  such cases to `src/bench/`, away from the code they measure.

## Swapchain

What wgpu 30 allows on a window's swapchain, and the rules the renderer keeps
to stay inside it. Facts and sources:
`.notes/research/2026-10-29-swapchain.md`.

- **An acquired image holds nothing.** wgpu marks it uninitialized: a load
  reads zeros and a present of an unwritten image clears it. No frame reads
  the last frame's pixels from the swapchain. A frame that keeps pixels
  paints through the backbuffer; a direct frame is whole, and its first pass
  clears.
- **Inside one frame the image is an ordinary texture.** A pass may load what
  an earlier pass of the frame stored, and a copy may read it where the
  surface took `COPY_SRC`: a pane's capture, the damage overlay.
- **Ask only for usages the surface offers.** `RENDER_ATTACHMENT` is
  required; `COPY_DST` and `COPY_SRC` are taken where `caps.usages` holds
  them, and every path that needs one checks it (`RenderTarget::takes_copy`,
  `gives_copy`). A usage the surface lacks fails `configure`.
- **Never sample a swapchain image.** Metal, DX12 and GL surfaces offer no
  `TEXTURE_BINDING`; only some Vulkan drivers do. A pass that must sample
  what the frame painted samples the backbuffer. Refused: a sampled path for
  Vulkan alone, a second way to paint a frame that the m2 cannot run.
- **Keep nothing of an image past its present.** Each acquire is a new
  texture: no view, bind group or cache keyed by it outlives the frame, as
  wgpu does not stop a stale view after a reconfigure. One image is acquired
  at a time, and none lives when `configure` runs.
- **Submit, then present.** The frame's work goes in `queue.submit`, then
  `pre_present_notify`, then `queue.present`. A frame that paints nothing
  acquires nothing.
- **Configure as the surface lists.** A nonzero size inside the device's
  largest texture, an sRGB format in the sRGB color space, `AutoVsync` or
  `AutoNoVsync`, an alpha mode from the list. `configure` waits for the
  device to idle, so it runs only when the configuration changed, and a
  rebuild invalidates the target's contents.
- **Handle each acquire outcome as wgpu documents it.** `Suboptimal`:
  present, then reconfigure. `Outdated`: reconfigure. `Lost`: create the
  surface again, as a lost surface does not configure. `Timeout`: paint
  again. `Occluded`: skip. `Validation`: back off.

## Device loss

What a host does when the device goes away: a GPU hang reset by the OS (TDR, amdgpu), a driver
update, a removed GPU. Facts and sources: `.notes/research/2026-10-30-device-loss.md`.

- **Recover, do not panic.** On a loss the host opens a new device and rebuilds every GPU object
  on it; the UI state stays and the next frame of every window paints whole. Refused: a panic,
  which a TDR in a long image-processing shader turns into a crash. Source: MDN
  `GPUDevice.lost`, Zed PR #50898.
- **A recovery runs the first attach again.** Startup and a recovery attach a device through the
  same calls: `HostCore::new` and `HostCore::attach` build the device half with one
  `BackendSpec::open`, `HostCore::attach` gives every window its stream as `HostCore::open` gives
  a new one, and `SurfaceManager` opens the device and builds a window's surface through the
  functions `start` uses. Refused: a recovery path of its own, which drifts from the init.
- **Device-bound state is rebuilt, not repaired.** The backend, the window streams and the view
  targets are made again with their constructors; the CPU atlases the GPU mirrors (gradient
  rows) clear and refill as frames register them, as Zed's atlas does. Only the image store
  attaches in place, as every `ImageHandle` holds its `Rc`. Refused: a `reset` on each GPU type,
  which a field added later can miss. Source: Zed PR #54349, DirectX DeviceResources.
- **Registered images: the app uploads them again.** On the first frame after a recovery,
  `Ui::device_restored()` is true, and the app calls `ImageHandle::update` for the images it
  still shows; until then they paint nothing. Refused: a CPU copy of every image, which doubles
  the RAM of darkroom's 64 MP images. Source: DirectX `IDeviceNotify::OnDeviceRestored`, Android
  `onSurfaceCreated`.
- **`GpuPaint::init` runs again on the new device,** as it does when a view comes back. Refused:
  a separate `device_lost` hook, as Rust drops the old handles when `init` replaces them.
- **`Gpu` owns the device's health.** `Gpu::new` sets the lost callback and the uncaptured-error
  handler; the handler panics on an error while the device is healthy, as wgpu's default does,
  and drops it once the device is lost. Refused: setting them in each host, which leaves a `Gpu`
  without a health. Cost: a callback an app set on the device it wraps is replaced, and a device
  its owner destroys reads lost only after the next `Gpu::wait`, as wgpu runs that callback from
  a maintain (wgpu #9278).
- **No poll on the frame path; wgpu's blocking wait where a caller blocks.** Each `queue.submit`
  runs the map and work-done callbacks, so nothing polls per frame; `Gpu::poll` is an empty
  submit, which a lost device cannot make panic, and `Gpu::wait` is `poll(Wait)` behind the
  health check, then a wait for a work-done marker of its own. `clippy.toml` keeps `Device::poll`,
  `Instance::poll_all` and both device callbacks inside `gpu::device`. Refused: a loop of empty
  submits, which spins and never ends on a device its owner destroyed; `catch_unwind`, which
  `panic = "abort"` makes useless. Cost: a loss found during a blocking wait ends the process, as
  in Zed; the desktop host never waits.
- **Nothing on the GPU half panics on a lost device.** A belt allocation that does not map
  declines (`GpuCtx::stage`), and the upload skips: the loss can come inside a submit, after the
  call that found it.
- **The limits are shared state.** A new adapter can have another `max_texture_dimension_2d`, so
  one `SharedDevice` cell holds the limit the recovery writes; the gradient atlas clears to it,
  the surfaces clamp to it, and an image past it paints nothing, with a `tracing::warn`. Refused:
  requesting the old limits, which fails a recovery onto a smaller GPU that could run the app.
- **A recovery makes every window a new surface,** dropping the old one first, as a lost surface
  is renewed. Refused: configuring the old surface on the new device, which releases its old
  swapchain through that device (wgpu-hal `vulkan::Surface::configure`).
- **A capture the loss broke is taken again.** Every screenshot on its way back from the old
  device goes back to the window's requests at the attach, and the next frame takes it.
- **The hosts report and recover.** The winit host recovers before a window draws on a lost
  device, and `run()` returns `WinitHostError::DeviceLost` when no new device opens.
  `OffscreenHost` runs the CPU half and submits nothing while `is_device_lost()`, and
  `recover(gpu)` attaches the device the app gives; `HeadlessWindow` forwards both and makes its
  texture again. Refused: a new host, which loses the UI state.
- **Tests lose a real device.** `Gpu::lose` destroys an isolated device and waits, as wgpu's own
  device-loss tests do; the shared test device is never lost. A real hang can be checked by hand
  with `cat /sys/kernel/debug/dri/0/amdgpu_gpu_recover` as root while the showcase runs, which
  resets the GPU for every app on the machine.

## Public API

- **Never change it on your own.** Adding, renaming, removing or re-signing an
  exported item — a type, a method, an argument, a trait bound — waits for a
  go-ahead. Propose it, then stop.
- **Match its neighbors.** Before proposing new surface, read what it will
  sit beside — the widget's other setters, the sibling type's methods, the
  trait it joins, the `Response` its peers return — and match their names,
  argument shapes and order.

## Public API design

An exported item that breaks a rule below is a bug. `cargo drive surface`
renders every exported declaration, over every public feature, into
`.notes/API_SURFACE.md`. Regenerate the file with any change to the surface,
and audit against it: CI fails when the committed file differs from what the
tree renders.

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
  `Span`, `Corners`, `RgbaF32`, `Stroke`, `Shadow`, `Chrome`, `Brush` and the
  gradients, `GlyphFont`, `TextStyle`, `PaintAnimation`, the theme structs.
  It is checked where it enters (**Input validation**, rule 3).
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

`cargo drive vocabulary` lists every declared name and word for a review.

- **Spelling.** American (`center`, `color`) and whole words
  (`AnimationSpec`, `Interpolation`, `SplitDirection`, `saturation`). The
  abbreviations allowed are the fixed short forms below and the ones
  geometry, color and input already use: `id`, `x` `y` `w` `h`, `min` `max`,
  `dt`, `bbox`, `eps`, `inf`, `rgb` `rgba` `srgb` `hsv` `okhsv` `oklab`
  `hex`, `ctrl` `alt` `mods`, `vsync`, `gpu` `cpu`, `svg`, `fmt` after
  `format_args!`, and the words `config`, `spec` and `stats`. Parameter names
  follow the rule too (`press`, `shortcut`, `anchor`, `top_left`), except a
  single letter for a setter's one argument when its type names it
  (`padding(p)`, `style(s)`), and `px` for a length.
- **Fixed short forms.** `idx`, `pos`, `ctx`/`Ctx`, `dir`, `bg`, `col`,
  `exec`, `desc`, `cur`, `prev`, and their plurals with `s` (`idxs`, `poss`),
  in every name; prose, strings and test names keep whole words. A name a
  foreign API sets keeps its own form (`Iterator::position`,
  `wgpu::TextureDescriptor`), and so does a term of another meaning: `idx` is
  a position and `dir` a file-system directory, so a lookup index
  (`GradientIndex`), a registry (`WindowDirectory`), a `direction` and a
  `ContextMenu` keep the whole word.
- **`Chrome` is what a node paints behind its content**: fill, border,
  corner radii and shadow, set through `chrome` and `default_chrome`, and
  recorded with the focus ring as `NodeChrome` and `ChromeRow`. `bg` is a
  background color alone (`Palette::window_bg`). Refused: `bg` or
  `background` for a `Chrome`, which holds more than the fill CSS and WPF
  call a background; and "chrome" for a widget's scaffolding (`ToggleRow`)
  or a stroke's join billboards.
- **Constructors.** `new` is the primary one. A named constructor names the
  kind or the arrangement it builds (`Panel::hstack`, `Sizing::fixed`,
  `Anchor::below`, `Splitter::row`, `Shape::rect`, `Chrome::rounded`).
  `from_*` converts another representation (`Image::from_srgba8`,
  `Rect::from_min_max`, `ButtonTheme::from_parts`). `on(&snapshot)` attaches
  an overlay to a trigger, with the snapshot first and required text second;
  `for_id(id)` attaches by id. No widget constructor takes `ui`; a
  `Response` borrows the `Ui`, so `Response::new` does.
- **Setters.** Named for the setting, not the act (`padding`, not
  `set_padding`). A boolean setting takes a `bool`, so a caller can bind it to
  state. A setting with more than two values takes its enum or its config, and
  a common value may get a shorthand that takes nothing beside it
  (`visibility` / `hidden`, `bar_mode` / `hide_bars`, `zoom_config` /
  `zoomable`, `weight` / `bold`). `default_*` is a fallback used only where the
  caller set nothing (`ThemeDefaults`, `default_chrome`). `start_*` is a
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
  nothing (`Chrome::NONE`, `Stroke::NONE`, `Shadow::NONE`, `Sense::NONE`,
  `Modifiers::NONE`, `TextStyleOverrides::NONE`). `ZERO` is a geometric zero
  (`Rect`, `Size`, `Spacing`, `Corners`). `TRANSPARENT` is a color or a brush
  that paints nothing. `ONE` and `IDENTITY` are the multiplicative identity.
  `ALL` is a full set, and `EMPTY` is empty text.
- **Units.** A length is in logical pixels, and its name carries no unit
  (`font_size`, `thickness`, `min_thumb`). A device-pixel quantity says
  `physical` (`Display::physical`, `GpuFrameCtx::physical_size`). An angle
  is in radians. A fraction of a full circle is a _turn_, named `turn` or
  `hue`.
  Time is `std::time::Duration`, never a float of seconds or milliseconds. Any
  other unit is named in full in the identifier (`refresh_millihertz`,
  `percent`). A multiplier is a `factor` (`line_height_factor`,
  `scaled_by(factor)`), and a share of a whole is a `ratio` or a `fraction`
  (`DockSplit::ratio`, `width_ratio`, `ProgressBar::new(fraction)`). A
  color is linear `RgbaF32`; `SrgbaU8` is for bytes in and out.
- **Lines and corners.** The width of a line — a stroke, a border, a caret — is
  `width` or `*_width` (`Stroke::width`, `border_width`, `caret_width`,
  `arrow_width`). The cross size of a bar, a track or a band is `thickness` or
  `*_thickness` (`Separator::thickness`, `track_thickness`,
  `rule_thickness`). One corner rounding is `radius` or `*_radius`
  (`TabsTheme::radius`, `preview_radius`), and four are `Corners`.

### Argument shapes

- **Concrete types**, so a setter can be `const`. `impl Into<T>` only where
  `T` has shorthand spellings a call site wants: numbers and tuples for
  `Spacing`, `Corners`, `Size`, `Sizing`, `SizeSpec` and `GridCell`; a color
  or a gradient for `Brush`; text for `TextInput` (borrowed, owned, interned,
  or `fmt!` output) and for `String` where the text outlives the frame (window
  titles); `Option<T>` for an optional argument (`style`,
  `TabStrip::selected`); `Rc<T>` for a shared value (`Ui::set_theme`,
  `Ui::load_icons`); a pair or array for `Vec2` (`Configure::pos`);
  `SrgbaU8` where a color is stored as bytes, so a linear `RgbaF32` encodes
  once at the boundary (`Mesh` vertices); a `&'static str` for a name
  (`AnimationSlot`); and a source enum (`FontSource`, `DragNum`, `PathBuf`).
- **Required input goes in the constructor**: the bound value (`&'a mut T`),
  the options, required text (`Text::new(text)`, `MenuItem::new(label)`,
  `Tooltip::on(&snapshot, text)`). Optional input goes through setters:
  optional text through `.label(..)`, an optional binding as `&'a mut T` in a
  setter named for what it binds (`Expander::open`).
- **Text.** Every text argument a widget reads during the frame takes
  `impl Into<TextInput<'a>>`. `TabItem` holds an `InternedStr`, because it is
  `Copy` data in a slice.
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
  `chrome(chrome)` and `default_chrome(chrome)`; no trait
  holds them in step, a test in `widgets::tests` does.
- **Node values a widget supplies** go through `default_*` (`default_gap`,
  `default_child_align`, `default_size`), in `show`, so a caller's setter
  always wins over the theme.
- **Activation.** A focused widget reads Space and Enter through
  `Widget::activation_response`, which folds them in as one left click, so
  `clicked()` reads the same from the pointer and the keyboard. A toggle takes
  Space alone, as WAI-ARIA leaves Enter to the form.
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
value. _Validation_ is about the value alone: finite, not negative, a power of
two. _Coercion_ is about the value against its context: inside a range, an
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
    - _Per-frame authoring_ — builder setters and the value constructors a
      record pass calls: a release `assert!` with `#[track_caller]`, whose
      message is the kind's rule, under `# Panics`. **This is a deliberate
      exception to the global guide's "`debug_assert!` on hot paths"**, for
      public input validation only: a wrong value is never drawn quietly, and
      the cost is one comparison per value. A value computed at run time goes
      through the kind's `is_*` predicate or a coercing kind first.
    - _Cold configuration_ — host builders, render targets, theme scaling,
      dock configuration: a release `assert!` under `# Panics`.
    - _Data from outside the program_ — files, persisted settings, decoded
      images, icon sources, font names, typed numbers: `Option` when one rule
      can fail, `Result` with an error enum when several can.
3. **Plain data stays plain.** Its fields are public, because arithmetic
   passes through invalid intermediate values. It is checked where it enters
   a widget, a shape or a node. Shape geometry — rects, points, mesh vertices,
   text origins — is checked as _offsets_: finite. A negative size is not an
   error; it paints nothing.
4. **Themes.** A theme file is checked when it loads, by the same predicates
   as a call site, so the two cannot disagree. A theme built in code is plain
   data, checked where its values enter: a value a widget hands to a checked
   setter (`gap`, `padding`, a shape's color) panics there on a value outside
   its kind, and a length the widget floors goes through `length_at_least`.
   A font is how theme text reaches paint, so a face the shaper cannot use
   shapes nothing, at `Shape::text` as in every text widget.
5. **Indices and handles.** An index into a list that changes at run time
   (options, pages, tabs) is coerced. A handle the crate minted
   (`IconHandle`, `TabGroupId`) that names nothing is a caller bug and panics.

The kinds live in `widget::domain`, public because a widget outside the crate
validates its setters the same way; `domain::vec2` and `domain::f64` hold the
twins for `Vec2` and for the `f64` values `Slider` and `DragValue` bind. Each
validating kind is an `is_*` predicate and a checker that returns its
argument; each coercing kind is one total `const fn`. Every numeric
parameter's doc names its kind ("`px`: a _length_"), and every setter that can
panic says so under `# Panics`. The table is in the `domain` module doc.

## Verification

`cargo drive` runs every check: the driver is `cargo-drive`, the same in every
project, installed with `cargo install --locked --git
https://github.com/xorza/drive`. The drive table's `version` names the driver
the project needs, and an older or newer one refuses to run. Run it from the
root.

```
cargo drive check          # after each change: fmt, then clippy --all-targets --all-features
cargo drive test [args]    # the tests, each time; the runner's arguments pick some: a test name
cargo drive bench [args]   # the benches; a filter, or --test to run each case once
cargo drive gate           # the full check: fmt, each lint set, ship, tests, docs; CI adds --ci
```

`check` takes no arguments: each lane covers the whole workspace, and the
workspace of each path dependency outside it, in its own folder. A step does
not run again when no input changed since it passed; under nextest, the tests
run only for the crates a change reaches and every crate that depends on them.
Each step prints one line; a failure prints each diagnostic once and names its
log, `target/drive/log/<step>.log`, which holds no passing test. A run ends on
one verdict line: `ok`, or `FAIL` with the failed steps and the first error,
and `target/drive/last-<command>.txt`, which holds all the run printed; above
it, each failed step shows its first lines again. So `| tail -1` is enough: on
a failure, read the file it names rather than run the command again. Never
run cargo by hand: another shape of a cargo command builds another copy of
the crates.
`check` and `gate` format the code and end at the first failed step; `gate
--ci` formats nothing and runs every step.

A root's flags are in its `Cargo.toml`, under `[workspace.metadata.drive]`,
or `[package.metadata.drive]` in a root that is one package. `gate` lints with
each set of the `lint` key and tests with each set of the `test` key, both
`--all-features` by default; `check` lints with `--all-features` alone.
`docs = true` adds rustdoc and the doc tests to `gate`, after the others; they
are off by default. `test-runner` is `nextest` (the default) or `cargo-test`,
which needs nothing installed but runs every test when a change reaches any
crate. Every lane runs `--locked`: after a change to the dependencies,
`cargo update --workspace` writes the lock.

CI runs `cargo drive gate --ci` on each OS.

- **Every test run needs a GPU adapter**, as `internals`, on in every test
  build, brings GPU tests. A headless machine needs a software Vulkan driver
  (Mesa lavapipe, `mesa-vulkan-drivers`), as CI installs.
- **The plain clippy lints the build that ships**: every other target turns
  `internals` on through the self dev-dependency.
- **A change to bench code** adds `cargo drive bench --test`, which runs each
  case once; `benches/AGENTS.md` says how to run and measure.
- **`--all-features` is clippy's alone**: under `profile-with-tracy` a test
  binary opens the Tracy socket. The first test set names `alloc`, as
  `--tests` with `golden` would pull in `visual`; the second builds the
  library without `winit`, as a host that embeds the crate does.
- **Rendering changes** (shaders, encoder/composer, atlases, color pipeline,
  layout that moves pixels) also run the golden suite, which no lane holds:
  `cargo drive run visual [filter]`, in the first test lane's shape so it
  reuses that build. Its goldens in `tests/visual/golden/` are local and record their adapter in
  `golden/adapter.txt`. Run the suite on the unchanged tree first, and
  rewrite stale goldens, or another adapter's, with `UPDATE_GOLDEN=1` before
  changing anything. A missing golden is written, then failed; a
  failure leaves `actual.png`, `expected.png` and `diff.png` in
  `tests/visual/output/<name>/`.
- **A change a user can see** ends with a look at
  `cargo run --example showcase`.
- **Before measuring or reaching for `perf`**, read `benches/AGENTS.md`.

## Non-shipping code

`internals` is the one feature for what only tests and benches build; the
crate's dev-dependency on itself turns it on in every test and bench build.
Test and bench code has four homes beside a file's own `#[cfg(test)] mod
tests`:

- **`internals` at the end of a file** — a reach-in to the file's private
  items, the last item before `mod tests`. Its gate is exactly the builds its
  callers exist in: `#[cfg(test)]` when only this crate's unit tests call it,
  `#[cfg(feature = "internals")]` when a bench or an integration test does —
  never `any(test, feature = "internals")`. An item whose callers exist in
  fewer builds narrows with its own `cfg`; a wrong gate shows as dead code,
  which no lint allow hides.
- **`crate::internals`** (`src/internals/`) — the subsystems: the frame
  harness, the paint capture, the panic probe, the shared fixtures; public as
  `palantir::internals`.
- **`bench.rs`** beside the code it measures, reached through the `bench`
  facade in `src/lib.rs`.
- **`crate::golden`** (`src/golden/`) — golden-image comparison, under
  `feature = "golden"`; public API, as suites outside the crate use it.

Items code outside the crate calls (`tests/visual`, `tests/alloc`, the
showcase, `benches/`) are `pub`; the rest are `pub(crate)`. Two subsystems sit
outside `src/internals/`: `gpu::test_gpu` and `gpu::bench_gpu` hold wgpu
types, which `clippy.toml` keeps in `crate::gpu`, and `text::mono` is a
measurement backend beside `cosmic`.
