# The observatory example, and light and dark — proposal

Two pieces of work, in this order:

1. **Light and dark in palantir** (batches L1–L4): a light palette from Ayu Light, a system
   scheme read through `mundy`, and a System / Light / Dark preference on `Ui`. It is public
   API, so each batch waits for its go-ahead (palantir `AGENTS.md`, **Public API**).
2. **A new example, `examples/observatory`** (batches 1–7): five connected studies of one
   fictional tool on one model. The showcase stays whole, as the widget reference.

## Diagnosis of the showcase

The showcase today: 27 pages, 87 sections, 9.2k lines, grouped by crate module (Widgets, Input,
Layout, Paint, Runtime, Diagnostics).

- **Each page is an island.** Only `AppState { counter }` crosses pages, and only the state page
  and the inspector window read it. Nothing done on one page shows on another.
- **Specimens show the parameter, not the use.** Swatches `A`–`E`, buttons "one … five", boxes
  labeled "default theme". A reader learns what `Justify::SpaceAround` does, not when to want it.
- **The one composite with meaning is the strongest page.** The controls page's settings form
  with its live-state readout reads as software; the rest reads as a test sheet.
- **The dock has nothing to dock.** Its panes are placeholders that print their own size.

The showcase has four jobs: the first impression (the README recording), the API reference (the
`api!` chips, the custom-widget page the README links), the regression surface (shot scripts
open a page by label, two goldens compile pages), and the diagnostics (fixtures, frame bench,
overlays). The observatory takes the first job. The showcase keeps the other three; for them, a
specimen that pins one parameter is the right shape.

## Options for the new example

| | Shape | Connects by | Cost | Who does it |
|---|---|---|---|---|
| A | **Catalog with knobs.** Each specimen gets a knobs panel bound to its parameters, and its code. | One anatomy, "related" links | Low; still a list | [Storybook Controls](https://storybook.js.org/docs/api/doc-blocks/doc-block-controls), [WinUI 3 Gallery](https://github.com/microsoft/WinUI-Gallery), [egui widget gallery](https://github.com/emilk/egui/blob/master/crates/egui_demo_lib/src/demo/widget_gallery.rs) |
| B | **Studies.** A few one-screen composites in one fictional domain, on one shared model, beside a catalog of specimens. | A shared model; links to the catalog | Medium | [Flutter Gallery studies](https://dart.googlesource.com/external/github.com/flutter/gallery/+/HEAD/lib/studies), [Material Studies](https://m2.material.io/design/material-studies), [Slint `demos/` beside `examples/gallery`](https://cdn.jsdelivr.net/gh/slint-ui/slint@1.16.0/demos/README.md), the Dear ImGui demo's [Examples menu](https://flutter.googlesource.com/third_party/imgui/+/refs/heads/docking/imgui_demo.cpp), [shadcn/ui examples](https://blog.devgenius.io/shadcn-ui-codebase-analysis-examples-nav-tsx-explained-9302e3cd8a11) |
| C | **One world.** A whole product; every page is a view of it. | Everything is one app | High; a full app | [Apple Backyard Birds](https://github.com/apple/sample-backyard-birds), [Makepad Ironfish](https://docs.rs/crate/makepad-example-ironfish/0.6.0) |
| D | **Live theme panel.** A palette and metrics editor that restyles everything. | Every widget reads the theme you edit | Medium; joins A, B or C | [Radix Themes `ThemePanel`](https://www.radix-ui.com/themes/docs/overview/getting-started), [Material Theme Builder](https://github.com/material-foundation/material-theme-builder), Jetsnack's runtime theme picker ([compose-samples](https://github.com/android/compose-samples)) |

**Pick: B, with D as one of its studies, in an example of its own.** Material describes its
studies as fictional apps that test the system against real product limits; Slint keeps its
use-case demos in `demos/`, apart from its widget gallery. C is the full app the brief rules
out. A connects nothing that a reader feels.

## What the practice says

Each point below is a rule the plan holds to; the batches cite them by title.

- **A global settings strip changes every study.** Flutter Gallery's settings hold the theme
  (system, dark, light), the text scale and a slow-motion switch, and they apply to every study
  ([settings.dart](https://github.com/flutter/gallery/blob/main/lib/pages/settings.dart)). The
  observatory's rail footer holds the scheme, the UI scale and the debug toggles.
- **Each study says what it shows and where its code is.** egui puts a source link under each
  demo; the compose samples list the features each sample shows. Each study's header strip
  names its API (`api!` chips), the showcase pages that show each piece, and its source file.
- **Keyboard first, by the WAI-ARIA patterns.** A data grid moves by arrows, Page Up/Down,
  Home/End and Ctrl+Home/End, and marks its sorted column
  ([APG grid](https://www.w3.org/WAI/ARIA/apg/patterns/grid), [APG table](https://www.w3.org/WAI/ARIA/apg/patterns/table)).
  A modal traps focus and closes on Escape. Every action in a study has a key.
- **Semantic colors, checked in both schemes.** Apple's guidance: name a color for its role, give
  each role a value per appearance, and check each screen in both
  ([WWDC19 "Implementing Dark Mode"](https://developer.apple.com/videos/play/wwdc2019/808/)).
  The observatory reads every color from a palette role; goldens hold each study in both schemes.
- **Content keeps its own look.** A night sky is dark in a light app, as a photo is: the chart
  and its HUD are content and stay night-dark; the chrome around them follows the scheme.
- **An observatory's own third look is red.** Red light keeps the eye's dark adaptation, which
  bright light takes up to 40 minutes to restore; Stellarium's night mode turns the whole UI red
  ([Stellarium Labs](https://www.stellarium-labs.com/blog/nightmode)). The appearance study
  carries a night-vision preset: a custom `Palette` through `Theme::from_parts`, the domain's
  reason for a theme that is neither light nor dark.
- **Deterministic data.** A demo that a golden photographs draws the same each run: a seeded
  generator, no clock, no I/O, no file.

## Two examples

- **`observatory` is a new example; `showcase` stays.** The showcase loses no page, section or
  behavior. Its edits are the ones a palantir API change forces on every caller (the
  `Palette::DEFAULT` → `Palette::DARK` rename, `set_theme` → `set_theme_for` on the focus page)
  and the move of its shot mode to a shared module.
- **Links to the catalog are text.** A study names the showcase page of each piece it uses, as a
  chip cannot open a page in another binary. Refused: "used in" chips on the showcase's pages,
  an edit the showcase does not need.
- **The shot mode is shared, not copied.** Today `examples/showcase/shot/` holds it, and only
  `Shell::with_page` and `UnknownPage` tie it to the showcase. It moves to
  `examples/shared/shot/` (a folder with no `main.rs`, so Cargo finds no example there), behind
  one trait: an `App` that opens on a page by label and lists its labels. Each example compiles
  it through `#[path]`, as `tests/visual` compiles showcase pages. Its tests stay in the
  showcase, so they run once. The observatory gets its own drive command, `shot-observatory`, as
  the driver passes arguments only to a program step.
- **The example is a module tree a test can compile.** `main.rs` holds the command line and the
  window; `app/` holds the rest, so `tests/visual` and `tests/alloc` compile `app` through one
  `#[path]`.
- **Each example keeps its own scaffold.** The observatory's tokens read the live palette; the
  showcase's `support.rs` keeps its constants of the dark palette, so the showcase stays dark.

## The observatory

### Domain

A palantír is a seeing-stone, and darkroom's lumos is astrophotography, so the domain fits the
name and the parent project. It has a real use for each hard part of palantir: a pan-and-zoom
chart, raw wgpu, strokes and gradients, a long keyboard-driven table, a form, a dock, a second
window, a modal, the clipboard, motion. Refused: a mail or task client (text only), a photo
editor (it repeats darkroom), a synth (it needs audio, and it is Ironfish's look), a ground
station (further from palantir, and its plots need a clock).

### Layout

```text
examples/observatory/
  main.rs               the command line, the window, --shot
  app/mod.rs            the App: rail, routing, the eyepiece window
  app/model/            Observatory and its parts, each with its tests
    catalog.rs          the generator, ObjectId, Object
    sky.rs              projection, star color, magnitude sizes
    plan.rs, session.rs, log.rs, appearance.rs
  app/scaffold.rs       tokens from the live palette, the study header, sections
  app/studies/          sky_chart.rs, catalog.rs, session.rs, workspace.rs, appearance.rs
  app/sky.wgsl          the GpuView's shader
  shots/                one shot script per study, each scheme
examples/shared/shot/   the shot mode, moved from the showcase
```

Each study file stands alone and stays under about 400 lines, so a reader can copy one.

### Model

`Observatory` owns all state; each study is `fn(&mut Ui, &mut Observatory)`, the shape a darkroom
view has. Its rules are plain methods with unit tests.

- **Catalog.** 4,000 stars and 60 deep-sky objects, made from a fixed seed by SplitMix64, inline
  (about ten lines; [Steele, Lea and Flood 2014](https://dl.acm.org/doi/10.1145/2714064.2660195)).
  Refused: the `rand` crate, a new dependency whose output a minor version may change, which
  moves every golden. Positions are uniform on the sphere with a bias to a band (the example's
  Milky Way); magnitudes follow the inverse of the star-count law `N(<m) ∝ 10^(0.6 m)` over
  −1 … 6.5, a closed form; names are designations such as `OBS 0412`, so no real catalog is
  copied. `ObjectId` is the index.
- **Star color, derived.** B−V in −0.4 … 2.0 → temperature by Ballesteros' formula
  (`T = 4600 K · (1/(0.92·BV + 1.7) + 1/(0.92·BV + 0.62))`,
  [EPL 97 34008](https://arxiv.org/abs/1201.1809)) → chromaticity by the Kim et al. cubic fit of
  the Planckian locus, valid 1667–25000 K
  ([Planckian locus](https://en.wikipedia.org/wiki/Planckian_locus#Approximation)) → linear
  sRGB, scaled so the largest channel is 1. Refused: a lookup table of star colors, a baked-in
  approximation of the same curve.
- **Star size, derived.** Flux goes as `10^(−0.4 m)` and a disc's area with flux, so the radius
  goes as `10^(−0.2 m)`, clamped to a floor and a ceiling in physical pixels.
- **Selection:** one `Option<ObjectId>`. **Plan:** entries of an object, a filter and a frame
  count; add (no duplicate), remove, move up and down, clear. **Session:** the camera, the
  filter wheel, the exposure, the gain, the cooling and the binning. **Log:** a ring of the last
  200 events, each stamped with a step counter, not a clock.
- **Filter and sort are cached.** The catalog view is a `Vec<ObjectId>` the model rebuilds only
  when the query or the sort changes, so a frame allocates nothing (palantir `AGENTS.md`:
  per-frame allocation is a regression).
- **Appearance:** a `Palette` per scheme, the `Metrics`, the active preset (Stock or Night
  vision). A theme is built through `Theme::from_parts` only when a value changes, never per frame.

### Studies

- **Sky chart** — an atlas chart in RA and Dec (plate carrée) inside a zoomable `Scroll`, so it
  runs on palantir's own pan and zoom. Stars are discs of their color; the bright ones get a
  radial-gradient glow, quantized to 16 color classes so the gradient atlas holds 16 rows, not
  one per star. Asterism lines are strokes; plan entries are rings. Only stars in the visible
  rect are recorded. A hover card at the pointer (a `Popup` at a point, as a star is not a
  widget), a context menu (Add to plan, Copy coordinates, Show in catalog), and a slew: when the
  selection changes, `Ui::animate` eases `Scroll::set_offset` and `Scroll::set_zoom` to the
  target. A frosted HUD (`BackdropFilter::frosted` with `with_max_luminance_for`) shows RA/Dec
  under the pointer and the field of view. Under it all, a `GpuView` draws the sky glow and the
  band from the visible RA/Dec rect, a uniform each frame.
- **Catalog** — a table: Name, Type, Mag, RA (`05h 35m 17s`), Dec (`−05° 23′`), Note. Numbers
  right-aligned in mono, so digits line up. A header click sorts and shows ▲ or ▼ on the sorted
  column. Rows are virtualized: a fixed row height, and only the visible rows recorded between
  two spacers. Keys: ↑ ↓, Page Up/Down, Home/End, Ctrl+F to the search field, Esc clears it,
  Enter selects, F2 edits the note (`EditableLabel::edit`), Ctrl+C copies the row as
  tab-separated text, the menu key or a right click opens the row's menu. Each piece built by
  hand (the table, the virtual rows, the sort header) goes to `.notes/TODO.md` as a widget
  candidate.
- **Session** — the form: camera (`ComboBox`), filter (`ComboBox`), exposure (`DragValue`, s),
  gain (`Slider`), cooling (`Switch`) with a target temperature, binning (radios). The plan as a
  list: Alt+↑/↓ and a drag handle reorder, Delete removes. "Clear plan" opens a `Modal`: focus
  stays in it, Enter confirms, Esc cancels. A readout shows the total integration time.
- **Workspace** — a `DockView` of four panes (chart, catalog, details, log) built from the same
  view fns as the studies, under a `MenuBar` (File: Reset session; View: the scheme as radio
  items, Night vision; Window: Eyepiece F8) and over a status bar (selection, plan count,
  scheme). F8 opens the eyepiece, a second OS window that shows the selection from the same
  model.
- **Appearance** — each palette role of the shown scheme as a color chip with its contrast
  against its ground, computed live (`text on window_bg 15.45:1`), so a reader sees the floors
  L1 tests; the metric steps as drag values; presets Stock and Night vision; Copy as Rust, which
  writes the `Palette` and `Metrics` literals to the clipboard; Reset.

### Connections

- **One selection.** A star in the chart, a row in the catalog, a plan entry: the chart slews,
  the catalog scrolls to the row, the details pane and the eyepiece show it, the status bar
  names it, the log records it.
- **One plan.** Added from the chart or the catalog; edited in the session; drawn as rings on the
  chart.
- **One look.** The scheme switch, the night-vision preset and the appearance editor restyle
  every study and the eyepiece window. Night vision also turns the chart's stars red.

### Rail

The five studies (Ctrl+1 … Ctrl+5), the home on the sky chart. The footer: the scheme switch
(System, Light, Dark, with the system's answer beside System), the UI scale, and the debug
toggles F9, F10 and F12, as the showcase's rail has them.

## Light and dark in palantir

Palantir has one look, `Palette::DEFAULT`, the semantic layer of Ayu Graphite, and reads no
system scheme. Facts and sources for this section:
`.notes/research/2026-10-31-system-preferences.md`.

### What exists elsewhere

- **egui:** `ThemePreference { Dark, Light, System }`, a dark and a light `Style` in `Options`,
  `RawInput::system_theme: Option<Theme>` from winit, and a fallback theme
  ([`ThemePreference`](https://docs.rs/egui/latest/egui/enum.ThemePreference.html)).
- **iced 0.14:** `system::theme()` and `system::theme_changes()`; the app picks its theme;
  `mundy` on Linux behind the optional `linux-theme-detection` feature
  ([`theme_changes`](https://docs.rs/iced/latest/iced/system/fn.theme_changes.html),
  [`iced_winit` manifest](https://docs.rs/crate/iced_winit/0.14.0/source/Cargo.toml.orig)).
- **Zed:** `"theme": { "mode": "system", "light": …, "dark": … }`
  ([Zed appearance](https://zed.dev/docs/appearance)).
- **Material 3's switch** paints its selected handle in `on-primary`, the ink of the accent: dark
  on a light accent in the dark scheme
  ([Material Web switch](https://material-web.dev/components/switch),
  [Flutter's token data](https://flutter.googlesource.com/mirrors/flutter/+/HEAD/dev/tools/gen_defaults/data/switch.json)).

### Where the scheme comes from

- **One reader, `mundy`, on every OS, as a plain dependency of the `winit` feature.** It reads
  the scheme and the double-click time and follows both: the portal's `SettingChanged` on Linux,
  KVO on macOS, `UISettings` on Windows. It replaces the host's own double-click reads, and with
  them palantir's direct `windows` and `objc2-app-kit` dependencies. Refused: `dark-light` 3,
  which reads the scheme alone and polls macOS once a second; `system-theme`, with few users;
  `ashpd`, Linux alone; `zbus` by hand, the same crates with the portal code to own; winit's
  `ThemeChanged`, which reports nothing on X11 or Wayland and, on macOS, nothing while a window's
  appearance is pinned; iced's optional feature, as each Linux app wants the scheme. Cost: 36 new
  crates on Linux (`zbus` 5 and the `async-*` family), 9 on macOS, 7 on Windows.
- **The first frame is in the right scheme.** The host reads once with
  `Preferences::once_blocking` before it opens a window, bounded by a timeout, then subscribes.
  Refused: a subscription alone, whose first value can land after a frame painted in the
  fallback, a flash of the wrong scheme.
- **The subscription runs off the main thread.** Its callback (`Send + Sync`) stores the values
  in shared cells and wakes every window, as `Ui::waker` does. `subscribe` and `once_blocking`
  are called on the main thread, as `mundy` asks.
- **On Linux, no preference reads as light.** The portal's 0 is "No preference", and GNOME
  writes it for its own Light setting; libadwaita reads it as light. `mundy` gives
  `NoPreference` for 0 and for a desktop with no portal alike. Cost: a desktop with no portal
  shows light.
- **No reader at all** (a harness, an offscreen host): the fallback scheme, dark, today's look.
- **The double-click time improves on GNOME** (its setting reads through the portal, where
  `platform/linux.rs` keeps a fixed 400 ms and says the read fails); KDE's own setting is still
  not read, as the portal answers its GSettings value.

### The API (for the go-ahead)

```rust
pub enum ColorScheme { Light, Dark }                  // what a window shows
pub enum SchemePreference { System, Light, Dark }     // the person's pick; serde; default System

impl Palette { pub const DARK: Self; pub const LIGHT: Self; }   // DEFAULT renamed
impl Theme { pub fn stock(scheme: ColorScheme) -> Self; }       // Default = stock(Dark)

impl Ui {
    pub fn theme(&self) -> &Rc<Theme>;                          // the resolved one, as today
    pub fn theme_for(&self, scheme: ColorScheme) -> &Rc<Theme>;
    pub fn set_theme_for(&mut self, scheme: ColorScheme, theme: impl Into<Rc<Theme>>);
    pub fn scheme_preference(&self) -> SchemePreference;
    pub fn set_scheme_preference(&mut self, preference: SchemePreference);
    pub fn color_scheme(&self) -> ColorScheme;                  // resolved
    pub fn system_color_scheme(&self) -> Option<ColorScheme>;   // None: nothing reports it
}
impl Harness { pub fn set_system_color_scheme(&mut self, scheme: Option<ColorScheme>); }
impl OffscreenHost { pub fn set_system_color_scheme(&mut self, scheme: Option<ColorScheme>); }
```

- **`set_theme` is removed** (no compatibility shims); its callers move to `set_theme_for`.
- **Palantir resolves, the app does not.** A manual pick wins, then the system, then the
  fallback. Refused: iced's shape, where each app subscribes and calls `set_theme`.
- **The preference and the system scheme are app-wide; the themes are per window.** They live in
  `UiResources` beside the user scale (`AppSetting`), so a pick in one window changes every
  window. A `Ui` keeps its theme pair, as it keeps its theme today.
- **A frame never mixes two themes.** Each `Ui` resolves its scheme at the start of a frame; a
  change made during a record shows from the next frame, which the setter requests. A change of
  the resolved scheme bumps the theme epoch, so that frame repaints whole.
- **App-owned colors follow `color_scheme()`.** An app that paints its own content keys its own
  palette on it each frame. Refused: `Theme` keeping the `Palette` it was built from, a second
  copy of each color that a hand edit of the theme makes wrong.
- **One look is a preference.** An app with one look pins `SchemePreference::Dark`; darkroom does
  this until ayu-graphite gives it a light table.
- **Window decorations follow the resolved scheme:** winit's `set_theme` on each window colors the
  macOS title bar, Wayland's Adwaita frame and the Windows title bar.

### The palettes

- **New roles.** `on_accent`, the ink on an accent fill: the switch knob paints `p.text` today
  and the check mark `p.window_bg`, which reads right on dark only. `shadow`, the color of a
  float shadow, which `Metrics::float_shadow` holds today as `BLACK` at 50 %, a color in the
  geometry steps; `Metrics` keeps the offset and blur. Role docs lose "brighter": the ladder
  goes further from `window_bg`, up in dark and down in light.
- **The floor test checks the pairs the recipes paint, not every pair.** Text, 4.5:1 (WCAG
  1.4.3): `text` on every surface; `text_muted` on `surround` (inactive tabs, menu titles), on
  `element` (a menu's shortcut), on `element_mid` (a field's placeholder, a hovered row's
  shortcut); `text` on the selection wash; `on_accent` on `accent`. Graphics, 3:1 (1.4.11):
  `border_focused` on `window_bg`, `surround` and `element_mid`. Rungs, 1.10:1 (the step Ayu
  Graphite holds): `window_bg`–`surround`, `surround`–`element_mid`, `element_mid`–
  `element_strong`. The test computes each ratio from the palette, so a value change shows as a
  failed pair.
- **The dark palette fails two of its own pairs today** (computed): `text_muted` `#a4a4a4` on
  `element_mid` `#454545` is 3.85:1, so a field's placeholder and a hovered menu row's shortcut
  miss 4.5:1; and the knob, `p.text` on the accent, is 1.89:1. Ayu Graphite holds its muted
  text to 4.5:1 on its control fill (`gray_3`), but palantir rests controls one rung up, on
  `element_mid` (`gray_4`). L1 fixes both (**Decisions**).
- **The light palette: Ayu Light's grounds and hues, tuned by hand to the floors.** Ayu Light as
  it ships fails them: muted text `#828E9F` on `#F8F9FA` is 3.15:1; its accent `#F29718` on
  white is 2.28:1; its ground and band are 1.03:1 apart. The candidate: text `#3d4248`, muted
  `#5f6873`, ground `#fcfcfc`, band `#eff1f3`, elements `#ffffff` / `#e4e7ea` / `#d6dadf`, accent
  `#a66509` (Ayu's orange darkened at its hue until white on it clears 4.5:1: 4.68:1), white
  `on_accent`. It passes every pair but one rung, `surround`–`element_mid` at 1.096:1, which L1
  tunes. In light, `element` is the raised white of a menu, a tooltip and a field's disabled
  fill, set apart by its border and shadow, not by a rung.
- **The light shadow** starts from Ayu Light's popup shadow (`#6B7D8F` at 10 %) and is tuned in
  the shots.

### Found on the way, not in this scope

- **Frosted glass has a ceiling and no floor.** `BackdropFilter::with_max_luminance_for` keeps a
  light ink readable on dark glass and asserts the tint is darker than the ink's ground; dark ink
  on light glass has no counterpart. The observatory's HUD stays night-dark, so it does not need
  one. Goes to `.notes/TODO.md` with L1.
- **Reduced motion is ignored.** The portal on this machine answers `reduced-motion 1`, and
  palantir animates regardless. Batch L4 proposes it.

## Decisions

- **Studies over a catalog, in a separate example; the showcase stays whole.** Refused: a
  catalog with knobs, which connects nothing a reader feels; one world app, which the brief rules
  out; studies inside the showcase, which moved and removed its pages.
- **One domain, the observatory.** Refused: the domains in **Domain**, and a domain per study as
  Flutter does, which shares no model and so connects nothing.
- **The appearance study restyles the whole example**, as Radix Themes' `ThemePanel` does.
  Refused: a preview beside the editor, which connects nothing.
- **The sky is a `GpuView` shader under palantir shapes**, so raw wgpu has a real job. Refused:
  shapes alone, which leaves the GPU view an island.
- **The chart is an atlas in a `Scroll`**, so it runs on palantir's pan and zoom. Refused: a
  stereographic projection around a moving center, the planetarium's view, which a 2-D scroll
  cannot hold and which would bypass the API the study shows.
- **The light palette is tuned by hand in palantir**, its source in the doc comment, the floor
  test its guard. Refused: a light variant in the ayu-graphite repo, which puts the work in a
  second repo whose solver assumes a dark ladder.
- **The light accent is Ayu Light's orange, `#a66509`.** Refused: blue in both schemes, as macOS,
  Windows and GNOME keep the hue. Cost: two accent hues, so an app's own accent reads
  `ui.color_scheme()`.
- **`mundy` reads the system on every OS, with no feature of its own**, and replaces the
  double-click reads. Reasons in **Where the scheme comes from**.

- **The dark palette's muted text rises to `#b2b2b2`**: 4.52:1 on `element_mid`, and text over
  muted stays 1.78:1 apart. The border ladder and the scrollbar thumbs derive from `text_muted`
  at an alpha, so they brighten a little too; darkroom builds its own `Palette` and does not
  move. Refused: resting controls on `element` as Ayu Graphite does, which
  darkens every control and moves every widget golden; leaving the two pairs under 4.5:1.
- **`on_accent` is dark in the dark scheme, `#141414`**, 8.16:1 on the accent, as Material 3
  paints its selected handle in `on-primary`. Today's light knob turns dark; the check mark
  already wears this ink. Refused: a light knob, 1.89:1, under the 3:1 a graphic needs.

## Plan

Order: L1 → L2 → L3, then 1 → 7; L4 when its go-ahead comes. Each batch ends green on
`cargo drive gate`, regenerates `.notes/API_SURFACE.md` when the surface moves, and, for a change
a person can see, ends with `cargo drive run shot` before and after. A batch that changes
palantir's API also migrates darkroom (palantir is its submodule) and runs darkroom's gate.

### L1. Palette roles and the light palette
- [ ] `Palette::DEFAULT` → `Palette::DARK` at its 40 uses; `Palette` keeps no `Default`.
- [ ] Roles `on_accent` and `shadow`; `Metrics::float_shadow` becomes offset and blur; the
      switch knob, the check mark and every float shadow read the roles; role docs say "further
      from `window_bg`".
- [ ] `Palette::DARK`: `text_muted` `#b2b2b2`, `on_accent` `#141414`, `shadow` black at 50 %
      (today's value).
- [ ] `Palette::LIGHT`, tuned until the floor test passes; its doc comment cites Ayu Light.
- [ ] The floor test (`theme/tests/floors.rs`): one table of ink, ground and floor per palette,
      each ratio computed; it names each failed pair.
- [ ] `Theme::stock(ColorScheme)`; `Theme::default()` is `stock(Dark)`.
- [ ] Goldens: the visual suite on the old tree first, then the changed knob and shadows
      rewritten; a light golden of the widgets' still states (`Theme::stock(Light)`).
- [ ] darkroom: `palantir_bridge` fills `on_accent` and `shadow` (darkroom's palette has a
      `shadow` role); its metrics set the float shadow's geometry.
- [ ] `.notes/TODO.md`: frosted glass with dark ink has no contrast floor.

### L2. The scheme on `Ui` and the hosts
- [ ] `ColorScheme`, `SchemePreference` (serde, default `System`).
- [ ] `UiResources` holds the preference and the system scheme as `AppSetting`s; each `Ui`
      holds a theme per scheme, both `Theme::stock` at first.
- [ ] The resolution at frame start; the theme epoch bumps when the resolved scheme changes; a
      setter requests a frame.
- [ ] `theme_for`, `set_theme_for`, `scheme_preference`, `set_scheme_preference`,
      `color_scheme`, `system_color_scheme`; `set_theme` removed and its callers moved.
- [ ] winit host: each window's decorations follow the resolved scheme.
- [ ] `Harness::set_system_color_scheme`, `OffscreenHost::set_system_color_scheme`.
- [ ] Tests: the resolution table (3 preferences × 3 system answers → the scheme); a pick in one
      window resolves in the other; a change during a record shows from the next frame, and
      that frame repaints whole; the epoch does not move when the resolved scheme does not.
- [ ] darkroom: `set_theme_for(Dark, …)` and `set_scheme_preference(Dark)` where it set the
      theme.

### L3. System preferences through `mundy`
- [ ] `mundy` (`color-scheme`, `double-click-interval`, `callback`, `async-io`; no default
      features) under the `winit` feature.
- [ ] At start, on the main thread: `once_blocking` with a timeout, before the first window;
      then `subscribe`, whose callback writes the shared cells and wakes the windows. The
      timeout is set from a measured read on each OS.
- [ ] Linux reads `NoPreference` as light; macOS and Windows map their two answers.
- [ ] The double-click time from `mundy`; remove the platform files' reads and the `windows`
      and `objc2-app-kit` dependencies; the platform files keep the event-loop and app-id code.
- [ ] The shot mode's header line `scheme light|dark` sets the harness's system scheme.
- [ ] Check on each OS by hand: a system switch with the example open repaints every window in
      the new scheme within a frame; note the callback thread in the research file.
- [ ] Measure the gate's time before and after; note it in the research file.

### L4. Reduced motion (proposed; its own go-ahead)
- [ ] `mundy`'s `reduced-motion`; `Ui::prefers_reduced_motion()`; `Theme::stock` and the
      transitions drop their animation while it is on, as CSS `prefers-reduced-motion` asks.

### 1. The example, its model and the shared shot mode
- [ ] `[[example]] observatory` (`winit` required, `test = true`); the first test lane names
      `--example observatory`; drive command `shot-observatory`.
- [ ] Move the shot mode to `examples/shared/shot/` behind the page trait; the showcase compiles
      it through `#[path]`, its shot tests and every script pass unchanged.
- [ ] `app/model/`: the generator, star color and size, plan, session, log, the cached catalog
      view. Tests with exact values: the first objects of the seed; the magnitude law at its
      ends; Ballesteros' temperature and the Planckian chromaticity at hand-computed points; the
      radius law; RA/Dec text at its edges (`23h 59m 59.6s` rounds to `00h 00m 00s`, `−00° 00′`
      for a small negative Dec); filter and sort by exactly which ids survive in which order;
      each plan operation, a duplicate add included.
- [ ] `app/scaffold.rs` and the rail; the study header (title, blurb, keys, `api!` chips, the
      showcase pages, the source path); Ctrl+1 … 5.

### 2. Catalog study
- [ ] The table: header and sort marks, virtual rows, the keys of **Catalog**, the notes, copy,
      the row menu.
- [ ] Harness tests: Page Down moves by the visible row count; End selects the last row and
      scrolls it into view; a sort keeps the selection; Ctrl+C puts the row's exact text on the
      clipboard; the virtual rows record only the visible range plus one.
- [ ] `.notes/TODO.md`: the widget candidates the study built by hand.

### 3. Sky chart study
- [ ] The atlas in a zoomable `Scroll`: discs, the 16 glow classes, lines, plan rings, the
      visible-rect cull.
- [ ] The hover card, the context menu, the eased slew through `set_offset` and `set_zoom`.
- [ ] `app/sky.wgsl` in a `GpuView`, its uniform the visible RA/Dec rect; the frosted HUD.
- [ ] Harness tests: a click selects the nearest star within its radius; a selection made
      elsewhere ends, after a settle, with the star at the viewport's center; the cull records
      no star outside the rect.

### 4. Session study
- [ ] The form, the plan list and its keys, the modal, the readout.
- [ ] Harness tests: Alt+↓ moves an entry down; Delete removes it; the modal's Esc keeps the plan
      and Enter clears it; the readout's total equals the sum the model computes.

### 5. Workspace study
- [ ] The dock of the four panes, the menu bar, the status bar, the eyepiece window (F8).
- [ ] Harness tests: a catalog selection in its pane moves the chart pane; F8 records an open
      command for the eyepiece; the View menu's scheme items set the preference.

### 6. Appearance study
- [ ] The role chips with live contrast, the metric drag values, the presets (Stock, Night
      vision), Copy as Rust, Reset; a theme rebuilt only on a change.
- [ ] The rail's scheme switch, the system's answer beside System.
- [ ] Tests: Copy as Rust of the stock palettes gives source text that names `Palette::DARK`'s
      and `Palette::LIGHT`'s exact values; night vision sets the dark slot and pins Dark, and
      Reset restores both.

### 7. Finish
- [ ] Shot scripts in `examples/observatory/shots/`, one per study and scheme; a test parses
      each, so a script cannot rot.
- [ ] Goldens: each study in both schemes, through `#[path]` into `app`.
- [ ] `tests/alloc`: the catalog, session and chart studies record ten frames after warmup with
      zero allocations.
- [ ] README: a section and the recording for the observatory
      (`cargo run --profile release-dev --example observatory`); the showcase's section and its
      custom-widget link stay.
- [ ] palantir `AGENTS.md`: the lasting decisions fold in (the examples, the scheme, the
      palettes, `mundy`); this proposal is deleted.
