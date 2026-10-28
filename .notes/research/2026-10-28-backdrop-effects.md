# Research for backdrop blur, frosted glass and glass (2026-10-28)

## Practice elsewhere

- CSS `backdrop-filter`: the backdrop is the content behind the element, and
  a blur samples past its edge by mirroring that content inward
  (`edgeMode="mirror"`, fxtf-drafts #374 and #527, resolved November 2023;
  Filter Effects 2: "if the filter functions list includes a blur() filter,
  the filter will be applied with edgeMode=mirror"). Chrome had duplicated
  the edge pixels, so "even a 1px scroll can cause the color to completely
  change"; its PSA says mirror "visually aligns closely to Safari's filter".
  No source found says that Safari or Firefox read content past the
  element's edge; they show no flicker, by an unstated method.
  <https://drafts.csswg.org/filter-effects-2/>,
  <https://groups.google.com/a/chromium.org/g/blink-dev/c/ZtMnFCHZhMQ>,
  <https://jameshfisher.com/2024/04/23/backdrop-blur-without-the-flickering/>
- CSS `blur(<length>)` takes the Gaussian's standard deviation, unlike
  `box-shadow`, whose blur radius is 2σ. Apple's `blur(17.5px)` is σ = 17.5.
  <https://drafts.fxtf.org/filter-effects/#funcdef-filter-blur>
- Android SurfaceFlinger clamps a blur to its rect: a read past the rect
  takes the edge color (change 561d05c18f, "Fix blur sampling outside bounds
  of layer"). Compose's `BlurredEdgeTreatment` offers the same clamp or
  transparent black.
  <https://android.googlesource.com/platform/frameworks/native/+/561d05c18f>
- Every engine found reads the backdrop back from the target it is drawing
  into, and none replays the draws below. WebRender splits the parent
  surface's task into phases on one backing surface: "Primitives added prior
  to the backdrop filter will be added to task A, phase 0", the filter reads
  a resolve of it, and "A(1) is bound to the same backing surface as A(0)
  but does not clear the target" (bugs 1762818, 2011747). Impeller ends the
  pass and blits the backdrop; PR 55983 removes the last blit where the
  device has framebuffer fetch.
  <https://bugzilla.mozilla.org/show_bug.cgi?id=1762818>,
  <https://bugzilla.mozilla.org/show_bug.cgi?id=2011747>,
  <https://dart.googlesource.com/external/github.com/flutter/engine/+/a456ad0a7d782a5e869ae017f5770fcdac49a54b>
- Chromium `cc` damage: a surface with a pixel-moving backdrop filter that
  intersects damage from content drawn before it takes its whole rect into
  the damage (`HasFilterThatMovesPixels`, `intersects_damage_under`).
  <https://chromium.googlesource.com/chromium/src.git/+/9ec375f2c2485af85d873791e9a2a32b71d6f903%5E%21>,
  <https://chromium.googlesource.com/chromium/src/+/aa6dc33d901b79097eb7440d2e201ecdf56db341/cc/trees/damage_tracker.cc>
- Flutter: `BackdropGroup` and `BackdropKey` let several `BackdropFilter`s
  share one backdrop read; Impeller PR 55701 "removes second save layer for
  each backdrop filter; can fuse backdrop filters if there is more than one
  identical filter". <https://api.flutter.dev/flutter/widgets/BackdropGroup-class.html>,
  <https://dart.googlesource.com/external/github.com/flutter/engine/+/ad9e4fef451a73427285826455db5fbdde502bd7%5E%21/>
- SwiftUI Liquid Glass: `GlassEffectContainer` shares one sampling region
  among nearby glass shapes; glass cannot sample other glass (secondary
  sources; Apple's page is "Applying Liquid Glass to custom views", WWDC25
  session 323). <https://swiftwithmajid.com/2025/07/23/glassifying-custom-swiftui-views-groups/>
- Apple's own WebKit media controls: `backdrop-filter: saturate(180%)
  blur(17.5px)` under a faint white tint.
  <https://opensource.apple.com/source/WebCore/WebCore-7606.3.4.0.2/Modules/modern-media-controls/controls/background-tint.css>
- Windows Acrylic: background, Gaussian blur, exclusion blend, tint and
  luminosity opacity, noise. Replications leave out the exclusion layer,
  whose effect is hard to see. FluentUI QML defaults: blur 32, tint opacity
  0.65. <https://learn.microsoft.com/windows/uwp/style/acrylic>,
  <https://zhuzichu520.github.io/qml-fluentui-controls-acrylic.html>
- Haze (Compose): blur, tint and a tiled precomputed blue-noise texture at
  15 % by default; progressive (varying) blur costs about 25 % more on
  Android 13+, a mask fade almost nothing.
  <https://chrisbanes.github.io/haze/latest/usage>,
  <https://chrisbanes.github.io/haze/latest/blur/>
- Skia GPU blur: halves the image and the sigma while sigma exceeds 4
  (`kMaxLinearBlurSigma = 4.f`, at most 28 samples a pass), then runs a
  separable Gaussian with linear sampling; its radius is `ceil(3σ)`, and a
  direct 2D kernel stays near 5×5. <https://skia.googlesource.com/skia/+/chrome/m118/src/gpu/BlurUtils.h>,
  <https://skia.googlesource.com/skia/+/0c42332/src/effects/SkGpuBlurUtils.cpp>
- Dual Kawase (Bjørge, ARM, SIGGRAPH 2015): different down- and upsample
  kernels at a constant sampling distance, less bandwidth than a Gaussian.
  KWin uses it; its strength comes from iteration count plus an offset, and
  a KWin developer noted "there aren't enough strength steps"; KWin adds
  noise against banding.
  <https://community.arm.com/cfs-file/__key/communityserver-blogs-components-weblogfiles/00-00-00-20-66/siggraph2015_2D00_mmg_2D00_marius_2D00_notes.pdf>,
  <https://phabricator.kde.org/D9848>
- Linear sampling (Rákos 2010): one bilinear fetch reads two Gaussian taps,
  9 taps for 5 fetches. OBS StreamFX found dual filtering better for large
  radii. <https://www.rastergrid.com/blog/2010/09/efficient-gaussian-blur-with-linear-sampling/>
- Liquid Glass refraction (kube.io): Snell's law with one refraction event,
  rays orthogonal to the pane, a bezel height profile; a convex bezel
  converges rays and magnifies. Shader replications take the rounded-rect
  SDF gradient as the normal, add a Fresnel and directional rim highlight,
  and optional chromatic dispersion; Haze offers a concave bezel and a raised
  lip. <https://kube.io/blog/liquid-glass-css-svg>,
  <https://chrisbanes.github.io/haze/latest/effects/liquid-glass/>,
  <https://cdn.jsdelivr.net/npm/@ybouane/liquidglass@1.0.3/src/shaders.ts>
- Bicubic B-spline from four bilinear fetches: Sigg and Hadwiger, "Fast
  Third-Order Texture Filtering", GPU Gems 2 ch. 20. The cubic B-spline's
  variance is 1/3 texel², and by Marsden's identity `Σ j² B(x − j) = x² + 1/3`
  at every phase `x`, so the variance it adds does not depend on where a
  pixel falls between texels.

## Arithmetic

- The tent `[1 3 3 1] / 8` has variance 0.75 source texel²: two bilinear
  fetches at ±0.75 texel read it on one axis, four in 2D.
- A Gaussian truncated at ±3σ keeps 0.9733 of its variance: a kernel cut at
  `ceil(3σ)` and renormalized is 1.3 % narrower than its nominal σ.
- `f32::exp` is not `const` (rustc 1.99.1, E0015), so no `const fn` can
  compute Gaussian weights.
- Interleaved gradient noise: Jimenez, "Next Generation Post Processing in
  Call of Duty: Advanced Warfare", SIGGRAPH 2014; a per-pixel dither with no
  texture.

## In the code

- One frame is one main render pass per surface (`wgpu_backend/mod.rs`
  `run_main_pass`), walked once per damage rect; partial frames
  `LoadOp::Load` the `Backbuffer`, whose texture has `TEXTURE_BINDING` and
  `COPY_SRC`. Full frames above `DIRECT_PROMOTE_COVERAGE` (0.4) render
  straight into the surface, which offers only `RENDER_ATTACHMENT`
  (`SurfaceUsages`).
- The stencil clears per pass and is discarded at its end; the schedule
  never ends a walk with a mask stamped.
- `Quad` is 64 B, pinned; `QuadForm` (solid, gradient, triangle) is one
  pipeline each, chosen to keep register pressure off the solid form (Pi 5
  V3D: 54 temporaries for all forms, 22 for solid).
- `OcclusionPruner` drops a quad covered by a later opaque quad of the same
  scissor group; text batches coalesce across groups while paint order
  holds; the clear fold drops everything before a full-viewport opaque quad.
- `finish_region` collapses `raw_rects` by the pass-budget heuristic.
- `GpuView` targets: `frame_targets` lists the ones to repaint and
  `live_targets` the roster that keeps the rest alive, the retention pattern
  a cached backdrop can reuse.
- `Shadow::blur` is a Gaussian σ in logical px, half CSS's blur radius;
  shadows are analytic in the quad shader and need no backdrop.
- `prelude.wgsl` substitutes `AA_HALF_WIDTH` as a constant, and the
  immediates hold only `viewport_size`, which `clip_from_px` divides by, so
  clip space does not depend on the target's resolution.
- Fragment code reads `@builtin(position)` as surface px: the quad shadow
  (`shadow_pixel`) and the cutout bake in `quad_pipeline/shader.wgsl`.
- The raster atlas's flat path reads one texel per pixel by `textureLoad`
  and takes `FLAG_RESAMPLE` only when a quad's size differs from its
  raster's; at another target scale it would skip texels.
- `DamageEngine::compute` pushes `raw_rects` per plane in paint order, then
  the predamaged animation rects and the removed widgets' rects, which carry
  no paint position.
- Pipelines are built per `TargetFormat` (`FormatPipelines`); a target in
  another format needs its own set.
- Layers paint bottom-up (`Main`, `Popup`, `Modal`, `Menu`, `Tooltip`), all
  into one `RenderBuffer`.
