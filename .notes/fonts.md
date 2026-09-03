# Custom fonts

Research notes and a proposal for app-provided fonts and OS fonts in
palantir. The README lists "Italic + app-facing font loading" as a known
gap. Nothing here is committed.

## What exists today

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

## Proposal

### 1. `FontFamily` becomes an interned family name

`FontFamily(u16)`: `Copy`, an index into a process-wide, append-only name
table (`static RwLock<Vec<&'static str>>`; `TextEpoch::reserve` already
keeps a static counter). Seeded with `FontFamily::SANS = 0` → `"Inter"` and
`FontFamily::MONO = 1` → `"JetBrains Mono"`. `FontFamily::named("Segoe
UI")` interns (cold path, one lock). `name(self) -> &'static str` feeds
`Family::Name` in `attrs_for`. Serde writes the name and reads by
interning, so a theme file says `family: "Inter"`.

`TextStyle` stays `Copy`. `GlyphFont` stays 12 bytes — a `u16` fits the
padding after the two bytes. `ShapeRecord` stays 88.

Why not a name hash: a hash cannot become a name again for serialization,
and 8 bytes would grow every text record and key. Why not a registry on the
shaper: `TextStyle` deserializes with no shaper in reach.

**Resolution rule.** A family with no face in the database resolves to
`SANS` and warns once (`tracing::warn!`). Never cosmic's platform fallback
— the look must not depend on what the machine has installed.
`Ui::font_available(family) -> bool` lets an app pick deterministically,
the way Godot's `font_names` and Zed's default stack do.

### 2. Registration on `Ui`, like icons and images

```rust
pub enum FontSource {
    Static(&'static [u8]),
    Bytes(Arc<[u8]>),
    File(PathBuf),
}

impl Ui {
    pub fn load_font(&self, source: FontSource) -> Result<FontFamily, FontLoadError>;
    pub fn font_available(&self, family: FontFamily) -> bool;
    /// Every family the database knows, system fonts included.
    pub fn font_families(&self) -> impl Iterator<Item = FontFamily> + '_;
}
```

`FontLoadError::{Io, NoFaces}` — the bytes are untrusted, so this is a
`Result`. The return is the family of the first face. A collection loads
every face, and each of its families becomes reachable through
`FontFamily::named`. There is no unload: fonts are process-lifetime, and
the atlas keys on `font_id`. `FontFamily` is `Copy` and always valid, so no
RAII owner is needed — this is the one place it differs from `IconSet`.

The registry lives on `CosmicMeasure`, which owns the `FontSystem` and the
shaped caches. `Ui` reaches it through `UiResources.text` → `TextShaper`.
Standalone recorders get `TextShaper::load_font`.

**Invalidation.** `load_font` goes through `db_mut()` (cosmic clears its
match cache) and bumps a `font_epoch` on the shaper. `CosmicMeasure` drops
every shaped buffer. The renderer's encoded-run cache compares the epoch
on its next prepare and clears. Atlas entries stay valid. One frame of
re-shaping after a load is the whole cost, and loads are cold events.

### 3. System fonts as an explicit policy, off the main thread

```rust
pub enum FontSources { Bundled, System }
```

`HostConfig.fonts: FontSources`, default `System` for the winit host.
`TextShaper::with_sources(FontSources)` for standalone recorders and
tests, default `Bundled` — deterministic metrics, 6 µs.

`System` spawns the `fontdb::Database` scan on a thread at the top of
`WinitRuntime::new`, before `create_window` and `GpuInit`, and joins it
when `HostCore` is built. `Database` is `Send`. On a hot cache the GPU
init is the longer of the two, so the scan costs no wall time. On a cold
cache the window appears when the scan finishes, which is what happens
today.

**Warm-up.** After construction, call `get_font_matches` for every
registered family at the weights and styles the theme uses. Cost 2 moves
from the first frame to startup. Cost 3 (first script fallback, 56 ms
here) stays a known spike. A later phase can pre-warm by locale.

### 4. Weight and style axes

`FontWeight(u16)` with `THIN … BLACK` consts (`REGULAR = 400`, `BOLD =
700`), serialized as the number. New `FontStyle { Normal, Italic }` on
`GlyphFont`, `TextStyle`, and `Shape::text`. Compile in the two italic
files already in `assets/fonts` (+1.2 MB). cosmic instantiates `wght` on
the variable faces. For a face with no italic, `CacheKeyFlags::FAKE_ITALIC`
exists — verify cosmic applies it on a style mismatch, otherwise set it in
`attrs_for`.

`TextShapeKey` stays 24 bytes: `family_q: u16` plus one `u16` that packs
weight (10 bits), style (1), `halign` (2), `fit` (2).

### 5. Fallback list — later

`FontConfig { fallback: Vec<String>, forbidden: Vec<String> }` becomes a
custom `Fallback` impl: app names first, then the platform list (Zed's
merge rule). The trait wants `&'static str`, so the names leak once at
construction.

### 6. OS index instead of a scan — later, needs a dependency

`fontique` answers "which file holds family X" through the OS index
without parsing 774 files. The path then goes into fontdb as
`Source::File`. For an app that names one or two OS fonts this replaces
the scan entirely. It is a new crate, so it waits for a go-ahead and is not
in the plan below.

## Plan

### Phase 1 — interned families and registration

1. `src/text/font_family.rs`: `FontFamily(u16)`, the name table, `SANS` /
   `MONO`, `named`, `name`, serde by name. The enum leaves `text/mod.rs`.
2. `TextShapeKey.family_q: u16`; the tag asserts in `key.rs`; the pins in
   `hot_struct_sizes.rs` stay at 24 / 88.
3. `attrs_for` resolves through the availability check. Unavailable →
   `SANS` and one warning per family (a `FixedBitSet` on `CosmicMeasure`).
4. `FontSource`; `FontLoadError` in `src/text/error.rs`;
   `CosmicMeasure::load_font`, `TextShaper::load_font`, `Ui::load_font`,
   `Ui::font_available`.
5. `font_epoch` on the shaper, the shaped-buffer drop in `CosmicMeasure`,
   the encoded-run clear in `renderer/backend/text`.
6. Tests, extending `text/tests/wrap.rs` on its existing fixture:
   - `resolved_family` on a loaded third font. The Inter Italic bytes are
     the fixture, so no new asset.
   - unknown family → `"Inter"`, and `font_available == false`.
   - key round-trip for `family_q` at 0, 1, 2 and `u16::MAX`.
   - serde: `"Inter"` ↔ `SANS`, an unknown name interns and round-trips.
   - late load: shape, load, shape again — the second run resolves to the
     new face.
   - the `ShapeRecord::Text` hash table in `scene/shapes/hash.rs` gains the
     family axis.
7. Darkroom: `FontFamily::Mono` → `FontFamily::MONO`. Mechanical.
8. README: drop "no arbitrary font registration yet".

### Phase 2 — system fonts as policy

1. `FontSources` on `HostConfig`; `TextShaper::with_sources`.
2. The scan thread in `WinitRuntime::new`; join before `HostCore::new`.
3. Warm-up of match keys for every registered family.
4. `Ui::font_families()` for a preferences picker.
5. Tests build `Bundled`. Measure the suite-time drop.

### Phase 3 — weight and italic

1. `FontWeight(u16)`, `FontStyle`, the bundled italics,
   `TextStyle::italic()`, the packed key bits.
2. Tests: widths at 300 < 400 < 700 on Inter; italic resolves to a face
   whose `post_script_name` contains `Italic`; darkroom `FontWeight::Bold`
   → `BOLD`.

### Phase 4 — fallback config, fontique proposal

## Open questions

- The name table is the crate's first process-wide mutable static. It is
  append-only and cold, but it is a global.
- `load_font` from a record pass of any window goes through the shared
  shaper's `RefCell`. Fine today; note it if a window ever shapes on
  another thread.
- On wasm `load_system_fonts` is compiled out, so `Bundled` is the only
  policy there. `System` should be a no-op, not a panic.
- Whether darkroom wants a font preference at all, and so whether
  `font_families()` ships in phase 2 or waits.
