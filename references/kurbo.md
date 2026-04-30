# kurbo — reference notes for Palantir

kurbo is Linebender's 2D curve and shape vocabulary crate. It is the geometry layer that druid, vello, parley, peniko and floem all share — `Point`, `Vec2`, `Affine`, `Rect`, `RoundedRect`, `Circle`, `BezPath`, plus the algorithms (arclen, area, nearest-point, offset, curve fitting) those types need to be useful. It is **f64-only**, **single-purpose** (curves and shapes, no rendering), and aggressively numerically robust. This file pins what's worth borrowing and what the cost would be.

All paths under `tmp/kurbo/kurbo/src/`.

## 1. The `Shape` trait + `PathEl` / `PathSeg`

`Shape` (`shape.rs:17`) is the unifying interface. Every concrete primitive — `Line`, `Rect`, `RoundedRect`, `Circle`, `Ellipse`, `Arc`, `BezPath`, `CubicBez`, `QuadBez`, `Triangle` — implements it. Required methods: `path_elements(tolerance) -> Iterator<PathEl>`, `area`, `perimeter`, `winding`, `bounding_box`. Optional `as_rect`, `as_line`, `as_rounded_rect`, `as_circle`, `as_path_slice` let consumers fast-path concrete cases before falling back to Bézier flattening (`shape.rs:144-171`). The blanket `impl<T: Shape> Shape for &T` (`shape.rs:175`) means APIs can take `impl Shape` and accept owned or borrowed.

The two representations are deliberate. `PathEl` (`bezpath.rs:114`) is the drawing-API view: `MoveTo | LineTo | QuadTo | CurveTo | ClosePath` — one element per "pen instruction." `PathSeg` (`bezpath.rs:136`) is the geometry view: `Line(Line) | Quad(QuadBez) | Cubic(CubicBez)`, each segment self-contained with its start/end. `BezPath(Vec<PathEl>)` (`bezpath.rs:106`) stores elements; `BezPath::segments` (`bezpath.rs:307`) and the free `segments()` adapter (`bezpath.rs:802`) lift any `Iterator<Item = PathEl>` into `Iterator<Item = PathSeg>` by remembering the previous endpoint. Hit-testing, subdivision, arclen and area all work on segments; serialization and drawing work on elements.

Tolerance is the universal accuracy knob. The `Shape::path_elements` doc (`shape.rs:39-48`) calls out that "a value of 0.1 is appropriate" for UI, and that segment count scales as `tolerance ^ (-1/6)` — i.e. cheap to tighten.

## 2. Numerically robust algorithms

**Arclen** is the canonical example. `CubicBez::arclen` (`cubicbez.rs:673`) calls `arclen_rec` (`cubicbez.rs:628`), which estimates Gauss-Legendre quadrature error using derivative magnitudes at the midpoint, then picks 8/16/24-point quadrature or recursively subdivides. The estimator at `cubicbez.rs:642-662` is the trick: it computes the error a priori from the curve's "wiggliness" `(dd_norm2 / d_norm2)` rather than running the integration twice and comparing. `Line::arclen` and `QuadBez::arclen` are closed-form (`line.rs:143`, `quadbez.rs:276`).

**Inverse arclen** — "what `t` gives me arclen `s`?" — uses the ITP (Interpolation-Truncation-Projection) root finder rather than bisection (`param_curve.rs:98-123`). ITP retains bisection's worst-case bound but typically converges as fast as secant. The implementation also subdivides progressively so each evaluation operates on a shorter sub-curve, avoiding O(n²) total work.

**Area** is exact for Béziers via Green's theorem; `CubicBez::signed_area` is a closed-form polynomial (`cubicbez.rs:680`). Path area is sum of segment areas (`bezpath.rs:953`). No subdivision needed.

**Nearest-point** on a cubic reduces to roots of a degree-6 polynomial: minimize `|c(t) - p|²`, take derivative, solve quintic via Bairstow-style root-finding (`cubicbez.rs:695`). Returns a `Nearest { distance_sq, t }` (`param_curve.rs:147`). For arbitrary shapes, the docs at `lib.rs:31-56` show the canonical pattern: iterate `path_segments(tol)`, take min of `seg.nearest(p, tol)`. This is exactly what hit-testing for non-rectangular widgets would call.

**Bounding box** uses extrema of each segment (`ParamCurveExtrema`, `MAX_EXTREMA = 4`); for cubics this means solving the derivative quadratic per axis, evaluating the curve at those `t`s, and taking min/max.

The pattern is consistent: closed-form when possible (line, quad area, segment endpoints); a priori error estimation + adaptive subdivision when not (cubic arclen); iterate-segments + reduce when working on whole shapes.

## 3. `Affine` vs `TranslateScale`

`Affine([f64; 6])` (`affine.rs:17`) is the full 2D affine — six coefficients for the 2×3 matrix `[a c e; b d f]`. Supports rotation, shear, non-uniform scale, `then_*` composition, `inverse()`, `determinant()`. Multiplication is matrix-multiplication; `Affine * Point` is defined, the reverse isn't (`affine.rs:42-46`).

`TranslateScale { translation: Vec2, scale: f64 }` (`translate_scale.rs:44`) is the restricted subset: uniform scale + translation, four floats instead of six. The motivating comment (`translate_scale.rs:39-40`) is the key insight: "less powerful than `Affine`, but can be applied to **more primitives, especially including `Rect`**." Under a general affine, a `Rect` is no longer axis-aligned — it has to become a `BezPath` or quad. Under `TranslateScale`, a `Rect` stays a `Rect`, a `Circle` stays a `Circle`, a `RoundedRect` stays a `RoundedRect`. So a viewport zoom/pan, a parent-to-child position transform, or a DPI scale uses `TranslateScale` and preserves the type. Only when actual rotation enters do you reach for `Affine`.

This is directly relevant to a UI library's paint pass: the typical "owner rect → child rect" composition is pure translation, and `Rect: Add<Vec2>` (`rect.rs` impls) handles that without a transform type at all. `TranslateScale` is the right escape hatch for zoom/DPI; `Affine` only enters when you allow rotated content.

## 4. Curve fitting and the offset-stroke implementation

`fit.rs` is "given a `ParamCurveFit` source (e.g. an Euler spiral, an offset of a cubic, a perspective-distorted curve), produce a `BezPath` of cubics matching it to within `accuracy`." The trait (`fit.rs:51`) doesn't expose `eval` and `deriv` separately — instead `sample_pt_tangent(t, sign)` returns position + unit tangent, and `sign` chooses which side of a cusp the tangent comes from (`fit.rs:54-60`). This is what makes the algorithm robust on offset curves, which routinely have cusps whose existence and location the caller doesn't know in advance.

`fit_to_bezpath` (`fit.rs:165`) is the recursive driver: try a single cubic over `[t0, t1]`, if it doesn't meet error subdivide. The single-cubic fit (`fit_to_cubic` / `try_fit_line`) matches signed area and first moment to the source, derives a quartic in the tangent magnitudes, solves it, and picks the best root by sampling N=20 points and taking the worst error. A `break_cusp` query on the source lets the recursion split exactly at cusps instead of bisecting through them. There's also a slower `fit_to_bezpath_opt` (`fit.rs:564`) that picks subdivision points more carefully for fewer output segments.

`offset.rs` is the headline application. Cubic offset curves are not themselves cubics — they're arbitrary algebraic curves with potential cusps where curvature equals `1/d`. The implementation (`offset_cubic`, `offset.rs:108`) wraps the cubic in a `CubicOffset` source (`offset.rs:20`) that knows its cusp polynomial `c0 + c1*t + c2*t²` (`offset.rs:27-35`), then runs the curve fitter on it. `MAX_DEPTH = 8` recursion guard (`offset.rs:67`); `N_LSE = 8` sample points for least-squares (`offset.rs:51`). Levien's claim is that this is robust on near-cusp "J-shaped" curves where naive offsetting (e.g. Tiller-Hanson) blows up.

The Euler-spiral angle (Levien's posts, not in this tree) is upstream of this: when fitting *one* cubic to an arbitrary curve, an Euler spiral is the natural intermediate because its curvature is linear in arclength, which makes the area/moment math closed-form. Inside kurbo today the fitter operates directly on cubics via the quartic-area-match approach, but the design (separate `ParamCurveFit` source + cubic-fitting kernel) is the one that fell out of the spiral work.

For Palantir, the relevance is: rounded-rect borders and stroked paths in the UI are offset-curve problems. Egui sidesteps them by tessellating to triangle fans on the CPU. Vello sidesteps them by stroking in a compute shader. Kurbo provides the CPU-side reference implementation that the others can be checked against.

## 5. f64-only, vs glam / euclid / mint

Every coordinate in kurbo is `f64`. `Point { x: f64, y: f64 }`, `Vec2 { x: f64, y: f64 }` (`vec2.rs:24`), `Rect { x0, y0, x1, y1: f64 }` (`rect.rs:20`), `Affine([f64; 6])`. No `f32` variants, no SIMD, no generics over scalar.

The contrast with neighbors:
- **glam** is `f32`-by-default with `Vec2`/`Vec3`/`Vec4`/`Mat3`/`Mat4`, SIMD-backed, oriented at games and shaders. Trades precision for throughput.
- **euclid** (Servo's) is generic over scalar and **typed by unit** (`Point2D<f32, ScreenPx>`), preventing accidental cross-space arithmetic. Has an `interop_euclid.rs` adapter in kurbo (`tmp/kurbo/kurbo/src/interop_euclid.rs`).
- **mint** is just interface types for interop; no algorithms.
- **nalgebra** is general linear algebra; far more than 2D needs.

Levien's `f64` choice is deliberate: curve algorithms (root-finding, offset cusps, area integrals) lose accuracy fast in `f32`. A degree-6 polynomial solve for cubic nearest-point will produce visible errors in `f32` for normal UI coordinate ranges. f64 also costs nothing on modern CPUs for non-vector code, and kurbo isn't the SIMD hot path — vello is.

The cost: every coordinate that crosses kurbo↔wgpu becomes an `as f32` cast at the boundary. This is what vello does (`tmp/vello/vello_encoding/`) and it is fine.

## 6. Lessons for Palantir

**The split that matters.** kurbo isn't trying to be a math library — it's trying to be the *vocabulary* shared between layout, hit-test, paint and a renderer. `Shape` + `path_elements(tolerance)` + the `as_*` downcast methods is exactly the contract Palantir's `Shape` enum (`src/shape.rs`) is reaching for. Right now Palantir's enum is closed (`RoundedRect | Text | Line | …`) and that's correct for the paint pass — typed batches need to know the variant. But for hit-testing arbitrary paths later, kurbo's open trait + path-element fallback is the right shape.

**Should Palantir replace `geom.rs` with kurbo?** Concretely, swap `Vec2`, `Size`, `Rect`, `Color`, `Stroke` for `kurbo::Vec2`, `kurbo::Size`, `kurbo::Rect`, plus a thin Palantir `Color` (kurbo doesn't ship one — peniko does). The cost ledger:

- *Wins*: free `Rect::union/intersect/inset`, free `bounding_box`/`area`/`winding` on every shape we add later, free hit-testing math, free `TranslateScale` for DPI/zoom, free serde of geometry, automatic vello/peniko interop if we ever swap renderers.
- *Costs*: `f64` everywhere in layout (currently `f32`); a real dependency rather than ~200 lines of `geom.rs`; learning the kurbo idioms (no `Size::new` constructor for free in const context, etc.); vendor-locked to Linebender's release cadence.
- *Verdict*: worth it once we have more than rectangles. While the only shapes are `RoundedRect`, `Line`, `Text`, the f32 `Vec2`/`Rect` in `geom.rs` are paying their way. The moment we add path support — vector icons, custom widget chrome, anything stroked — the kurbo dependency becomes load-bearing and rewriting it is wasted work. Pre-decide: introduce kurbo as soon as `Shape::Path` is on the roadmap, not before.

**Borrow now, dependency later.**
- The `(tolerance, accuracy)` parameter convention. Every algorithm exposes its accuracy knob; UIs pass 0.1, scientific code passes 1e-6, and the implementation adapts. Palantir's eventual text-shaping and SDF-rounded-rect APIs should follow.
- `path_elements` returning an iterator, not a `Vec`. Lets `Rect::path_elements` be allocation-free (just yield 5 elements); lets `BezPath::path_elements` be a slice iterator. Same idea for any future "tessellate to GPU" path in Palantir.
- The `as_rect` / `as_circle` downcast pattern. In the paint pass, before falling back to a generic shape walker, check the concrete-type fast paths. Maps directly onto the typed-batch model (`RoundedRect → SDF instance`, `Text → glyph quads`).
- Separating `PathEl` (drawing) from `PathSeg` (geometry). When Palantir gets a `Path` shape, store elements, derive segments lazily for hit-test/measure.

**Avoid.**
- The `BezPath: Vec<PathEl>` allocation per path. For UI it'd be fine but kurbo itself notes (`shape.rs:36`) that allocating per shape is a real cost — Palantir's `Tree.shapes` arena already avoids this; don't regress.
- Generic `impl<T: Shape> Shape for &T`. Convenient in a library, but Palantir's recorder owns its shapes outright; an extra trait layer just costs compile time.
