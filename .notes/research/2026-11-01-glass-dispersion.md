# Glass dispersion (2026-11-01)

Facts for the spectral dispersion of `BackdropFilter`'s lens.

## How others split color

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

## Glass

- N-BK7: `n_d` 1.5168, `V_d` 64.17, so `n_F − n_C` ≈ 0.0081. N-SF11: `n_d` 1.7847, `V_d` 25.68,
  so ≈ 0.0306 (Schott catalog values).
- Cauchy's offset at 400 nm is 1.7643 and at 700 nm −0.4390: across 400–700 nm the index moves
  2.2 times `n_F − n_C`.

## Measured

`integration/backdrop_frame/gpu/glass/*`, Apple laptop, full runs, the glass look's lens at
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
