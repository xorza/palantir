# Shapes and paint

What a caller draws with `Ui::add_shape` and how each paint covers its shape. The words are in
the top `AGENTS.md`, "A shape is geometry, a paint what covers it"; how the GPU draws them is in
`src/gpu/AGENTS.md`.

## Drawing

- **`Ui::add_shape(shape, paint)`**: geometry and paint are two arguments, as Skia's
  `drawRRect(rrect, paint)`, Flutter's `Canvas` and SwiftUI's `GraphicsContext.fill` take them.
  Each shape takes the paints its pipeline draws through a sealed bound, one impl per pair, so
  rustdoc lists each shape's paints; a pair the pipeline cannot draw panics at `add_shape`.
  Refused: a `.paint(p)` setter on each shape; a bound on the `(shape, paint)` tuple, which
  rustdoc shows as one blanket impl; Vello's separate `fill` and `stroke` calls, which split a
  box's one draw.
- **An animation goes on the added shape**: `add_shape` returns the `AddedShape`, or `None` where
  the pair paints nothing. Refused: `add_shape_animated`, a second signature of one call; a
  `Result`, as painting nothing is no failure.
- **A shape lays out in a rect**: the owner's arranged rect or `.at(rect)`, as WPF's `Rectangle`
  and SwiftUI's shapes take the rect they are offered; a circle is the largest one centered in
  it. Refused: a center and a radius, which no layout moves.
- **A point shape's points are px in its rect, and `Fit` scales them**, with CSS `object-fit`'s
  names, as WPF's `Polygon` takes a `Stretch`; a stroke's width and a corner radius do not scale.
  Refused: fractions of the rect, which a glyph drawn to the pixel would divide by its size.

## Paints

- **Images, icons and effects are paints of a shape**, as Skia's `SkImageShader` and SwiftUI's
  `ImagePaint` fill one. Refused: a `Brush::Image`, as a brush is saved in theme files and
  tweened, which a handle cannot be; an effect as a shape kind with a rect of its own.
- **A shape clips its image**, as CSS's `border-radius` clips an `object-fit` image. Refused: the
  corners rounding the fitted image, as SwiftUI's `clipShape` after `aspectRatio`, which a
  triangle's image could not follow.
- **A gradient spans its shape's bounds**, turned with the shape, as SVG's `objectBoundingBox`.
  Refused: the owner's rect, which a triangle inside it does not fill; the screen's frame, which a
  turn leaves behind.
- **A box's fill reaches under its border, a material under both, in one draw**, as CSS's
  `background-clip: border-box` and `backdrop-filter` cover the border box; an edge pixel weighs
  the border over the fill by area. Refused: the fill inside the border, one `Paint` two ways; a
  second draw or a second pipeline for the material.
- **A shape covers its inside**: no shape paints the outside of its boundary but a halo's
  bounded rim. Refused: Skia's inverse fill types, a form of their own in every pipeline.

## Effects

- **An effect is a program; theme data names one; `EffectPaint` is the one draw.** An effect is a
  WGSL program an `EffectName` names, a color effect or a backdrop effect; palantir's stock ones
  are loaded and checked as an app's are. `Material` and `PressEffect` name one as plain data a
  theme file saves and a transition blends, and lower to the one effect draw. As Skia's
  `SkRuntimeEffect`, Android's `RuntimeShader` and Flutter's `FragmentShader` are one program
  whose placement decides its use. A material holds its effect with the effect's parameters as
  f32 lanes inline (`MaterialEffect`), which a theme file lists and a tween blends. Refused: parameter bytes of any
  type, which no theme file holds and no tween blends; a closed set of material looks (SwiftUI,
  WinUI), which drops app materials; a built-in filter type beside app effects.
- **A halo is a paint's, of the shape it surrounds**: `EffectPaint::outside(reach)` paints a color
  effect outside its rect or circle alone, as CSS draws an outset `box-shadow` outside the border
  box only. The effect reads the shape's own frame, and palantir cuts the shape's edge whatever
  the effect returns there. Refused: a shape of two edges, Skia's `drawDRRect`
  ([example](https://skia.googlesource.com/skia/+/main/docs/examples/Canvas_drawDRRect_b.cpp))
  and Android's `drawDoubleRoundRect`, whose effect learns the inner rect through its params; a
  backdrop effect's halo, until its capture takes the reach in; a triangle's, whose effect reads
  no corners.
- **A triangle's effect reads its covering rect and no corners**; palantir clips it to the
  triangle, as the CSS Paint API hands a worklet its box alone. Refused: an edge distance in
  `EffectInput`, which every effect would pay for.
