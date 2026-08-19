# SVG icons — proposal and implementation plan

Status: **shipped**. The runtime and the polish pass (§7, slices 1 and 3) are
built, tested, and rendering. **Slice 2 — the build-time baker — is dropped**:
`IconAtlas::from_svgs` derives the same metadata at load, and
`IconAtlas::baked` is there for anyone who later wants a generated `const`, so
the baker buys a parse at startup and nothing else.

**Bake SVGs at build time into a normalized, compiled-in blob; rasterize each
icon at its exact physical pixel size on first use; cache in a glyph-style
atlas.** Pixel-exact at every display scale and zoom level, full SVG fidelity
(colour, gradients, clips, filters), and the atlas machinery already exists —
it is what draws the text.

Locked by decision: separate **`IconHandle`** (not an `ImageHandle` arm);
separate **atlas instance** from the glyph atlas; **resvg** as the runtime
rasterizer; **filters allowed** with auto-prewarm; raster sizes **exact below
64 px, 4 px grid above**. The sub-region `ImageHandle` from the superseded
pixel-atlas plan is dropped.

---

## 1. What the target is

Real toolbar icons — a save diskette, a new-file sheet, a folder — with
multiple colours, gradients, and soft edges. Not monochrome line icons.

That rules out the two cheap paths. A build-time pixel atlas can't be
pixel-exact at 1.5× display scale. A hand-rolled compositor over a coverage
rasterizer (`zeno`, already in the tree via `cosmic-text → swash`) handles flat
fills and could be taught gradients, but isolated groups, nested clips, and
luminance masks are where a from-scratch SVG compositor goes wrong — and
that's a rendering project, not a slice.

So the icon pixels come from **resvg**, the reference Rust SVG renderer, at the
exact size asked for. Everything else in this document is about getting those
pixels onto the screen through machinery palantir already owns.

---

## M. Measured

A scratch crate against resvg 0.48.1 / tiny-skia 0.12, on this machine
(i9-13980HX, 32 threads). Three fixtures: `assets/logo/palantir-mark.svg`
(radial + linear gradient, luminance mask), a diskette icon (linear gradient,
clip path, group opacity), and a shadowed folder (Gaussian-blur filter,
radial gradient, even-odd fill).

**Round-trip through `usvg::Tree::to_string` is lossless.** Renders of the
original tree and the re-parsed normalized tree are **byte-identical** at 16,
24, 32, 64 and 256 px on all three — 0 differing pixels, max channel delta 0,
against non-blank output (30 680 opaque px on the logo at 256). Gradients,
masks, clip paths, filters, group opacity and even-odd fills all survive. §4's
format is safe; the baker should still assert it per icon, since this is three
fixtures and not a proof.

**Rasterization cost per icon** — the number that decides how icons behave
under zoom:

| Fixture | 16 px | 24 px | 32 px | 48 px |
| --- | --- | --- | --- | --- |
| diskette (gradient + clip) | 13 µs | 15 µs | 26 µs | 38 µs |
| logo (gradients + mask) | 25 µs | 37 µs | 48 µs | 72 µs |
| **shadowed (Gaussian filter)** | **71 µs** | **188 µs** | **293 µs** | **689 µs** |

Filters are a 10–20× cliff and grow superlinearly with size. Everything else is
cheap enough to rasterize inline on a cache miss.

**Dependency cost**, stripped binary against an empty control, clean build:

| Runtime | New crates | Binary | Clean build |
| --- | --- | --- | --- |
| `resvg` (default-features off) | 23 | **+1 161 KiB** | 2.8 s |
| `tiny-skia` alone (no `png-format`) | 4 | **+207 KiB** | ~1 s |

Eight of resvg's 31 crates are already in palantir's tree (bytemuck, log,
memchr, bitflags, smallvec, arrayvec, cfg-if, roxmltree). Seven of the 23 new
ones are a PNG codec (png, flate2, miniz_oxide, fdeflate, simd-adler32, adler2,
crc32fast) that tiny-skia pulls through its default `png-format` feature and
that resvg does not disable — dead weight we cannot turn off from downstream.

**resvg's own renderer, minus filters and the CLI, is 671 lines**
(`render` 183, `path` 205, `lib` 109, `clip` 98, `mask` 46, `geom` 30).
Filters are a further 2 896. That is the size of what a tiny-skia-only backend
would have to own.

---

## 2. Shape of the design

An icon is a text glyph that came from an SVG. Every stage mirrors one that
exists:

| Stage | Text today | Icons |
| --- | --- | --- |
| Authoring | `Shape::text(…)` | `Shape::icon(handle)` → `IconShape` (`at`, `fit`, `tint`) |
| Record | `ShapeRecord::Text` | `ShapeRecord::Icon { local_rect, handle, fit, tint }` — 33 B payload, under the 75 B `Image` variant that sets the 88-byte pin, so **`ShapeRecord` does not grow** |
| Encode | `DrawTextPayload` | `DrawIconPayload { rect, handle, tint }` |
| Compose | `TextDrawRow { origin, bounds, colour, snapped scale }` | `IconDrawRow { key, origin, colour }` — the physical box is resolved and **rounded to whole pixels here**. No `bounds`: an icon is one quad, so the group's own scissor is the whole of its clipping |
| Backend prepare | glyph walk → `atlas.touch` / `atlas.insert`, swash raster on miss | `atlas.touch` / `atlas.insert`, **resvg raster on miss** |
| Draw | glyph pipeline + atlas bind group | same shader and pipeline layout, icon atlas bind group |

**Reused verbatim:** the 20-byte `RasterQuad`, `raster_atlas/shader.wgsl` with
both its mask and colour paths, `ContentType`, the atlas's clock-sweep
eviction, growth-with-blit, batched staging uploads, `ExpiryWheel`,
`DynamicBuffer`, and the composer's tier-conflict flush machinery.

An icon quad *is* a glyph quad, so the instance type, the shader, the vertex
layout, and the group-0 bind-group shape live in `raster_atlas::quad` — with
the atlas both passes read through, rather than inside whichever pass needed
them first.

**New:** the atlas generalizes over its key; the icon set and its baker; the
resvg raster wrapper; `PaintTier::Icon`; the authoring types.

### 2.1 Handle

The handle owns nothing and is `Copy`, so the per-frame path never touches a
refcount — what keeps a set alive is the `IconSet` the app holds:

```rust
/// 12 bytes, `Copy`. No `Rc`, no clone at the call site, no RAII lifetime.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconHandle { icon: IconRef, view_box: Vec2 }
```

It carries `view_box` so resolving `IconFit` at encode time needs no registry
lookup: the aspect ratio travels with the handle rather than being fetched on
the hot path. `IconRef` — the `(set, icon)` pair alone — is what the atlas
caches against.

`IconShape` has three knobs and all of them mean something. This is why it is
not an `ImageShape`: on a vector source `min_filter`, `mag_filter`,
`downsample`, `Cover` and `Tile` are all no-ops, and shipping a builder where
half the chain silently does nothing is worse than shipping two builders.

### 2.2 Atlas

`GlyphAtlas` becomes generic over its key — the key appears in exactly four
places (`cache`, `slot_keys`, `unallocated_expiry`, eviction) — and is
instantiated twice. Shared: the shader, the pipeline layout, every allocation
and eviction path. Not shared: the textures, the bind group, the eviction
budget.

Icons start their colour side at 512² (emoji default is 256²; colour icons are
the common case here, not the rare one) and the mask side at 256².

`PaintTier::Icon` joins Mesh / Image / Curve, so paint-order correctness comes
free from the existing conflict-flush logic in `composer/higher_kind.rs` —
three match arms and one array entry.

### 2.3 Why this is pixel-exact

The glyph sampler is `Nearest` on both axes and quads are emitted at exactly
the raster's pixel dimensions. Icons inherit that contract: the raster box is
rounded to whole physical pixels at compose time and drawn 1:1. Nothing is ever
resampled, at any display scale, under any zoom.

The cost is that an icon's on-screen box quantizes to whole physical pixels —
a 24.6 px box draws at 25 px. Glyphs already take that deal.

### 2.4 Cache key and quantization

```rust
struct IconRasterKey { icon: IconRef, size: U16Vec2 }   // 8 bytes
```

No subpixel bins — icons snap to integer positions, unlike glyphs. Compare
cosmic's 24-byte `CacheKey`.

Text snaps *scale* to a 0.5 % ladder so a zoom gesture doesn't re-rasterize
every glyph every frame. Icons snap **size**, on a two-part ladder:

- **≤ 64 px: exact integer pixels.** This is where a pixel of size error shows
  and where rasters are cheap — 13–72 µs measured (§M), so the churn a zoom
  gesture creates here is affordable.
- **> 64 px: round to a 4 px grid.** Raster cost climbs with area, so this is
  where churn hurts; a 4 px grid cuts the rasters a continuous zoom mints by 4×
  for at most 3 % of size error, which is invisible at that size.

Without a ladder, 40 icons under a continuous zoom cross a rung together and
cost ~1.2 ms of rasterization on those frames — 14 % of a 120 Hz budget, and
worse the further you zoom. The ladder puts the ceiling where the cost is.

`MAX_ICON_RASTER_PX` (512) keeps a zoomed canvas from asking for a 4096 px
raster; past it the largest cached raster is reused and magnifies. Atlas-full
falls back to the `AtlasFull` path glyphs already use — re-encode each frame,
never a missing icon.

---

## 3. Colour, tint, and the two atlas sides

The glyph shader already has both paths, and they map onto the two kinds of
icon exactly:

| Icon kind | Atlas side | Tint semantics |
| --- | --- | --- |
| **Colour** (gradients, multiple fills) | colour, straight sRGB RGBA | alpha only — `vec4(s.rgb * s.a, s.a) * tint.a`. Fades for disabled/ghost states; does not recolour. `IconShape::desaturate` collapses it to its own Rec. 709 luminance, which is the other half of a disabled look |
| **Tintable** (one paint, or `currentColor`) | mask, R8 coverage | full — `tint.rgb * coverage`. One icon serves every theme colour |

The baker classifies each icon and records a `tintable` flag; both kinds live
in one set and one atlas.

Two consequences worth designing around:

- **Hover / pressed states on colour icons** can't be a tint. **Decision: the
  background chip carries the state**, which is what real toolbars do — the
  icon itself does not change. **Disabled is `desaturate` plus an alpha step**,
  shipped: the flag rides one spare bit of `RasterQuad::uv_and_kind`, directly
  above `u` and below the content type, so it costs no instance lane and one
  `select` in the colour path. `quad.rs` derives the whole layout — and the
  three numbers the WGSL needs — from a single `U_BITS`, and substitutes them
  into the shader through `shader_template` so the two cannot drift.
- **`tiny_skia::Pixmap` is premultiplied sRGB**; the colour side wants straight
  (the shader premultiplies in linear). Demultiply before upload —
  `Pixmap::demultiply` — which is also the better colour pipeline.

SVG gradients interpolate in sRGB, and resvg honours that. Palantir's own
gradients are linear-RGB. That divergence is correct: an icon should look like
it does in the designer's tool, not like a palantir gradient.

---

## 4. Baked format

usvg does the hard part at build time, so the baker is thin: parse, let usvg
resolve `use`, transforms, CSS, basic shapes, and text-to-paths, then write the
normalized tree back out. The runtime never sees a font or an unresolved
reference.

```rust
pub struct IconAtlas<'a> {
    /// Sorted by name — `by_name` binary-searches, no map, no allocation.
    pub icons: &'a [IconDef],
    /// Every icon's normalized SVG, concatenated.
    pub svg: &'a [u8],
}
pub struct IconDef {
    pub name: &'static str,
    pub view_box: Vec2,   // nominal logical size
    pub svg: Range<u32>,  // slice of IconAtlas::svg
    pub tintable: bool,
    /// Uses an SVG filter — 10–20× the raster cost (§M), so this one is
    /// prewarmed rather than rasterized on the frame that first shows it.
    pub filtered: bool,
}
```

Generated `icons.rs` holds the table plus `include_bytes!("icons.svgblob")`,
wrapped in a `const fn baked(..)` call that borrows both — so a compiled-in set
copies and parses nothing at startup — and one `IconId` constant per icon
(`icons::SAVE`). Slice 2 adds a `FORMAT_VERSION` constant and the generated
`const _: () = assert!(..)` that reads it, so a stale generated file is a
compile error rather than a runtime surprise — it is not in the crate yet,
since a version nobody checks is worse than none.

Size, measured (§M): normalizing makes an icon **bigger**, not smaller —
+63 % to +83 % on the three fixtures, since usvg expands shorthand and
synthesizes a full `<defs>`. Absolute numbers stay trivial either way: ~1.1–1.9 KB
per icon normalized, so a 40-icon set is ~45–75 KB against ~30 KB raw. What
normalization buys is a **2.5–3× faster parse** and a runtime that never needs a
font. Both are worth more than 30 KB, so bake normalized — but the honest
reason is parse time and font-freedom, not size.

Parsing is **lazy, per icon, on first raster** (`usvg::Tree` cached in the set),
so startup pays nothing for icons the session never draws.

---

## 5. API

```rust
// startup — no GPU work, no parsing; registers the set and mints its id
let icons: IconSet = ui.load_icons(Rc::new(my_icons::atlas()));

// per frame
ui.widget(Node::leaf().size(icons.nominal(my_icons::SAVE)))
  .record(ui, None, |ui| ui.add_shape(icons.shape(my_icons::SAVE)));
```

- `IconSet::shape(id) -> IconShape` — the draw-site call; the same as
  `Shape::icon(set.handle(id))`, which stays available for a handle held
  across frames.
- `IconSet::nominal(id) -> Vec2` — the viewBox size, for sizing the node.
- `IconSet::by_name(&str) -> Option<IconId>` — binary search over the
  name-sorted table; the generated constant is the compile-checked path.
- `IconShape`: `at(rect)`, `fit(IconFit)` — `Contain` (default, aspect
  preserved) / `Fill` / `None` — and `tint(color)`, per §3.

A set is held behind an `Rc` and `load_icons` keys on that allocation, so
loading every frame from a handle the app holds costs one refcount bump and
adds no second entry — the immediate-mode-friendly behaviour, without a
per-frame rebuild.

**Before the baker exists**, `IconAtlas::from_svgs([(name, svg), …])` builds a
set at runtime, deriving each icon's viewBox, tintability, and filter use by
parsing it — the same classification `bake-icons` will do at build time, so the
two cannot drift into different answers for one icon. It owns its buffers and
frees with the last `IconSet` holding it; the only thing `baked` buys over it is
skipping the parse and the copy.

---

## 6. The rasterizer seam

The whole resvg dependency sits behind one function:

```rust
fn rasterize(icon: &IconSource, px: UVec2, out: &mut Vec<u8>) -> RasterKind
```

`out` is a retained scratch buffer wrapped as a `tiny_skia::PixmapMut`, so
steady-state rasterization allocates nothing after warmup. Swapping resvg for a
leaner backend later changes this function and nothing else — not the API, not
the atlas, not the record, not the shader.

That matters because the leaner backend is a real option if the dependency
weight ever bites: bake a binary display list and render it with **tiny-skia
alone** (drops ~10 crates and the XML parse, costs us the group/clip/mask
tree-walk and filters — a few hundred lines we would own). Recommending against
it now, for the reason in §1, and recording it as the diet.

**First-frame cost, and prewarm.** An unfiltered colour icon at 32² takes
13–48 µs, so a toolbar of 40 rasterizes in 1–2 ms on the frame that first shows
it — the same shape as a cold glyph atlas, and left lazy.

Filtered icons are 10–20× that (§M), which is a dropped frame rather than a
hitch, so **they prewarm**: at `load_icons` and again on any display-scale
change, every icon the baker flagged `filtered` is rasterized at
`view_box × scale` before the frame path sees it. Two honest limits — a filtered
icon drawn at some *other* size still pays inline, and prewarm rasterizes icons
the session may never draw. Both are why only the flagged ones prewarm.

---

## 7. Implementation slices

**Slice 1 — the runtime. Done.** `GlyphAtlas` became `RasterAtlas<K>` and
moved to `renderer::backend::raster_atlas`, with `ContentType`, the
per-instance label/initial-size config, and the shared draw program
(`quad.rs`: `RasterQuad`, the shader, the vertex layout, the bind-group
helpers) alongside; text keys it on its cosmic `CacheKey` and icons on
`IconRasterKey`. `src/icons/` holds the baked types,
the registry, the size ladder, and the resvg wrapper;
`renderer::backend::icon` holds the pass, which reuses the glyph shader,
`RasterQuad`, and the vertex layout verbatim. `ShapeRecord::Icon` →
`DrawIconPayload` → `IconDrawRow` → `PaintTier::Icon` → `RenderStep::IconBatch`.
Driven from hand-written `IconAtlas` values, as planned.

The pins held: `ShapeRecord` is still **88 bytes** — the whole point of putting
`view_box` on the handle and keeping the record's payload at 41. `Ui` grew by
exactly 8, the registry's `Rc`.

**Slice 2 — the baker.** `bake-icons` crate: usvg parse (system fonts on, text
→ paths) → `Tree::to_string` → concatenated blob + generated `icons.rs` +
optional `--manifest`. Deterministic (sorted by name) so the output is
reviewable and committable. Classify `tintable` and `filtered`. Two build-time
checks worth having, both nearly free because the baker already has a renderer
in hand: **assert the round-trip** per icon (render original vs re-parsed
normalized, require byte equality — the §M property, verified rather than
assumed) and **report measured raster µs** per icon at 16/24/32/48 px, so a
gratuitously expensive icon is visible in the build log rather than in a
profile. Bundle a small MIT colour set in `assets/icons/` (add the licence to
`Cargo.toml`'s `license` field, as the bundled fonts already do); add the
showcase page.

**Slice 2 — the baker. Dropped.** `IconAtlas::from_svgs` runs the same
classification at load that a baker would have run at build time, sharing one
implementation (`SvgFacts`) so the two could not have disagreed anyway. What a
baker would still buy is skipping the parse and the copy at startup —
`IconAtlas::baked` accepts exactly that shape whenever someone wants to
generate it.

**Slice 3 — polish. Done.**

- **Desaturate**, shipped as above, pinned by a visual fixture against
  hand-computed luminance greys (`#e63c3c` → 125, `#3c78e6` → 124).
- **Eviction budgets are per instance now** (`RasterAtlasConfig::max_bytes` /
  `eager_growth_bytes`) rather than two tenants sharing one constant. The
  values land the same for both, which is the honest result rather than a
  tuning win: 16 MiB caps the icon colour side at 2048², about 450 icons at
  48² and far past any plausible working set, and a larger budget would only
  matter under a zoom deep enough that `MAX_ICON_RASTER_PX` has already bound
  the raster. The point is that changing text's number no longer moves icons'.
- **Folding the two atlases: measured, and don't.** A labelled toolbar of
  eight icon+label buttons composes to **one icon batch and eight text
  batches** — every icon closes the open text batch because icons are a higher
  paint tier. Merging the atlases would not recover those eight: the split is
  the tier boundary, not the second texture, and a control test shows eight
  *images* between the same labels split text identically. What the separate
  atlas actually costs is **one draw call**. Removing the splits would mean
  making icons a text-tier participant sharing the batch stream, with an
  ordering rule between icon and text rows inside one batch — a far larger
  change than the one draw it saves.

**Testing (slice 1, in place).** Runtime: the size ladder, hand-computed — 24 logical px at 1.5
scale → exactly 36 (exact rung); 50 logical px at 1.5 scale → 75 → 76 (4 px
grid, and the rung *above* 64 that a naive `round` would miss); prewarm
rasterizes every `filtered` icon and nothing else; cache hit on redraw, miss on
resize, LRU reclaim; both
atlas sides exercised by the two inline fixtures; `hot_struct_sizes` pin
unchanged at 88; alloc test — 200 icons/frame allocates zero after warmup, and
a resize allocates only atlas staging growth. Baker: determinism, `tintable`
classification, viewBox → `nominal`. Visual: an icon grid at 12/16/24/48 px at
display scales 1.0 / 1.5 / 2.0 — those goldens are the pixel-exactness claim,
and the gradient icon is what proves colour fidelity.

---

## 8. Dependency ask

Runtime, in palantir itself:

| Crate | Why |
| --- | --- |
| `resvg` 0.48, `default-features = false` | The renderer. Dropping defaults drops text and raster-image support — the baker has already outlined text, so no fonts at runtime |
| `usvg` 0.48, `default-features = false` | Pulled by resvg; parses the normalized blob. No `text` feature → no fontdb, harfrust, skrifa, or unicode tables |
| `tiny-skia` 0.12 | Pulled by resvg; the actual rasterizer |

Measured (§M): **23 new crates, +1 161 KiB stripped, 2.8 s clean build.** All
MIT/Apache-2.0, all linebender-maintained. Seven of the 23 are a PNG codec
palantir will never call, pulled in by tiny-skia's default `png-format` feature
through resvg — worth an upstream issue, not a blocker.

The measured alternative is tiny-skia alone: **4 new crates, +207 KiB**, at the
cost of owning resvg's 671-line renderer and losing filters.

Baker only: `usvg` with the `text` + write features (fonts and `xmlwriter`).

**Approved.** The tiny-skia-only alternative stays documented as the diet: if
the binary cost ever bites, it changes `rasterize()` and nothing else — not the
API, not the atlas, not the record, not the shader.

---

## 9. Why this shape (survey)

| Toolkit | Approach |
| --- | --- |
| **Zed / GPUI** | Rasterize SVG on CPU at the **exact device size**, cache in the same bin-packed atlas as glyphs. Raster scale derived from the active transform, quantized and clamped so the atlas can't explode. Alpha-only — monochrome icons only |
| **Chromium** | `.icon` files — SVG converted at build time to a compact path array compiled into the binary; Skia rasterizes at the requested size; `kFooBarIcon` constants generated |
| **Slint** | Compile-time embedding, rendered to a texture at viewBox size or higher |
| **egui / iced** | resvg at runtime, cached per size — the closest prior art to this proposal |
| **Flutter / Material** | Icon fonts. Monochrome only; multi-colour needs stacked glyphs |
| **Games / TextMeshPro** | MSDF — crisp at any zoom, own shader, monochrome, and pure-Rust generation from arbitrary contours is immature |

Half the field (Flutter, Zed, MSDF) is monochrome-only and therefore off the
table given §1. Of what remains, this is **Chromium's build step feeding
egui's runtime, through Zed's atlas**: normalize and generate constants at
build time, rasterize at the exact device size on demand, cache in a
glyph-style atlas with LRU eviction — which palantir already owns for text.

Sources: [Zed — Leveraging Rust and the GPU](https://zed.dev/blog/videogame),
[Chromium vector_icons README](https://chromium.googlesource.com/chromium/src/+/HEAD/components/vector_icons/README.md),
[Slint resource embedding](https://docs.slint.dev/latest/docs/cpp/cmake-reference/resource-embedding/),
[egui_extras image loaders](https://docs.rs/egui_extras/latest/egui_extras/loaders/fn.install_image_loaders.html),
[iced_tiny_skia raster cache](https://docs.iced.rs/src/iced_tiny_skia/raster.rs.html),
[resvg](https://crates.io/crates/resvg), [usvg](https://docs.rs/usvg/latest/usvg/),
[zeno](https://docs.rs/zeno/latest/zeno/).

---

## 10. Settled, and what is left

Settled: separate `IconHandle` and `IconShape` (§2.1); separate atlas instance
(§2.2); resvg at runtime (§8); filters allowed with prewarm (§6); the two-part
size ladder (§2.4); background chip for hover/pressed, desaturate bit reserved
(§3); sub-region `ImageHandle` dropped; a bundled demo set in `assets/icons/`.

Left, and both need code before they can be answered:

1. **Icon atlas eviction constants.** The glyph atlas's 16 MiB ceiling and
   4 MiB eager-growth budget were measured against 1-byte masks; a 32² colour
   icon is 4 KB. Start at the same numbers, measure in slice 1, retune in
   slice 3.
2. **Whether the icon atlas should fold into the glyph atlas.** Worth one draw
   call on a labelled toolbar. Decide with a profile, not in advance.
