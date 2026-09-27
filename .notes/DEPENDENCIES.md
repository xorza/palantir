# Dependencies

**The goal: a smaller graph with no loss of function.** A crate stays when it
does a hard job well. A crate goes when the project uses a small part of it,
when it only duplicates a crate that is already in the graph, or when a
feature pulls in code that nothing calls.

## How the numbers were measured

- The graph is the `catcad` binary for `x86_64-unknown-linux-gnu`: normal and
  build edges, default features, from `cargo tree`. Dev-dependencies are not in
  it. `cargo metadata` gives a larger count, because it joins the features of
  all builds (`golden` pulls `image` and `rayon` into its count).
- "Exclusive" is the set of crates that go out of the graph when one node goes
  out. It is exact for a removal and says nothing about a crate that another
  path also reaches.
- "CPU" is the sum of the `cargo build --timings` unit durations of a clean dev
  build on the `13980HX` (`opt-level = 3` for dependencies). A clean build takes
  50 s wall and 473 s CPU. The CPU figure is the one to compare. The wall
  figure hides most of a crate behind parallel units.
- The feature experiments ran on a copy of the repository. The counts after an
  arrow (`203 → 197`) are measured, not estimated.

## Where the graph comes from

The binary builds **203 crates**.

| node | exclusive crates | what they are |
| --- | --- | --- |
| `palantir` | 101 | everything below that sits under palantir |
| `wgpu` | 32 | naga, the HAL, ash, glow, gpu-allocator |
| `winit` | 29 | calloop, sctk, sctk-adwaita, x11-dl, xkbcommon |
| `resvg` | 22 | usvg, tiny-skia 0.12, the PNG and deflate chain |
| `cosmic-text` | 16 | fontdb, fontconfig-parser, harfrust, skrifa 0.40 |
| `tracing-subscriber` | 7 | regex-automata, regex-syntax, matchers, sharded-slab |
| `swash` | 6 | skrifa 0.44, read-fonts 0.41, zeno, yazi |
| `silverpoint` | 5 | dashu-int, dashu-ratio, num-modular |
| `strum` | 3 | strum_macros, heck |

The heaviest single crates by CPU are `x11rb-protocol` (25.7 s), `ash` (16.0 s),
`naga` (15.7 s), and the two copies of `read-fonts` (12.5 s and 10.2 s).

## Duplicate versions

Fourteen crates build twice. Each crate is at its latest release on crates.io,
so a version bump removes none of the duplicates.

| crate | versions | cause |
| --- | --- | --- |
| `skrifa`, `read-fonts`, `font-types` | 0.40 / 0.44 and their bases | `cosmic-text` pins `skrifa ^0.40`, `swash` uses 0.44 |
| `tiny-skia`, `tiny-skia-path` | 0.11 / 0.12 | `sctk-adwaita` (winit CSD) / `resvg` |
| `miniz_oxide` | 0.8 / 0.9 | `png` and `flate2`, both under `resvg` |
| `pollster` | 0.4 / 1.0 | `rfd` / palantir |
| `rustix`, `linux-raw-sys`, `thiserror` | 0.38 / 1.1, 1 / 2 | `calloop 0.13` and `sctk` under winit 0.30 |
| `smol_str` | 0.2 / 0.3 | winit / cosmic-text |
| `roxmltree` | 0.20 / 0.21 | `fontconfig-parser` / `usvg` |
| `hashbrown`, `rustc-hash` | 0.16 / 0.17, 1 / 2 | inside wgpu and naga |
| `syn` | 2 / 3 | `bytemuck_derive` and `palantir-anim-derive` use 3, all others use 2 |

Proposals 1 and 4 remove the `tiny-skia` pair and the font pair. The winit
duplicates stay until a winit release after 0.30. The wgpu and `syn` duplicates
are out of our control.

## Proposals

Each proposal gives the crates it removes, the CPU it saves, and what it costs.
The order is by gain against cost, and each tier is independent of the next.

### Tier 1 — feature changes and one small module

#### 1. winit: no Adwaita window decorations

Palantir takes winit with default features, and the default includes
`wayland-csd-adwaita`. Replace the dependency with:

```toml
winit = { version = "0.30", optional = true, default-features = false, features = ["rwh_06", "x11", "wayland", "wayland-dlopen"] }
```

- **Removes 6 crates (203 → 197), 8.9 s CPU:** `sctk-adwaita`, `ab_glyph`,
  `ab_glyph_rasterizer`, `owned_ttf_parser`, and the 0.11 copies of `tiny-skia`
  and `tiny-skia-path`.
- **Cost:** a compositor that does not give server-side decorations (Mutter,
  GNOME) then shows sctk's `FallbackFrame`, which is a plain frame with no title
  text. KDE and wlroots compositors draw their own decorations and show no
  change.
- **The better answer for GNOME** is a title bar that palantir draws itself,
  with `with_decorations(false)`. Palantir is a GUI toolkit and already has all
  the parts. This gives one look on every compositor, not Adwaita on one of
  them.

#### 2. Logging: a subscriber of our own in `catcad`

`catcad` uses `tracing-subscriber` with `env-filter` for one job: print events
to stderr, filtered by target and level from `RUST_LOG`. The code opens no
`tracing` spans. The only `span!` calls are `tracy_client` zones.

**The present setup also loses messages.** Nineteen crates in the graph log
through `log`, not `tracing`: `wgpu`, `wgpu-core`, `wgpu-hal`, `naga`,
`cosmic-text`, `resvg` and others. The subscriber is built without the
`tracing-log` feature, so no `log` record reaches the terminal. A wgpu
validation message is lost in the same way as a font warning.

The proposal is a `logging` module in `catcad` of about 150 lines:

- A `tracing::Subscriber` for events only. It filters in `register_callsite` and
  `max_level_hint`, so a disabled call site costs what it costs now. It writes
  `LEVEL target: message field=value` with a `tracing::field::Visit`.
- A `log::Log` on the same filter and the same writer. Then wgpu and the font
  stack print again.
- A `RUST_LOG` parser for the `target=level,level` grammar that `main.rs`
  documents. A directive that does not parse makes a warning. It is not ignored.

- **Removes 7 crates, 16.3 s CPU:** `tracing-subscriber`, `regex-automata`,
  `regex-syntax`, `matchers`, `sharded-slab`, `thread_local`, `lazy_static`.
- **Cost:** EnvFilter's span-field directives and regex matching go. No code
  uses them. `log` becomes a direct dependency of `catcad`. It is already in
  the graph, so the count does not increase, but the manifest gets a new entry.
- **A smaller step** is `filter::Targets` in place of `EnvFilter`, plus the
  `tracing-log` feature. That removes 3 crates (measured: 191 → 188, in the same
  experiment as proposal 3), adds `tracing-log`, and also fixes the lost
  messages.

### Tier 2 — small crates that the code can hold itself

Each of these crates does a small, well-defined job in a few places. Code of our
own removes a crate and its proc-macro build, and the tests can check its exact
behaviour.

| crate | use in palantir | replacement | removes | CPU |
| --- | --- | --- | --- | --- |
| `strum` | 3 derives: `EnumCount`, `VariantArray`, `IntoStaticStr` | hand-written impls, or one derive in `palantir-anim-derive` | 3 (`strum`, `strum_macros`, `heck`) | 7.3 s |
| `soa-rs` | `#[derive(Soars)]` on 3 render records | a hand-written struct of `Vec`s per record | 2 (`soa-rs`, `soa-rs-derive`) | 4.7 s |
| `padding-struct` | 3 GPU records | explicit `_pad` fields | 1 | 2.1 s |
| `fixedbitset` | 2 bit sets | a `Vec<u64>` bit set | 1 | 0.6 s |
| `tinyvec` | `ArrayVec` in 8 places, with serde | an `ArrayVec<T: Copy + Default, N>` | 1 | 0.5 s |
| `pollster` | 2 `block_on` calls in `requested_gpu.rs` | a `block_on` on `std::task::Wake` and `thread::park` | 1 (the 1.0 copy) | 0.1 s |

- **Removes 9 crates, 15.2 s CPU** in total.
- `padding-struct` needs no replacement. `bytemuck`'s `Pod` derive already
  refuses a struct with implicit padding, so an incorrect hand-written layout
  does not compile.
- `palantir-anim-derive` is on `syn 3`, the same as `bytemuck_derive`. A derive
  that moves into it adds no `syn` build. `syn 2` stays in the graph, because
  `serde_derive`, `thiserror-impl` and `zerocopy-derive` need it.
- `strum`, `soa-rs` and `padding-struct` give most of the gain. `fixedbitset`,
  `tinyvec` and `pollster` give almost no CPU gain. Replace them only if you
  want the code in the repository.

### Tier 3 — larger work

#### 3. X11 as a feature of palantir

Winit's `x11` feature and `arboard` bring the largest single cost in the graph:
`x11rb-protocol` (25.7 s), `x11-dl` (8.4 s) and `x11rb` (2.4 s).

- Give palantir `x11` and `wayland` features that pass to winit. An application
  then selects the backends that it ships.
- `arboard` with `default-features = false` is **X11 only on Linux**. It keeps
  `x11rb` in a Wayland-only build, and in a Wayland session it reaches the
  clipboard through XWayland. A Wayland-only build needs a different
  clipboard: `smithay-clipboard` uses the `sctk` stack that winit already
  brings. The alternative, `arboard/wayland-data-control`, brings
  `wl-clipboard-rs` and its own graph.
- **Measured:** Wayland-only with `arboard` gives 197 → 195. Without `arboard`,
  it gives 195 → 191. The net is −5 crates and about 37 s CPU when
  `smithay-clipboard` replaces `arboard`.
- **Cost:** a new dependency (`smithay-clipboard`) that needs your approval,
  and a build with no X11 support. Keep `x11` in palantir's default features,
  and let CatCad select Wayland only if it ships only for Wayland.

#### 4. `swash` out, own glyph rasterizer on `skrifa 0.40`

Palantir rasterizes glyphs with `swash`, and `swash` brings a second copy of
the font-parsing stack. `cosmic-text` already brings `skrifa 0.40` for
shaping, and `skrifa` gives each part that palantir takes from `swash`:

| palantir uses from `swash` | `skrifa 0.40` has |
| --- | --- |
| hinted outlines at a size | `OutlineGlyphCollection`, `HintingInstance` |
| `wght` variations | `Location` from the font's axes |
| `Source::ColorOutline` (COLR) | `color::ColorGlyphCollection` and a paint callback |
| `Source::ColorBitmap` (CBDT, sbix) | `bitmap::BitmapStrikes`, which returns PNG bytes |
| subpixel offset and synthetic italic | an affine transform on the outline |

The missing part is a coverage rasterizer. An analytic signed-area
accumulator (the `font-rs` method) is about 300 lines and gives exact
coverage, not a sampled approximation. COLRv0 is layers of solid fills, so it
is a composite of those coverages. COLRv1 gradients need more work. Colour
bitmaps need a PNG decoder. The `png` crate is in the graph now through
`resvg`, but proposal 5 removes it.

- **Removes 6 crates, 28.8 s CPU:** `swash`, `zeno`, `yazi`, `skrifa 0.44`,
  `read-fonts 0.41`, `font-types 0.12`. This removes the largest duplicate in the
  graph.
- **Cost:** the largest rewrite in this list, and the text path must stay the
  same. Palantir's golden images of text are the test. Hinting must match
  `swash` pixel for pixel, or the goldens change for a reason we can explain.
  Colour-emoji bitmaps then need their own PNG decode.
- **The alternative** is to wait until `cosmic-text` moves to the `skrifa` that
  `swash` uses. Nothing upstream shows this now.

### Upstream changes

#### 5. `resvg`: PNG only with `raster-images`

`resvg 0.48` takes `tiny-skia` with its default features, and those include
`png-format`. The library needs PNG decode only in `mod raster_images`, which
is behind `#[cfg(feature = "raster-images")]`. The `resvg` binary needs PNG
encode, but it has `required-features`. Palantir turns `raster-images` off.
All the same, it builds `png`, `flate2`, `fdeflate`, two `miniz_oxide`,
`crc32fast`, `adler2` and `simd-adler32`.

- A pull request to `resvg` that gives `tiny-skia` `default-features = false,
  features = ["std", "simd"]` and adds `tiny-skia/png-format` to
  `raster-images` **removes 8 crates and 10.2 s CPU** from each user that turns
  `raster-images` off.
- **Cost:** our time on a pull request and a release that we do not control.

## What to keep, and why

| crate | why it stays |
| --- | --- |
| `wgpu` | The GPU layer. `renderdoc` is forced by `wgpu` on every non-wasm target. `gles` (glow, khronos-egl: 4.2 s) is the fallback for a machine with no Vulkan. |
| `resvg`, `usvg` | Full SVG, filters included, and `svg_facts` reads the filters. A subset of our own gives less function. Proposal 5 removes the part we do not use. |
| `cosmic-text` | Shaping, bidi, line breaks and font fallback. Our own shaper does not give the same quality. `default-features = false, features = ["std", "fontconfig"]` stops the build of `SwashCache`, which palantir never calls, but it removes no crate. |
| `dashu-*` | Exact rational arithmetic for the kernel. An arbitrary-precision rational of our own is a correctness risk and gives no gain. |
| `serde`, `ron`, `glam`, `bytemuck`, `half` | Each does its job in full, and the whole codebase uses them. |
| `etagere` | A tested atlas allocator. An allocator of our own adds bugs and removes only `euclid`. |
| `rustc-hash`, `memchr`, `unicode-linebreak`, `unicode-segmentation` | Other crates in the graph also use them, so a replacement removes no crate. |
| `rfd` | Its only exclusive crate is `pollster 0.4`. The portal dialog is the correct way to open a file on Wayland. |

## Totals

| step | crates | CPU of clean build |
| --- | --- | --- |
| now | 203 | 473 s |
| tier 1 (proposals 1, 2) | −13 | −25.2 s |
| tier 2 (table) | −9 | −15.2 s |
| proposal 4 (`swash`) | −6 | −28.8 s |
| **in-repository total** | **175** | **−69.2 s (−15 %)** |
| proposal 3 (X11 as a feature, Wayland-only build) | −5 | about −37 s |
| proposal 5 (upstream `resvg`) | −8 | −10.2 s |

The CPU figures add unit durations. The wall time of a clean build goes down
by less, because the build runs units in parallel. The longest chain (`syn`,
`serde_derive`, `wgpu-core`, `palantir`, `catcad`) sets the wall time, and
these proposals do not change that chain.

Palantir holds proposals 1, 3, 4 and the tier 2 table, so they go into the
submodule. Proposal 2 is in `catcad`.
