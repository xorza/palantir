# Showcase reel

A calm motion-graphics reel of palantir's main features, rendered frame by frame by palantir
itself, and the library features it needs. Facts and sources:
`.notes/research/2026-11-01-reel.md`.

## Theme transition

- **A theme change animates the `Theme` tree.** `Theme` and every sub-theme derive `Animatable`;
  each frame shows `lerp(from, to, t)`, so colors, paddings, gaps, radii, strokes and font sizes
  move with real layout. A field that cannot blend switches at `t = 0.5`, as Flutter's
  `ThemeData.lerp`; under a spring, which has no phase, it switches at the start. Refused:
  interpolating `Palette` and `Metrics` and rebuilding through `Theme::from_parts`, which loses
  or snaps every edit an app makes after `from_parts`; a snapshot of the old frame under a mask
  (Telegram), which freezes moving content and moves no layout.
- **`set_theme_for` follows one setting on the `Ui`.** `Ui::set_theme_transition(Option<AnimationSpec>)`,
  per window; `None` snaps. A scheme change, from the system or `set_scheme_preference`, follows
  it too, as Flutter's `MaterialApp.themeAnimationStyle`. Refused: a separate animating call,
  one more method for a choice the setting already holds; a spec in `Metrics`, a field the
  transition itself would snap; a spec per call, which a system change has no call to carry.
- **Default 300 ms `OutCubic`.** An app that keeps the transition on while it sets a theme every
  frame lags 89 ms behind the edit; `InOutCubic` would lag 24 s, a freeze (research, "Measured").
  A live editor sets `None` for a continuous edit. The reel sets its own spec.
- **A second change starts from the current mix**, by the row rules every animated value has:
  CSS reversal for a duration, the kept velocity for a spring.
- **Reduced motion snaps**, as `Ui::animate` does.
- **Open — one theme per window.** The reel's slides share one canvas, so slide 4's morph changes
  every slide. Until a subtree theme exists (Flutter's `Theme` widget), slide 4 returns to its
  start theme before the camera leaves it.

## Animation

- **Every color animates in OKLab, premultiplied.** `RgbaF32`'s motion space becomes OKLab, so a
  theme transition and a look fade follow one curve and a dark-to-light change is perceptually
  even (CSS Color 4's default). Its settle tolerance is derived again in OKLab, under one 8-bit
  sRGB step at the darkest step. Refused: OKLab for the theme alone, two curves for one kind of
  motion; linear light, which reads bright too early; encoded sRGB (Flutter), less even than
  OKLab.
- **A look follows a moving theme with no delay.** A look row keeps its state's key; a new target
  under the same state passes through at once while the row's offset decays on its own clock, and
  a new state starts a new offset in motion space. Duration rows hold an offset from the target,
  as spring rows do. Today a look lags 29–35 ms behind a moving target, by refresh rate
  (research, "Measured"). Refused: snapping looks during a transition, which pops the clicked
  widget's own state; today's retarget, as Flutter's implicit animations do. Source: UIKit's
  additive animations (iOS 8). `Ui::animate` callers carry no state and keep today's retarget.

## Capture and pointer

- **The harness captures one frame at a set time.** A public call renders one frame at the clock
  the caller parked (`Harness::at`) and returns its pixels after the GPU finishes, running no
  other frame. Refused: `Harness::shot`, which runs frames until the picture lands and moves the
  clock; `read_back` under `internals`, which the reel example cannot rely on.
- **A pointer layer paints last and is never hit.** A new `Layer` above `Debug` for a cursor an
  app draws itself, here the reel's. Refused: drawing the cursor after readback, a second
  renderer with its own anti-aliasing; `Debug`, which is hit first and would take the clicks.

## The reel

- **An example of slides on one canvas.** `examples/reel`: each slide is a live palantir subtree
  on one pan-zoom canvas; a transition is a slow camera glide through `Scroll::set_offset` and
  `Scroll::set_zoom`, mostly pans, on the van Wijk–Nuij path with Reach and North's smooth ends.
  The last slide zooms out to the whole canvas. Refused: shared-element morphs between every pair
  (Keynote Magic Move), a design per pair, kept for one or two; crossfades; cuts.
- **Choreography is Rust.** A timeline per slide: cursor moves, clicks, keys, text, holds, theme
  changes, camera glides. Refused: extending the shot script, whose timing, easing and overlap
  would grow a language (Motion Canvas times in code for the same reason).
- **Exact frame times.** Frame `n` is at `n / fps` seconds, not a sum of `dt`.
- **The cursor feeds real input.** Each frame the pointer position goes in as hover or press
  input, so widgets animate their own states. It moves on a minimum-jerk path (Flash and Hogan),
  its duration from Fitts' law with a calm factor, with a slight arc; a click presses the cursor
  and grows a fading ring over about 400 ms. Refused: a cursor composited in Python on the PNGs.
- **Logical canvas 1280×720.** Preview at scale 1 (1280×720), final at scale 3 (3840×2160): both
  integer, so layout and pixel snapping match. Each logical pixel is 1.5 px at 1080p and 2 px at
  1440p, so stock text stays legible. Refused: logical 1920×1080, which needs scale 2/3 for the
  preview and reads small at 1080p.
- **Preview 720p60, final 4K120.** The final delivers 1080p and 1440p downscaled from the 4K
  render; a post camera zooms at most 1.5×, where 1440p text stays sharp.
- **Frames stream to ffmpeg.** Raw RGBA on its stdin; a PNG folder only to inspect a frame or a
  range. Refused: a PNG sequence of every frame, tens of GB written and read again.
- **Encode as BT.709.** `libx264 -crf 16–18 -preset slow -pix_fmt yuv420p -movflags +faststart`,
  `scale=out_color_matrix=bt709`, BT.709 tags; without the conversion browsers show shifted
  colors.
- **Motion blur is optional.** Sub-frames averaged in linear light, then encoded to sRGB once.
  Refused: averaging sRGB bytes, which darkens light-dark edges.
- **Music: a calm Pixabay track, cuts on its bars.** At 80 BPM in 4/4 a bar is 3 s; slide lengths
  round to whole bars. ffmpeg adds it with `-c:a aac -b:a 192k` and an `afade` out. The reel
  tells its story muted, as muted autoplay is the common case. Refused: the YouTube Audio Library,
  whose standard tracks are for YouTube; silence, empty with sound on.
- **The claim is "every frame drawn by palantir, in Rust".** ffmpeg encodes and the music comes
  from outside, so "made only with palantir" is false.
- **Slide 9 credits MCP to Darkroom.** palantir has `Harness` and `Remote` over one set of actions
  and locators, and no MCP server; Darkroom serves its window over MCP on top of them.

## Storyboard

About 105 s, rounded to bars.

| # | Slide | What moves |
|---|---|---|
| 0 | Title | The logo, then "An immediate-mode GUI library for Rust". |
| 1 | Code is the UI | The counter code beside its live window; a click changes the number and the code line lights up. |
| 2 | Two-pass layout | A window grows and shrinks; Hug and Fill columns share the space again. |
| 3 | Widgets | A switch, a slider, a combo box, typed text. |
| 4 | Theme morph | Dark to light and a density preset; colors, margins and sizes move with real layout. |
| 5 | Materials | Frosted glass, then refracting glass. |
| 6 | Crisp at any scale | An in-app zoom into icons, gradients and shadows. |
| 7 | Dock and windows | A tab drag splits a pane. |
| 8 | Raw wgpu in a widget | A `GpuView` scene in a rounded widget that clips and stacks. |
| 9 | Drive it like a person | One line of actions plays in a headless test that passes, a live window, and an agent. |
| 10 | Fast | "78 µs a frame (Apple laptop)", "0 allocations a frame". |
| 11 | Made with palantir | The camera pulls back to the whole canvas; slide 4's timeline plays beside its slide. |
| 12 | Outro | The logo, `cargo add palantir`, the GitHub URL. |

- **Open — slide 1's colored code.** One `Text` per token in a row, or a rich-text widget with
  colored spans, a feature palantir lacks.
- **Open — the final frame rate.** YouTube lists rates up to 60. A 120 fps render can deliver 60 fps
  by averaging each pair, a 180° shutter.

## Plan

Library first, the reel last. Each step lands with its tests and its decisions folded into
`AGENTS.md` or the doc comments of the code it governs.

1. **OKLab motion space.** `RgbaF32`'s `to_motion`/`from_motion` in premultiplied OKLab; the
   settle tolerance derived again. Tests: exact ends, hand-computed midpoints against the OKLab
   reference, a spring settling within one 8-bit step; goldens with fades re-baselined.
2. **State-keyed offsets.** Duration rows store an offset from the target; look rows carry their
   state key. Tests: a target moving under one state paints the target every frame; a state
   change fades over its spec; a reversal mid-fade keeps the CSS shortening; the two models agree
   on a fixed target.
3. **`Animatable` theme tree.** The derive on `Theme` and every sub-theme; a mode for fields that
   switch at `t = 0.5`. Tests: midpoint values of a hand-built pair; each discrete field before
   and after 0.5; ends exact.
4. **Theme transition on the `Ui`.** `set_theme_transition`, default 300 ms `OutCubic`;
   `set_theme_for` and scheme changes animate under it; a scratch theme reused, so a running
   transition allocates nothing after its first frame. Tests: the theme at a parked time; a second
   change mid-way; reduced motion; `None`; an alloc audit fixture over a running transition.
5. **Showcase follows.** `Appearance::apply` sets `None` for continuous edits and keeps the
   transition for discrete ones.
6. **Frame capture.** The public one-frame capture at a parked time. Tests: one frame run per
   capture, the clock unchanged, pixels equal to `shot` of the same frame.
7. **Pointer layer.** Tests: painted above `Tooltip` and `Debug`; a click under the cursor still
   reaches the widget.
8. **Reel skeleton.** `examples/reel`: the canvas, the slide set, the camera with its glide path,
   logical 1280×720.
9. **Recorder.** The timeline, exact frame times, the cursor path and click ring on the pointer
   layer, the bar grid, PNG and ffmpeg outputs, 720p60 preview. Tests: the minimum-jerk path's
   ends, its zero end velocities and its peak to mean speed ratio of 15/8; a timeline's frame
   times; the bar rounding.
10. **Slides**, one at a time, in storyboard order.
11. **Motion blur**, if time allows.
12. **Final render.** 4K120, encodes, the 1080p and 1440p deliveries, the music.
