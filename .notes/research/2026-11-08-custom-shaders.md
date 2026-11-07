# Custom shaders: backgrounds, press ripples and effects past a widget's edge (2026-11-08)

Facts behind app-written shaders in palantir: how other toolkits let an app write one, how
press ripples work, how an effect paints past its node, and what each kind of effect needs from
the renderer. Android sources are read from AOSP `main` and AndroidX `androidx-main` on this
date. wgpu facts are read from wgpu 29.0.4 in the cargo registry, and palantir pins 30.0.1.

## Palantir today

- **One uber-shader for rects.** `gpu/pipeline/quad_pipeline/shader.wgsl` (1514 lines) shades
  every rect. A fill tag in `FillKind` picks the brush: solid, three gradients, two shadows, a
  triangle, a ramp and three backdrop kinds. Each tag number is substituted into the WGSL, and
  `every_pinned_shader_constant_is_read` fails if the shader stops reading one.
- **One instance layout.** `renderer::quad::Quad` is 64 B per instance: rect, fill, corners,
  stroke, fill kind, LUT row, fill axis and rotation.
- **Shader assembly.** `ShaderBody` puts `prelude.wgsl` in front of each body and substitutes
  `/*{MARKER}*/` constants. An unsubstituted marker panics. The prelude declares no binding.
- **The prelude contract.** `var<immediate> imm` holds the viewport size and `frag_origin`, the
  surface pixel that the target's pixel 0 is. It is not zero on a damage canvas, which holds one
  rect of the surface. `frag_px` gives a fragment's surface position on every target. Every
  fragment writes premultiplied color, as the blend is `PREMULTIPLIED_ALPHA_BLENDING`. Color is
  linear light, and the GPU encodes sRGB at the swapchain write.
- **Pipelines per format and per stencil mode.** `FormatPipelines` holds every pipeline for one
  swapchain format, so an sRGB and an HDR window bind different sets. `StencilVariant` pairs a
  base pipeline with a stencil-test twin for rounded clips. Both are built on the first frame
  whose census asks for them, not at startup.
- **Paint animation.** `shape::paint_animation` is a pure function of `now` at encode time, so
  an irregular `dt` does not drift. It drives alpha and rotation only. Its module doc refuses
  translation and scale: "Translation and scale would need a swept union the cascade can't
  bound without sampling". `PaintSteps::Steps(n)` wakes only on step boundaries.
- **Ink past the node.** `cascade::paint_rect` holds the area a node paints, which includes a
  drop-shadow halo past its rect. A clipping ancestor clips the halo, and only the clipped halo
  goes into damage (`damage/tests/clipping.rs`).
- **Backdrop capture.** `BackdropFilter` shows the pixels painted before a rect, blurred,
  saturated, grained or through a glass lens. It reads past the rect's edge. It costs a capture
  and a blur on each frame where something under it changes.
- **Raw wgpu escape.** `GpuPaint` draws into an offscreen target that a `GpuView` widget owns.
  The target composites through the image pipeline. `init` runs again on a new device.
- **Layers.** `Layer` paints `Main`, `Popup`, `Modal`, `Menu`, `Tooltip`, `Debug`, `Pointer`,
  bottom-up. A node has no z-index.
- **Registration precedent.** `Ui::load_image` returns an RAII `ImageHandle`. The texture lives
  until the last clone drops, and `is_resident()` tells an app to upload again after a device
  loss.
- **Preferences.** Reduced motion snaps transitions and stops decorative paint motion. Reduced
  transparency paints every backdrop filter as its fallback, with no capture or blur.
- **Errors.** `DeviceHealth::watch` sets an uncaptured-error handler that panics while the
  device is healthy, as wgpu's default does. `cutout_tables.rs` uses `push_error_scope` for a
  call that can fail.
- **naga is a dev-dependency only**, with `wgsl-in`, for the shader tests.
- **`.notes/TODO.md`** lists "custom shaders for backgrounds" and "wgpu features and limits
  request public api".

## How other toolkits let an app write a shader

### SwiftUI (iOS 17+)

- Three modifiers take a Metal `[[stitchable]]` function. `colorEffect` gets the position and
  the color the view would draw, and returns a color. `distortionEffect` returns the source
  position to sample at each destination pixel. `layerEffect` gets the position and a
  `SwiftUI::Layer` that it can sample anywhere.
- `maxSampleOffset` on the distortion and layer effects is the largest distance between a
  destination pixel and the position it samples. A value of (10, 5) lets the shader read x
  from −10 to width + 10 and y from −5 to height + 5.
- Shaders are stateless. Time comes from `TimelineView(.animation)`, which fires each frame,
  and the app passes the elapsed time as a float argument.
- Apple's WWDC24 sample `Ripple` takes `position`, `layer`, `origin`, `time`, `amplitude`,
  `frequency`, `decay` and `speed`. It delays the wave by distance from the origin, clamps the
  delayed time at zero, and scales a sine by an exponential decay.
- Sources: [VisualEffect reference](https://onestack.dev/ios-reference/SwiftUICore/VisualEffect),
  [Hacking with Swift: layer effects](https://hackingwithswift.com/quick-start/swiftui/how-to-add-metal-shaders-to-swiftui-views-using-layer-effects),
  [`maxSampleOffset` notes](https://zenn.dev/ikeh1024/articles/cc51846dfad295),
  [Design+Code: ripple from Apple's sample](https://designcode.io/swiftui-handbook-ripple-visual-effect),
  [WWDC26 session 322 notes](https://wwdcnotes.com/documentation/wwdc26-322-compose-advanced-graphics-effects-with-swiftui/).

### Android and Compose (AGSL, API 33+)

- `RuntimeShader` calculates a per-pixel color from an AGSL `main(vec2)`. Its output is
  premultiplied. Coordinates match the Canvas, origin at the upper left.
- `RenderEffect.createRuntimeShaderEffect(shader, name)` feeds a render node's pixels to the
  shader as the input named `name`. In Compose it goes on `Modifier.graphicsLayer`. The effect
  applies to the view and all its children.
- Sources: [RuntimeShader](https://developer.android.com/reference/android/graphics/RuntimeShader),
  [createRuntimeShaderEffect](https://learn.microsoft.com/en-us/dotnet/api/android.graphics.rendereffect.createruntimeshadereffect?view=net-android-37.0),
  [AGSL in Compose](https://xckevin.github.io/en/blog/android-agsl-runtimeshader-skia/).

### Skia (SkSL, `SkRuntimeEffect`)

- Three kinds: shader, color filter and blender. Each adds a function to one fragment shader
  that Skia assembles per draw.
- `main()` gets local coordinates. A child (`uniform shader`) is any Skia shader, read with
  `.eval()`, with no texture conversion first.
- Uniforms are not color-converted. `layout(color)` marks a color uniform, supplied as
  unpremultiplied sRGB and converted to the working space. Every shader returns premultiplied
  color.
- Source: [SkSL](https://skia.org/docs/user/sksl/).

### Flutter (`FragmentProgram`)

- Shaders are listed in `pubspec.yaml` and compiled at build time. `FragmentProgram.fromAsset`
  loads one, and each `FragmentShader` pairs it with its own uniform values.
- Float uniforms are set by index with `setFloat`, once per component. Samplers have their own
  index space through `setImageSampler`. An unset float is 0.
- `ImageFilter.shader` runs a shader as a `BackdropFilter` or `ImageFiltered`. The engine sets
  sampler 0 to the input image and floats 0 and 1 to its size. Only Impeller supports it.
- Sources: [fragment shaders](https://docs.flutter.dev/ui/design/graphics/fragment-shaders).

### Qt Quick (`ShaderEffect`)

- Vulkan-style GLSL, compiled offline with `qsb` into `.qsb` packages. Qt 5's inline GLSL
  strings are gone.
- A `std140` block at binding 0 must start with `mat4 qt_Matrix` at offset 0 and
  `float qt_Opacity` at offset 64. Custom uniforms follow.
- QML properties map to uniforms by name: `real` to `float`, `color` to a premultiplied
  `vec4`, `rect` to `vec4`, `point` and `size` to `vec2`, an `Image` or `ShaderEffectSource` to
  `sampler2D`.
- `mesh` sets a `GridMesh` resolution for vertex distortion, as a page curl needs.
- Several instances batch into one draw only with `supportsAtlasTextures` and exactly one
  sampler.
- "Shaders, including `.qsb` files, are treated as trusted. Don't load user-provided shaders."
- Source: [ShaderEffect](https://doc.qt.io/qt-6/qml-qtquick-shadereffect.html).

### Godot (`canvas_item` shaders)

- The app writes `vertex()`, `fragment()` and optionally `light()` in a GLSL dialect. The
  engine supplies built-ins: `TIME`, `UV`, `COLOR`, `TEXTURE`, `FRAGCOORD`, `SCREEN_UV`,
  `SCREEN_PIXEL_SIZE`, `REGION_RECT`, `INSTANCE_CUSTOM`, and others.
- The screen behind is a `sampler2D` with `hint_screen_texture`. Godot 4 removed the
  `SCREEN_TEXTURE` built-in.
- Render modes pick the blend: `blend_mix`, `blend_add`, `blend_sub`, `blend_mul`,
  `blend_premul_alpha`, `blend_disabled`.
- `TIME` rolls over after `rendering/limits/time/time_rollover_secs`, 3600 s by default,
  because "large floating-point values are less precise than small floating-point values". If
  it is too low, "shader animations may appear to restart".
- Sources: [canvas_item shader](https://docs.godotengine.org/en/stable/tutorials/shaders/shader_reference/canvas_item_shader.html),
  [rollover setting commit](https://remotebranch.eu/Stowage/godot/commit/25ae74b625e9dbb48ecba5fc84d4862d36eaf69a).

### Shadertoy

- The common inputs: `iResolution` (vec3, z the pixel aspect), `iTime`, `iTimeDelta`, `iFrame`,
  `iMouse` (xy while a button is held, zw the click position), `iDate`, `iChannel0..3` with
  `iChannelResolution` and `iChannelTime`.
- Sources: [shadertoy-cli prelude](https://docs.rs/crate/shadertoy-cli/latest/source/assets/docs/glsl.md),
  [three.js port](https://github.com/rocksdanister/threejs-shadertoy).

### Rust toolkits

- **Bevy `UiMaterial`.** A struct derives `AsBindGroup` and names a WGSL fragment shader, and
  `MaterialNode` attaches it to a UI node. The vertex shader is Bevy's unless the material
  overrides it. The fragment reads `UiVertexOutput` (`size`, `uv`, border and radius), and the
  material binds at group 1. Bevy 0.18.1's `ui_material_pipeline.rs` says "for now, no
  batching": each `UiMaterialBatch` is one draw. Issue 7345 reports 24,000+ draws in the
  `many_buttons` stress test where 3 would do.
  Sources: [UI material example](https://bevy.org/examples/ui-user-interface/ui-material/),
  [`ui_material_pipeline.rs`](https://docs.rs/bevy_ui_render/0.18.1/x86_64-unknown-linux-gnu/src/bevy_ui_render/ui_material_pipeline.rs.html),
  [bevy#7345](https://github.com/bevyengine/bevy/issues/7345).
- **iced `shader` widget.** A `Program` yields a `Primitive` whose `prepare` gets the device,
  queue, bounds and viewport, and whose `render` gets the encoder and target. Pipelines live in
  a storage keyed by `TypeId`. The docs prefer `draw` in the shared pass over a new pass in
  `render`. Sources: [Primitive](https://docs.iced.rs/iced_wgpu/primitive/trait.Primitive.html),
  [iced#2085](https://github.com/iced-rs/iced/pull/2085).
- **cranpose-ui-graphics**, a Rust port of Compose. `RenderEffect` is `Blur`, `Offset`,
  `Shader(Arc<RuntimeShader>)` or `Chain`. Uniforms are a flat `array<vec4<f32>, 64>` set by
  index. The app may use 0..220, and 220..256 hold the renderer's data: the effect rect, the
  input size, the source region, mask, radii and alpha. The input texture and sampler sit at
  group 0. `ShaderWarmUp` builds pipelines before the first draw. Source:
  [render_effect.rs](https://docs.rs/cranpose-ui-graphics/0.9.35/src/cranpose_ui_graphics/render_effect.rs.html).

### Unity

- UI Toolkit draws through one uber-shader with dynamic branches, up to eight textures a batch.
  A stencil mask change breaks a batch. A rectangular mask does not, as the shader does it.
- On uGUI, a custom shader breaks batches. Unity's answer: batches need one shader, so limit
  the custom shader to a block of elements to break only around it.
- Sources: [UI Toolkit performance](https://docs.unity3d.com/kr/current/Manual/best-practice-guides/ui-toolkit-for-advanced-unity-developers/optimizing-performance.html),
  [Unity discussion](https://discussions.unity.com/t/grayscale-and-colorize-image-support/763843).

## Shader infrastructure facts

- **Composition tools.** naga_oil composes modules into one naga module, with imports that
  cannot recurse; Bevy uses it. WESL 0.2 has imports, `@if`/`@elif`/`@else` conditional
  compilation and Cargo shader packages. Generics are future work. WESL compiles to plain WGSL.
  Sources: [naga_oil](https://github.com/Wumpf/naga_oil), [WESL](https://wesl-lang.dev/spec/README).
- **naga's WGSL parser returns `ParseError`** with `message()`, `location()` and
  `emit_to_string()`. It has a recursion limit, but a thread's stack can run out first.
  Sources: [naga front/wgsl](https://docs.rs/crate/naga/latest/source/src/front/wgsl/mod.rs).
- **wgpu's default uncaptured-error handler panics**: `default_error_handler` logs "Handling
  wgpu errors as fatal by default" and panics with `wgpu error: {err}` (wgpu 29.0.4
  `backend/wgpu_core.rs`). `ShaderModule::get_compilation_info()` returns the compiler's
  messages (`api/shader_module.rs`). A validation error scope catches a failed shader module, and
  Firefox reports "Shader module creation failed: Parsing error" with line and offset
  ([Firefox test](https://searchfox.org/mozilla-central/source/dom/webgpu/mochitest/test_compilation_message_pos.html)).
- **wgpu `PipelineCache`** works on Vulkan alone. `get_data` returns bytes to persist, keyed by
  `util::pipeline_cache_key`, written atomically. Desktop drivers keep their own caches, so the
  gain is mostly on Android. Source: [PipelineCache](https://wgpu.rs/doc/wgpu/struct.PipelineCache.html).
- **Shader compilation jank** drove Flutter to Impeller. Skia built shaders at first use, so the
  first run of a screen stuttered. Warm-up runs and bundled shaders moved the work to developers,
  grew apps, and still did not remove jank, and precompiled shaders are specific to a device or
  GPU on Android. Sources: [Flutter: shader jank](https://docs.flutter.dev/perf/shader),
  [Impeller FAQ](https://github.com/flutter/engine/blob/main/impeller/docs/faq.md).
- **Trust.** Qt treats every shader as trusted. A shader that runs too long trips a GPU reset
  (TDR), which `2026-10-30-device-loss.md` covers.

## Reading the destination pixel

- **wgpu 30 has no framebuffer fetch.** `wgpu-types` 30.0.1 `features.rs` has no feature for it,
  and naga has no WGSL syntax for it. wgpu#10148 ("RenderPipeline input attachments", opened
  2026-08-22) has a draft spec. A maintainer objects to the hidden barriers of subpasses that
  start on a pipeline bind, and the author plans a second draft on dynamic rendering. naga#5233
  (input attachments) is open since 2024-02-10. Sources:
  [wgpu#10148](https://github.com/gfx-rs/wgpu/issues/10148), [wgpu#5233](https://github.com/gfx-rs/wgpu/issues/5233).
- **Dawn has an experimental extension**, `chromium_experimental_framebuffer_fetch`: a fragment
  input with `@color(N)` reads the attachment's current value, in sync with earlier writes to
  that fragment in API order. "Specification work in the WebGPU group hasn't started". On Vulkan,
  Dawn needs the rasterization-order attachment access extension. Sources:
  [Dawn extension doc](https://dawn.googlesource.com/dawn/+/main/docs/tint/extensions/chromium_experimental_framebuffer_fetch.md),
  [Dawn Vulkan commit](https://dawn.googlesource.com/dawn/+/5d40cfd4cf77fee5a479e97e8912ca45b143ef0d).
- **Vulkan.** `VK_EXT_rasterization_order_attachment_access` (a rename of the ARM extension)
  reads attachments fragment to fragment in rasterization order with no explicit sync. With
  `VK_KHR_dynamic_rendering_local_read` it equals `VK_EXT_shader_tile_image`. Arm says
  `dynamic_rendering_local_read` targets all GPUs, and it is in the Vulkan Roadmap 2024
  milestone. No source found states desktop NVIDIA or AMD support. Sources:
  [proposal](https://docs.vulkan.org/features/latest/features/proposals/VK_EXT_rasterization_order_attachment_access.html),
  [Arm: framebuffer fetch in Vulkan](https://developer.arm.com/community/arm-community-blogs/b/mobile-graphics-and-gaming-blog/posts/framebuffer-fetch-in-vulkan).
- **Impeller** removes its backdrop blit where the device has framebuffer fetch (PR 55983,
  cited in `2026-10-28-backdrops.md`), so fetch is a speed path beside a copy that gives the
  same pixels.
- **Dual-source blending exists in wgpu 30** (`Features::DUAL_SOURCE_BLENDING`): a fragment
  writes two outputs, and the fixed blend can weigh the destination by the second. wgpu-hal
  30.0.1 sets it on Vulkan from `dualSrcBlend`, on Metal from MSL 1.2+ and the device's
  `dual_source_blending`, and on GL from `GL_EXT_blend_func_extended` or
  `GL_ARB_blend_func_extended` (`vulkan/adapter.rs`, `metal/adapter.rs`, `gles/adapter.rs`).
  With `src·One + dst·Src1`, the shader picks the destination weight per channel: `1 − α` is
  source-over, `1` is add, the color with a zero first output is multiply, and `1 − c` with `c`
  first is screen. Every blend linear in the destination needs one pipeline. Palantir requests
  no such feature now.

## Press ripples

### Material flat ripple (Compose `RippleAnimation.kt`)

- Start radius `0.3 × max(w, h)`. End radius: the half diagonal, plus 10 dp when bounded. A
  bounded ripple draws under `clipRect`, so the extra 10 dp only makes the edge reach the
  corners sooner.
- On press: alpha 0 → 1 in 75 ms, linear. Radius start → end in 225 ms, FastOutSlowIn. Center
  touch point → node center in 225 ms, linear.
- On release: alpha → 0 in 150 ms, linear, after the fade-in ends. A release during the fade-in
  holds alpha at 1 until then.
- `ripple()` returns an `IndicationNodeFactory`. `MaterialTheme` supplies it through
  `LocalIndication`, so `clickable` draws it by default. A press starts a new ripple. Hover,
  focus and drag show a state layer of fixed alpha.
- Source: [RippleAnimation.kt](https://android.googlesource.com/platform/frameworks/support/+/refs/heads/androidx-main/compose/material/material-ripple/src/commonMain/kotlin/androidx/compose/material/ripple/RippleAnimation.kt),
  [ripple API](https://composables.com/jetpack-compose/androidx.compose.material/material/functions/ripple/api).

### Android 12+ sparkle ripple (`RippleShader.java`, `RippleAnimationSession.java`)

- One AGSL `main(p)`: a soft disc whose radius grows with `progress`, a soft ring at the front of
  width `0.05 × radius`, and sparkles from a triangle-noise threshold, masked by the ring and by
  a turbulence of three turned circle grids. A shape mask comes from the input shader's alpha.
- Center: `mix(in_touch, in_origin, saturate(progress × 2))`, so it reaches the node center at
  half progress. `setRadius(r)` stores `r × 2.3`.
- Sub-phases of `progress`: fade in 0–0.13, noise fades out 0.4–0.5, the wave fades out 0.4–1.
- Enter 450 ms on FastOutSlowIn (`PathInterpolator(0.4, 0, 0.2, 1)`), exit 375 ms linear. The
  noise phase loops over 7000 ms.
- The CPU calculates the turbulence rotations as `(cos, sin)` pairs and passes them as uniforms.
- `RippleDrawable.MAX_RIPPLES = 10`. `tryRippleEnter` drops a new ripple past the limit: "This
  should never happen unless the user is tapping like a maniac".
- Sources: [RippleShader.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/graphics/java/android/graphics/drawable/RippleShader.java),
  [RippleAnimationSession.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/graphics/java/android/graphics/drawable/RippleAnimationSession.java),
  [RippleDrawable.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/graphics/java/android/graphics/drawable/RippleDrawable.java).

### Other libraries

- Angular Material's ripple has an enter phase and an exit phase. With no `radius`, it reaches
  from its center to the host's farthest corner. `centered` starts it at the center.
  `speedFactor` scales the enter duration alone. The mat-ripple defaults are 450 ms enter and
  400 ms exit. Sources: [Angular ripple API](https://material.angular.io/components/ripple/api),
  [mat-ripple](https://github.com/moustafajazzar/mat-ripple).

## Paint past a node's edge

- **CSS ink overflow** is the part of a box's paint outside its border box: box shadows, border
  images, text decoration, overhanging glyphs, outlines. It does not change layout or scrollable
  overflow. The editor's draft leaves its extent undefined for `box-shadow`, which has no defined
  visual extent. Source: [CSS Overflow 3](https://drafts.csswg.org/css-overflow/).
- **SwiftUI** bounds a distortion or layer effect's reads with `maxSampleOffset` (above).
- **Android borderless ripples** draw on the nearest parent with a non-null background, and that
  parent's bounds clip them. An unbounded `RippleDrawable` "will be drawn atop the first
  available parent background", and "the drawing region may extend outside of the Drawable
  bounds". A transparent background is not null, so it catches the ripple. Sources:
  [Material animations](https://developer.android.com/training/material/animations),
  [RippleDrawable](https://developer.android.com/reference/android/graphics/drawable/RippleDrawable),
  [boundless projection commit](https://android.googlesource.com/platform/frameworks/base/+/ea2a064%5E%21).

## Water

- **Analytic ring.** A sine of distance, delayed by distance over speed, under an exponential
  decay: Apple's WWDC24 `Ripple`. It is a function of time alone, with no state.
- **Heightfield simulation.** Hugo Elias's method keeps two buffers of signed heights. Each step
  sets a cell to the average of its four neighbors, times two, minus its value in the older
  buffer, then damps it, and the buffers swap. A shading pass offsets texture reads by the
  height differences of neighbors. On a GPU each buffer is a render target and a fragment does
  one cell. A shader cannot read and write one texture, so the two alternate. An 8-bit texture
  holds `[0, 1]` with low precision, too little for signed heights. Sources:
  [Matyka](https://maciejmatyka.blogspot.com/2018/03/2d-water-algorithm.html),
  [9bitscience](https://9bitscience.blogspot.com/2014/05/realtime-2d-wave-simulation.html),
  [Khronos thread](https://community.khronos.org/t/2d-wave-ripple-algorithm-problem/75412),
  [Silverlight sample](https://learn.microsoft.com/de-de/archive/blogs/nikola/building-the-reallistic-water-ripples-sample-in-silverlight).

## Continuous effects and power

- Microsoft disabled reveal-style effects by default as "GPU intensive tasks". WinUI 2.6
  dropped Reveal highlight to match Microsoft's web and mobile products, which had none. Sources:
  [Fluent Design on Ribbon-like Pivot](https://learn.microsoft.com/de-de/archive/blogs/juanmejia/fluent-design-on-ribbon-like-pivot),
  [Fluent design](https://www.wikiwand.com/en/Fluent_design).
- WinUI acrylic falls back to its `FallbackColor` in Battery Saver
  ([acrylic](https://learn.microsoft.com/windows/apps/design/style/acrylic)).

## Accessibility

- WCAG 2.3.3 (AAA): motion from interaction can be disabled unless it is essential. Sufficient
  techniques: CSS `prefers-reduced-motion` (C39) and its script check (SCR40). Guides advise
  reducing motion to a fade with no movement instead of removing all animation, and warn that a
  global `animation: none` breaks spinners and progress. Sources:
  [Understanding 2.3.3](https://w3c.github.io/wcag/understanding/animation-from-interactions),
  [C39](https://www.w3.org/WAI/WCAG22/Techniques/css/C39.html).

## Effects and what each reads

| Effect | Reads | Time | Past its node |
|---|---|---|---|
| Flat ripple | position, press origin and times | function of time | no (bounded) |
| Sparkle ripple | the same, plus noise | function of time | no |
| Ring around a button | node rect, press time | function of time | yes, to a known radius |
| Animated background | position, size | function of time | no |
| Static procedural background | position, size | none | no |
| Shimmer on placeholders | surface position, one shared clock | function of time | no |
| Pointer spotlight (Reveal) | position, pointer | none | no |
| Turned border light | position, corners | function of time | no |
| Analytic water over content | the pixels under it | function of time | to its read offset |
| Simulated water | its own last frame | stepped state | its own target |
| Dissolve, pixelate, glow | the node's own pixels | function of time | glow: yes |
| Gooey merge (smooth min) | several shapes at once | any | the group's bound |
| Particle burst | many instances | stepped state | yes, large |
| Circular reveal | the old and new frame | function of time | the whole region |

- **Smooth minimum.** The polynomial `smin` is the usual 2D merge and depends on argument order.
  The exponential and power forms do not, and take more than two distances. Source:
  [Inigo Quilez: smin](https://iquilezles.org/articles/smin).
- **Circular reveal on the web** animates a `clip-path: circle()` on the View Transitions API's
  new-state pseudo-element from the toggle's position, with the default cross-fade turned off.
  Source: [theme toggle with View Transitions](https://akashhamirwasia.pages.dev/blog/full-page-theme-toggle-animation-with-view-transitions-api).
