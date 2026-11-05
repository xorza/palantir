# Backdrops: blur, frosted glass and glass (2026-10-28 to 2026-11-01)

Facts and measurements behind `gpu::backdrop`, `shape::paint::blur_plan` and the glass composite.
Hosts: a mobile AMD iGPU ran RADV on Linux; two Apple laptops, on Metal.

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

## How others split color through glass

- Filament's `specgen` (2025): N wavelengths, each a matrix
  `K_n = M_XYZ→sRGB · diag(W_n) · M_sRGB→XYZ` with `W_n` the CIE 1931 observer at `λ_n` times
  its quadrature weight; each XYZ channel of `Σ W_n` normalized to 1, so `Σ K_n = I`. The index
  per sample is `n_D + ((n_D − 1) / V_d) · offset(λ)`, Cauchy's model through D with
  `offset(λ) = (λ⁻² − λ_D⁻²) / (λ_F⁻² − λ_C⁻²)`. Default 4 samples at F, e, D and C ("best for
  strong rainbows"), or Gauss–Legendre nodes ("best for accuracy"). The observer is Wyman,
  Sloan and Shirley's multi-lobe fit, or the CIE table.
  <https://google.github.io/filament/dup/specgen.html>,
  <https://github.com/google/filament/blob/main/tools/specgen/specgen.cpp>
- Haze (Compose) liquid glass: `ChromaticAberrationMode.Simple`, red read forward, green at the
  center, blue backward; `Full`, seven reads along the offset, red to purple, with channel
  weights set by hand (red `1/3.5` from red, orange and yellow, `1/7` from purple, …).
  <https://github.com/chrisbanes/haze/blob/main/haze-glass/src/commonMain/kotlin/dev/chrisbanes/haze/glass/GlassShaders.kt>
- luma.gl glass material: red, green and blue transmission at their own offset.
  <https://luma.gl/docs/api-reference/experimental/glass-material>
- `@ybouane/liquidglass`: red along the normal, blue against it, green at the center, from a
  sharp and a blurred copy. <https://cdn.jsdelivr.net/npm/@ybouane/liquidglass@1.0.3/src/shaders.ts>
- Maxime Heckel: per-channel indices, several reads per channel, and six channels (r, y, g, c,
  b, v) folded back into RGB.
  <https://blog.maximeheckel.com/posts/refraction-dispersion-and-other-shader-light-effects/>

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
- Glass (Schott catalog): N-BK7 `n_d` 1.5168, `V_d` 64.17, so `n_F − n_C` ≈ 0.0081; N-SF11
  `n_d` 1.7847, `V_d` 25.68, so ≈ 0.0306.
- Cauchy's offset at 400 nm is 1.7643 and at 700 nm −0.4390: across 400–700 nm the index moves
  2.2 times `n_F − n_C`.

## Measured: the capture, replay against split (2026-10-28)

`integration/backdrop_frame/gpu/*/scrolling` (three panes, 2560×1440, every
pane recaptured each frame) and `modal/scrolling` (one pane over a 4K
window), wall time per frame, against the same cases with the main pass split
at each pane's barrier (`*_split`). The split frames match the replay's bit
for bit (`a_split_main_pass_paints_what_the_replay_paints`).

| case | mobile AMD iGPU replay | split | Apple laptop replay | split |
|---|---|---|---|---|
| blur | 6.44 ms | 4.74 ms | 2.32 ms | 2.00 ms |
| frosted | 6.46 ms | 4.82 ms | 2.32 ms | 2.01 ms |
| glass | 13.41 ms | 13.08 ms | 5.13 ms | 4.89 ms |
| modal 4K | 10.18 ms | 9.60 ms | 5.51 ms | 4.88 ms |

The Apple laptop runs are in ABBA order and each pair agrees within 1 %; the mobile AMD iGPU runs are one pass each.
The Apple laptop is a tiler: there a split stores and loads the whole attachment and still wins.

Partial frames (`*/under`, two panes recapture; `*/sliding`, one or none),
ABBA on both hosts:

| case | mobile AMD iGPU replay | split | Apple laptop replay | split |
|---|---|---|---|---|
| blur under | 3.9–4.1 ms | 3.9–4.0 ms | 1.58–1.60 ms | 1.67–1.68 ms |
| frosted under | 3.7–4.1 ms | 3.8–4.2 ms | 1.58–1.60 ms | 1.67–1.68 ms |
| glass under | 10.27–10.30 ms | 10.08–10.23 ms | 3.69–3.71 ms | 3.77–3.78 ms |
| blur sliding | 1.86 ms | 1.77–1.79 ms | 0.402 ms | 0.402 ms |
| glass sliding | 5.53–5.60 ms | 5.01–5.42 ms | 1.60–1.61 ms | 1.61–1.62 ms |

On the Apple laptop the split costs a partial frame about 6 % with two captures: the
tiler stores and loads the whole attachment for a small footprint. On the
mobile AMD iGPU the two agree within the run-to-run drift.

## Measured: a blur pass has a fixed cost (2026-10-29)

A temporary bench blurred `n` regions of 8 × 8 texels at σ 32 px (three tents and the two
residual axes, five passes a job), on surfaces of 256² to 4096². Wall clock on fanless, device time
between passes local.

| case | fanless, a pass per job and stage | fanless, one pass a stage | local, a pass per job and stage |
|---|---|---|---|
| 1 job, 256² | 465 µs | 458 µs | |
| 1 job, 2048² | 463 µs | 457 µs | 41 µs |
| 1 job, 4096² | 463 µs | 456 µs | |
| 8 jobs, 2048² | 2.16 ms | 459 µs | 248 µs |
| 16 jobs, 2048² | 4.04 ms | | 491 µs |

- The cost does not follow the surface: the passes load and store only what their scissors
  cover, or the Apple laptop skips tiles without geometry.
- On fanless each job adds about 240 µs, 48 µs a pass; drawn in one pass a stage, eight jobs cost what
  one does. Local, each job adds about 30 µs.

## Measured: where a frame's device time goes (2026-10-29)

`integration/backdrop_frame/gpu/*/scrolling`, device time split between the main pass, the
captures and the blurs, on a mobile AMD iGPU:

| scene | main pass | captures | blurs |
|---|---|---|---|
| modal, 4K | 2.22 ms | 394 µs | 732 µs |
| cards, 8 panes | 793 µs | 248 µs | 610 µs |
| many, 12 panes | 1.01 ms | 162 µs | 463 µs |
| stacked, 2 panes | 619 µs | 76 µs | 345 µs |
| pair, 2 panes | 568 µs | 81 µs | 173 µs |
| first, its pane on the clear | 571 µs | | |

## Measured: dispersion (2026-11-01)

`integration/backdrop_frame/gpu/glass/*`, an Apple laptop, full runs, the glass look's lens at
dispersion 0.06 and σ 2:

| case | three reads (before) | weight loop, 48 points | closed form |
|---|---|---|---|
| under | 1.450 ms | 1.966 ms | 1.482 ms |
| moving | 718 µs | 856 µs | 728 µs |
| sliding | 720 µs | 858 µs | 728 µs |
| scrolling | 1.949 ms | 2.586 ms | 1.997 ms |
| cached | 82.9 µs | 82.7 µs | 82.6 µs |

The closed form at the highest dispersion, 1, where the bezel's rim spreads past 32 texels and
reads the most steps, 33: under 1.809 ms, sliding 923 µs, scrolling 2.452 ms, 22–27 % over the
glass look's 0.06. Moving read 1.548 ms with a 1.13–2.28 ms interval, too noisy to rank.

A sum over the reads alone, four reads across a 2.3 px spread, gave a pixel at a white edge
`(0, 1.52, 0.33)` linear: x̄ and z̄ came from a read at 420 nm and ȳ from one at 510 nm.

## Measured: the wider `Chrome` (2026-10-28)

`Chrome` grew from 124 to 160 bytes with `backdrop_filter` (`ChromeRow` 64 to 80). On the AMD
mobile AMD iGPU the cascade benches did not move, and the frame fixture's CPU frames, which hold no pane,
rose about 2 % (`frame/cpu/cached` 131–135 µs to 135–137 µs), against up to 2.6 % between two
base runs.
