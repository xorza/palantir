# Custom fonts

Research notes and a proposal for app-provided fonts and OS fonts in
palantir. The README lists "Italic + app-facing font loading" as a known
gap. Nothing here is committed.

## What existed before this

Two variable fonts are compiled in with `include_bytes!`
(`src/text/cosmic/mod.rs:70`): Inter (875 KB) and JetBrains Mono (303 KB).
The italic files for both sit in `assets/fonts` but are not compiled in.

The face vocabulary is two `#[repr(u8)]` enums in `src/text/mod.rs`:
`FontFamily { Sans, Mono }` and `FontWeight { Regular, Bold }`. They travel
as `family_q: u8` / `weight_q: u8` inside `TextShapeKey` (24 bytes) and as
two bytes of `GlyphFont` (12 bytes) inside `ShapeRecord::Text` (88 bytes).
Both sizes are pinned in `src/hot_struct_sizes.rs`. `attrs_for` maps the
tags to `Family::Name("Inter")` / `Family::Name("JetBrains Mono")` and
`Weight::BOLD`.

`CosmicMeasure::with_bundled_fonts` calls `FontSystem::new_with_fonts`, and
that constructor **already scans every system font** through
`fontdb::Database::load_system_fonts`. The OS fonts are in the database
today. Only the API cannot name them. `TextShaper::new()` runs on the main
thread in `WinitRuntime::new` (`src/host/winit/runtime.rs:71`), after the
GPU is initialised.

Three things already hold for any number of fonts:

- The glyph atlas key is cosmic's `CacheKey` — `font_id`, glyph, size,
  weight, flags (`GlyphRasterKey`). Two fonts cannot collide.
- `rasterize_glyph` handles `SwashContent::Color`, so a colour emoji face
  works once it is reachable.
- `fontdb::ID` is a slotmap key. It is never reused unless `remove_face`
  is called, which nothing does.

Two things constrain the design:

- `TextStyle` is `Copy` and derives serde with a `try_from` check. `Theme`
  round-trips through RON (`examples/dump_theme.rs`). A family must stay a
  small `Copy` value and still serialize to something a theme file can say.
- `Ui::load_icons(Rc<IconTable>) -> IconSet` and
  `Ui::register_image(Image) -> Result<ImageHandle, _>` are the two
  registration idioms. Names resolve at the app boundary
  (`IconSet::by_name`); the hot path carries integer ids.

### Measured on this machine

774 faces, 376 MB under `/usr/share/fonts`, hot disk cache, release build,
cosmic-text 0.19.0. The bench is a 60-line binary against the crate alone.

| step                                   | system scan on | bundled only |
|----------------------------------------|----------------|--------------|
| `FontSystem` construction              | 14.8 ms (30.9 ms debug) | 6 µs |
| first shape of "Hello" in Inter        | 5.7 ms         | 55 µs        |
| first shape with CJK + Arabic text     | 56 ms          | 80 µs (tofu) |
| unknown family `"No Such Family"`      | Noto Sans, silently | Inter   |
| `load_font_file` of one file, later    | 29 µs          | —            |

fontdb's own numbers: 1906 faces load in ~20 ms hot and ~860 ms cold.
iced issue #2455 reports multi-second debug startups on a machine with
7 GB of fonts.

So there are three separate costs, not one:

1. **The scan.** Startup only. Hidden by a thread, or replaced by an OS
   index query.
2. **First match per `Attrs`.** `get_font_matches` builds a `FontMatchKey`
   for every face in the database the first time a family/weight/style
   combination is shaped. O(faces). Warmable at startup.
3. **First fallback per script.** Loads the fallback faces and matches
   again. A mid-session frame spike the first time CJK or Arabic text
   appears.

Every test that builds a shaper through `TextShaper::new()` or
`TextSystem::cosmic()` pays cost 1 and inherits the machine's fallback
fonts. There are ~70 such sites.

## Prior art

### CSS `@font-face` — the reference model

A family **name** is the unit of identity. `src` is an ordered source list:
`local("Name")` checks the OS, `url()` loads bytes. `font-weight: 100 900`,
`font-style`, `font-stretch` describe what a face covers, including ranges
for variable fonts. The `font-family` list on an element resolves
**per character**: first family with the glyph, then the generic
(`sans-serif`). Metric overrides (`ascent-override`, `size-adjust`) keep a
fallback face on the same baseline.

### egui

`FontDefinitions { font_data: BTreeMap<String, Arc<FontData>>, families:
BTreeMap<FontFamily, Vec<String>> }`. `FontFamily::{Proportional,
Monospace, Name(Arc<str>)}`. Each family is an ordered fallback list of
font names. `ctx.set_fonts(defs)` replaces the whole set. No system fonts.
`FontTweak { scale, y_offset_factor, y_offset, hinting }` per font. Names
live on the style path and are hashed per galley.

### iced

`Settings { fonts: Vec<Cow<'static, [u8]>>, default_font: Font }`.
`Font { family: Family::Name(&'static str) | SansSerif | Monospace | …,
weight: Weight(u16), stretch, style }`. `font::load(bytes)` at runtime calls
`db_mut().load_font_source` and bumps a `Version`; every text cache keys on
that version. The `FontSystem` is a global `OnceLock<RwLock<_>>` built with
`new_with_fonts`, so the eager scan is there too (issue #2455 is open).

### GPUI / Zed

`TextSystem::add_fonts(Vec<Cow<'static, [u8]>>)`. `Font { family:
SharedString, weight, style, features, fallbacks }`.
`resolve_font(&Font) -> FontId` falls back to the default stack when a
family fails. Results cache in `font_ids_by_family_cache`. Settings name
fonts by string: `buffer_font_family`, `buffer_font_fallbacks` (merged with
the platform list), plus the special names `.ZedSans`, `.ZedMono`,
`.SystemUIFont` for bundled and platform defaults. On the cosmic backend a
family with no matching face is an error at resolve time, never a silent
substitute.

### Slint

`import "x.ttf"` in a `.slint` file bakes the bytes in.
`register_font_from_memory` / `register_font_from_path` at runtime.
`default-font-family` on the window. Slint moved from fontdb to fontique
(`sharedfontique.rs`, `CollectionOptions { system_fonts: true }`), with
`SLINT_DEFAULT_FONT` and `SLINT_FONT_PATH` as environment overrides.

### fontique (parley)

Enumerates through the OS index — DirectWrite, CoreText, libfontconfig via
`dlopen` — instead of parsing font files. `Collection::register_fonts(Blob)
-> Vec<(FamilyId, Vec<FontInfo>)>`. `GenericFamily`, script and locale
fallback, `Query` with `Attributes { weight, width, style }`, and a
`Synthesis` answer (fake bold / italic) when the exact face is missing. It
cannot be cosmic-text's database, but it can hand a path to
`fontdb::Source::File`.

### Dear ImGui, Godot, Qt, Flutter

- ImGui: `AddFontFromFileTTF` / `AddFontFromMemoryTTF`, `MergeMode` to fold
  an icon font into a family, `PushFont(font, size)` at runtime since 1.92.
- Godot: `SystemFont { font_names: [..], font_weight, font_stretch,
  font_italic, allow_system_fallback, fallbacks }`. The first available
  name wins. Style matching is documented as "not guaranteed".
- Qt: `QFontDatabase::addApplicationFont(path)` /
  `addApplicationFontFromData(bytes) -> int`, `applicationFontFamilies(id)`,
  `removeApplicationFont(id)`, and since 6.8
  `addApplicationFallbackFontFamily(script, family)`.
- Flutter: `pubspec` fonts, `FontLoader(family).addFont(bytes).load()`,
  `TextStyle.fontFamilyFallback: [..]`.

### cosmic-text 0.19 itself

`FontSystem::new_with_locale_and_db_and_fallback(locale, db, impl
Fallback)`. `Fallback { common_fallback, forbidden_fallback,
script_fallback(script, locale) }` is fixed at construction. `db_mut()`
clears the match cache. An unknown `Family::Name` resolves through
`db.query` and then the platform list — silently. `CacheKeyFlags` has
`FAKE_ITALIC` and `DISABLE_HINTING`; palantir sets the second.

### What to take from it

- Identity is a family name. The hot path carries an integer id. A resolve
  step sits between them (Zed, Slint, every CSS engine).
- Sources are bytes, a file, or an OS name. Registration is additive and
  permanent — only Qt removes, and egui replaces the whole set.
- A missing family never fails shaping. It resolves to a declared default
  (CSS generic, Zed's default stack). The app can ask whether it resolved.
- System fonts are a policy, not a given. The scan is hidden or replaced.
- A load invalidates shaped caches (iced's `Version`).
- Weight is numeric 100–900. Italic is a separate axis. Both are matched,
  with synthesis when the face is missing.

## What shipped

Implemented. The sections below are what the code does, and where it
differs from the proposal this note opened with.

### 1. `FontFamily` is an interned family name

`FontFamily(u16)` in `src/text/font_family.rs`: `Copy`, an index into a
process-wide append-only `RwLock<Vec<&'static str>>` seeded with
`SANS = 0` → `"Inter"` and `MONO = 1` → `"JetBrains Mono"`. `named`
interns and leaks; `name` reads back. Serde carries the name, so a theme
file says `family: "Inter"`.

**Resolution.** A family with no face resolves to `SANS` and warns once
(`CosmicMeasure::resolve_family`), never to cosmic's platform fallback.
The memo is a `Vec<Option<FamilyResolution>>` indexed by the family's own
index — dense, so a shape costs one bounds check and no hash, and the
resolved `&'static str` means the name table's lock is never taken on the
shaping path.

`Ui::font_available` reads the **same** memo entry, which is why the entry
carries an `available` flag beside the name rather than the name alone:
`SANS` shapes under its own name whether or not a face answers to it,
since it is what everything else falls back to, so the name cannot answer
availability for the one family it matters most for. Sharing the entry
also keeps a query an immediate-mode app makes inside a record pass off a
per-frame walk of every face.

### 2. Registration on `Ui`

`Ui::load_font(impl Into<FontSource>) -> Result<FontFamily, FontLoadError>`,
`Ui::font_available`, `Ui::font_families`. `FontSource::{Bytes(Cow<'static,
[u8]>), File(PathBuf)}` with `From` impls for a slice, an `include_bytes!`
array, a `Vec`, a `Cow`, a `PathBuf`, a `&Path` and a `&str` path.

Two corrections to the proposal:

- **fontdb keeps `Source::File`, not `SharedFile`.** `load_font_file`
  memory-maps the file to parse its face table and then stores the path,
  re-mapping on the rare later read of the face data. So the file arm goes
  through `load_font_source(Source::File(path))`, which does the same thing
  and *returns the ids* — `load_font_file` returns none, and recovering
  them by position in the database would depend on slotmap iteration order.
  The path is opened once first, only so an unreadable file reports
  `Io` rather than "no faces".
- **`font_families` returns a `Vec`, not an iterator.** The database sits
  behind the shaper's `RefCell`, so a lending iterator would hold that
  borrow across the caller's whole walk.

**Invalidation.** `load_font` goes through `db_mut()` (cosmic clears its
match cache), drops every shaped buffer and ellipsis memo, clears the
resolution memo, and bumps a `font_epoch`. The encoded-run cache compares
that epoch **before** it emits a batch and clears when it moved — after
would mean the frame had already painted from stale templates. The epoch
lives in a `Cell` beside the shaper's `RefCell`, not inside it: an all-hit
frame is contracted never to crack that borrow, and the text backend's GPU
suite holds an exclusive borrow across a prepare to prove it.

### 3. System fonts as a policy, off the main thread

`FontScope { Bundled, System }` on `WinitHostConfig.fonts` (default
`System`) and `TextShaper::with_fonts`. `TextShaper::new()` is now
`Bundled` — deterministic metrics and 6 µs, which is what every test,
bench and standalone recorder wanted.

Named `FontScope` rather than the proposal's `FontSources`, which sat one
letter from `FontSource` and meant something else entirely.

**`FontSystem` is `Send`**, so `FontScan` builds the whole thing on a
thread rather than only the `fontdb::Database` — which keeps cosmic's own
locale detection instead of reimplementing it, and moves the monospace
scan and the match warm-up off the main thread too. `WinitRuntime::new`
spawns it before `create_window` and joins it at `HostCore::new`.

**Warm-up is bounded to the families in use** — the bundled pair at
startup, and whatever the app has already drawn after a load. Warming
every *interned* family would be O(faces) times several hundred once
`font_families` has interned the machine's whole list.

Two RTL cases (`geometry`, `truncate`) now ask for `FontScope::System`
explicitly. They shape Arabic and Hebrew, which no bundled face covers, so
they always depended on the machine's fonts — silently, until the default
changed.

### 4. Weight and style axes

`FontWeight(u16)` on the CSS 1–1000 scale with the nine named steps,
range-checked in `new` and in serde. `FontStyle { Normal, Italic }`. All
four bundled files are compiled in, upright and italic.

Both axes reach every authoring surface the family and weight already
did: `TextStyle::{with_style, italic}`, `Shape::text(..).style(..)`, and
`Text::italic()` beside `Text::bold()` as the one-axis hatch over the
resolved bundle.

**Fake italic is cosmic's own**: `override_fake_italic` sets
`CacheKeyFlags::FAKE_ITALIC` when the matched face is upright and the
request was not, and the flag is part of the glyph cache key. `attrs_for`
sets nothing.

**The key needed the packing immediately.** A `u16` family beside four
separate bytes is 25 bytes and pads to 32, so phases 1 and 3 landed
together: `TextShapeKey` carries `family_q: u16` plus a `FaceBits(u16)`
holding weight (10 bits), style (1), halign (3) and fit (2). The bound
half is the contiguous top five bits, which is what `WrapBound` rewrites.
`TextShapeKey` stays 24 bytes and `ShapeRecord` 88.

`AnimRow<AnimatedLook>` moved 472 → 488: a numeric weight plus a style no
longer fit the two bytes a pair of two-variant enums used, so each of the
row's `TextStyle`s carries four more.

### 5. Fallback list — still later

`FontConfig { fallback: Vec<String>, forbidden: Vec<String> }` as a custom
`Fallback` impl: app names first, then the platform list (Zed's merge
rule). The trait wants `&'static str`, so the names leak once at
construction.

### 6. OS index instead of a scan — still later, needs a dependency

`fontique` answers "which file holds family X" through the OS index
without parsing 774 files. The path then goes into fontdb as
`Source::File`. For an app that names one or two OS fonts this replaces
the scan entirely. It is a new crate, so it waits for a go-ahead.

## Open questions

- The name table is the crate's first process-wide mutable static. It is
  append-only and cold, but it is a global.
- `load_font` from a record pass of any window goes through the shared
  shaper's `RefCell`. Fine today; note it if a window ever shapes on
  another thread.
- On wasm `load_system_fonts` is compiled out and `std::thread::spawn` is
  not available, so neither `FontScope::System` nor `FontScan` can work
  there. The crate has no wasm target today; when it gets one, `System`
  should degrade to `Bundled` rather than panic.
- Whether darkroom wants a font preference at all, and so whether
  `font_families()` earns its keep.
