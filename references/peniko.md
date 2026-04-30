# peniko — reference notes for Palantir

Peniko is Linebender's "paint vocabulary" crate — the shared types Vello, Parley, and Floem all speak when describing *what* to paint (as opposed to kurbo's *where*). The name is Esperanto for "brush", which is the central type. It's `no_std`, deliberately tiny, and exists specifically so that downstream renderers don't each invent their own `Color`/`Brush`/`Gradient`.

All paths under `tmp/peniko/peniko/src/`.

## 1. The `Brush` enum — unified paint payload

`Brush` (`brush.rs:15`) is a three-way enum:

```rust
pub enum Brush<I = ImageBrush, G = Gradient> {
    Solid(AlphaColor<Srgb>),
    Gradient(G),
    Image(I),
}
```

That's the entire paint surface. Everywhere a renderer accepts "what color/pattern to fill or stroke with", it takes `impl Into<BrushRef>`. The two generic params are the trick — they let `BrushRef<'a> = Brush<ImageBrushRef<'a>, &'a Gradient>` (`brush.rs:118`) reuse the same enum shape without cloning the heavy variants. `From<&Brush>` for `BrushRef` (`brush.rs:144`) is one match arm per variant; the `Solid` case copies `AlphaColor` (it's `Copy`), the others borrow.

`Brush::with_alpha` / `multiply_alpha` (`brush.rs:81-110`) propagate uniformly across all three variants. That's the API payoff: callers don't branch on solid-vs-gradient to tweak opacity.

The `From` blanket impls (`brush.rs:24-58`) accept any `AlphaColor<CS>`, `OpaqueColor<CS>`, or `DynamicColor` and call `.convert()` to land in `AlphaColor<Srgb>`. So `button.fill(palette::css::RED)` just works regardless of which color space the caller's palette uses.

## 2. Color: not in peniko

The headline: **peniko doesn't define `Color`**. `lib.rs:37` re-exports the `color` crate; `lib.rs:57` aliases `pub type Color = color::AlphaColor<color::Srgb>` purely for ergonomics. The real type lives one crate down.

`color` is generic over `ColorSpace` (`Srgb`, `LinearSrgb`, `Oklab`, `Lch`, `DisplayP3`, …). Three flavors:

- `AlphaColor<CS>` — straight (unpremultiplied) RGBA in CS, the "neutral" form.
- `OpaqueColor<CS>` — three channels, no alpha. Cheaper, distinct type so opacity bugs are compile errors.
- `PremulColor<CS>` — premultiplied; the form gradients interpolate in.
- `DynamicColor` — runtime-tagged color space, used by `ColorStop` so a single gradient can mix sRGB + Oklab stops.

All channels are `f32`. There is `color::Srgb8` for storage-side bytes, but the API surface is f32 so gamma conversion (`AlphaColor<Srgb>::convert::<LinearSrgb>()`) is honest, not implicit. This is the opposite of egui's `Color32` (`u8` sRGB) and is closer to wgpu's `wgpu::Color` (`f64` linear), but with explicit colorspace tags.

The split between peniko (carries `Brush`/`Gradient`) and `color` (defines color math) is itself the lesson: paint vocabulary doesn't need to depend on color science.

## 3. Gradients

`Gradient` (`gradient.rs:301`) is one struct, three kinds via `GradientKind` (`gradient.rs:269`): `Linear(LinearGradientPosition)`, `Radial(RadialGradientPosition)`, `Sweep(SweepGradientPosition)`. The kinds are flat structs of `kurbo::Point` + scalars (`gradient.rs:144-231`), so geometry comes from kurbo and only the gradient *concept* is peniko's.

The non-kind fields are where the real complexity lives:

- `extend: Extend` — `Pad | Repeat | Reflect` (`brush.rs:159`). Same enum is reused by `ImageSampler::{x_extend, y_extend}` (`image.rs:97-99`).
- `interpolation_cs: ColorSpaceTag` — which color space to interpolate stops in. CSS Color 4 lets you say `linear-gradient(in oklab, …)`; this is the field that implements that.
- `hue_direction: HueDirection` — for cylindrical spaces (LCH, OkLCH), which way around the color wheel.
- `interpolation_alpha_space: InterpolationAlphaSpace` — `Premultiplied` (default, CSS Color 4) or `Unpremultiplied` (HTML canvas) (`gradient.rs:237`). Premultiplied avoids the "fade-through-purple" artifact when going transparent-red → opaque-blue.
- `stops: ColorStops` — `SmallVec<[ColorStop; 4]>` (`gradient.rs:101`). Inline up to 4 stops, heap-spill beyond. Most gradients have 2-3 stops, so the small-vec optimization matters.

`ColorStop = { offset: f32, color: DynamicColor }` (`gradient.rs:28`). `ColorStop` implements `BitHash`/`BitEq` for cache-key use (`gradient.rs:35-46`) — gradients are routinely uploaded to GPU LUT textures and indexed by content hash.

`ColorStopsSource` (`gradient.rs:464`) is a builder-pattern trait: `&[Color]` becomes evenly-spaced stops automatically (`gradient.rs:503-513`), `&[(f32, Color)]` keeps explicit offsets. Keeps `Gradient::new_linear(a, b).with_stops([RED, BLUE])` ergonomic.

## 4. `Compose` × `Mix` = `BlendMode`

`blend.rs` is the cleanest part of the crate. Two orthogonal enums:

- `Compose` (`blend.rs:97`) — Porter-Duff layer composition. 14 modes: `Clear`, `Copy`, `Dest`, `SrcOver`, `DestOver`, `SrcIn`, `DestIn`, `SrcOut`, `DestOut`, `SrcAtop`, `DestAtop`, `Xor`, `Plus`, `PlusLighter`. These describe *which regions of source/destination survive*.
- `Mix` (`blend.rs:11`) — color mixing function. 16 modes from W3C Compositing 1: `Normal`, `Multiply`, `Screen`, `Overlay`, `Darken`, `Lighten`, `ColorDodge`, `ColorBurn`, `HardLight`, `SoftLight`, `Difference`, `Exclusion`, `Hue`, `Saturation`, `Color`, `Luminosity`. The last four are the non-separable HSL-domain modes.

`BlendMode = { mix, compose }` (`blend.rs:159`). Default is `(Normal, SrcOver)` — what 99% of UI wants. `From<Mix>` and `From<Compose>` (`blend.rs:201-217`) let you construct partial blends without naming the other axis.

`is_destructive()` (`blend.rs:179`) flags `Clear/Copy/SrcIn/DestIn/SrcOut/DestAtop` — the modes that can erase backdrop pixels. Renderers use this to skip the "early-out on transparent source" optimization. Useful tag for any GPU pipeline that wants to avoid blending overhead on common cases.

All three enums are `#[repr(u8)]` (`blend.rs:10, 96`, `brush.rs:158`) and (with the `bytemuck` feature, `lib.rs:33-34`) `Contiguous` — meaning `BlendMode` packs into 2 bytes and is safely castable for upload to a uniform/storage buffer.

## 5. Why split from kurbo

Kurbo is the geometry crate — `Point`, `Rect`, `BezPath`, `Affine`, `Stroke` (the *parameters* — width, joins, caps, dash). Peniko depends on kurbo (`gradient.rs:10` uses `kurbo::Point`; `style.rs:4` uses `kurbo::Stroke`) but not the reverse. The split is:

- **kurbo** = "where and what shape" (geometry, transforms, curves, hit testing).
- **peniko** = "what paint" (brush, color, blend, image format).
- **color** = "how to think about color values".
- **vello / vello_cpu / parley** = "how to actually render".

The motivation, from Linebender's design discussions: a path-tessellation crate (kurbo) shouldn't carry color types; a font-shaping crate (parley) needs `Brush` to attach style runs but shouldn't depend on a renderer; a UI like floem wants `Brush` in its widget API without pulling vello. By extracting paint vocabulary, every consumer pays only for what it uses, and Vello + custom renderers can interoperate on the same scene description.

`Style` (`style.rs:40`) is `Fill(Fill) | Stroke(kurbo::Stroke)` — the "what kind of draw call" tag that pairs with `Brush`. `Fill` (`style.rs:13`) is `NonZero | EvenOdd`. Together `(Style, Brush, BezPath, Affine, BlendMode)` is essentially Vello's complete draw-call argument list.

## 6. `Image` and `Blob`

`ImageData` (`image.rs:70`) is `{ data: Blob<u8>, format: ImageFormat, alpha_type: ImageAlphaType, width, height }`. `Blob` is from `linebender_resource_handle` (`lib.rs:43`) — an `Arc<dyn AsRef<[u8]>>` with content-id, so equality is pointer-cheap and uploads are dedupable.

`ImageFormat` is intentionally tiny: just `Rgba8` and `Bgra8` (`image.rs:11`). The complexity lives in `ImageSampler` (`image.rs:95`) which carries `x_extend`, `y_extend`, `quality: ImageQuality { Low, Medium, High }`, and an alpha multiplier. `ImageBrush<D>` (`image.rs:190`) is generic over the storage `D` — defaulting to `ImageData` (owned) but specializable to a renderer's pre-registered atlas id, which is exactly what a wgpu backend wants.

## 7. Lessons for Palantir

**Adopt the `Brush` enum shape, not the dependency.** Palantir's `src/geom.rs:47` `Color` is fine for the prototype, but as soon as gradients or images are needed, lift the *shape* `enum Paint { Solid(Color), Gradient(Gradient), Image(ImageHandle) }`. The two-generic `Brush<I, G>` trick for cheap borrowed vs. owned views is worth copying — recording-time we want `PaintRef<'a>` to avoid cloning gradient stops into the arena. Implement `From<Color> for Paint` and accept `impl Into<PaintRef>` on widget builders, mirroring `brush.rs:24-58`.

**Don't pull peniko itself, yet.** Peniko transitively brings `color`, `kurbo`, `linebender_resource_handle`, `smallvec`. Palantir already has `glam::Vec2` for geometry; mixing `kurbo::Point` would mean either depending on both (bloat) or rewriting `geom.rs` on top of kurbo. Defer this until after the wgpu paint pass exists — if we end up using vello for path rendering anyway, peniko comes along for free and `glam`-vs-`kurbo` is forced. If we stay on a custom SDF rounded-rect + glyphon pipeline, we don't need kurbo and can roll a 100-line internal `Paint`/`Gradient`/`BlendMode` mirror.

**Lift `Compose` and `Mix` verbatim if/when blending is needed.** They're literally W3C Compositing 1 transcribed to enums; reinventing them is pointless. The `is_destructive()` flag (`blend.rs:179`) is exactly the kind of metadata our paint pass wants to skip overdraw on common cases.

**Keep `Color` as `f32 RGBA` like peniko, not `u8` like egui.** `geom.rs:47` already does this — good. Resist the temptation to add a `Color32`. Gamma-correct math (SDF anti-aliasing especially) wants linear floats; conversion to 8-bit is a renderer concern at vertex-buffer pack time, not a public API concern. When real color spaces matter (theming, dark/light mixing), reach for the `color` crate then — it's the Linebender consensus and adopting it later is a re-import, not a redesign.

**Borrow the `Extend` reuse trick.** Peniko reuses `Extend { Pad, Repeat, Reflect }` between gradients and image samplers (`brush.rs:159`, `image.rs:97`). Same enum, two consumers. When Palantir grows image fills and gradient backgrounds, define `Extend` once.

**Don't repeat the `ColorStopsSource` trait.** It's elegant but the ergonomic win (`with_stops([RED, BLUE])` vs. `with_stops(&[(0.0, RED), (1.0, BLUE)])`) is small for an internal API. A two-method builder (`add_stop`, `with_stops_array`) costs one screen of code and saves a whole trait-dispatch layer. Peniko needs the trait because it's a public crate consumed by strangers; Palantir is a single binary's UI lib for now.

**Watch the kurbo/glam fork.** This is the single biggest forward-compat decision. If Palantir ever wants to consume Vello, Parley, or any Linebender stack, switching `geom.rs` to wrap kurbo is mandatory. Mark this in `DESIGN.md` as a known fork point so the decision isn't made by accident.
