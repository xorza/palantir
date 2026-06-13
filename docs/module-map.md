# Palantir module map

Point-in-time structural snapshot (production code only — tests, `test_support`, and `src/showcase/` excluded). Per file: `[loc]` and the types it defines.

| module | files | loc | types |
|---|--:|--:|--:|
| `(root)/` | 8 | 1673 | 21 |
| `primitives/` | 23 | 4661 | 39 |
| `common/` | 6 | 528 | 4 |
| `forest/` | 13 | 4771 | 57 |
| `layout/` | 28 | 8024 | 53 |
| `input/` | 7 | 2037 | 21 |
| `text/` | 2 | 1581 | 15 |
| `animation/` | 4 | 713 | 10 |
| `renderer/` | 34 | 11131 | 105 |
| `ui/` | 7 | 3852 | 30 |
| `widgets/` | 34 | 7045 | 69 |
| `winit_host/` | 4 | 863 | 13 |

## `(root)/`  — 8 files, 1673 loc
- **`context.rs`** [150] — HostContext, HostState
- **`debug_overlay.rs`** [95] — DebugOverlayConfig
- `lib.rs` [226]
- **`main.rs`** [233] — State, ShowcaseFn
- **`offscreen_host.rs`** [88] — OffscreenHost
- **`shape.rs`** [459] — Shape, PolylineColors, LineCap, LineCapBits, LineJoin, LineJoinBits, ColorMode, ColorModeBits, TextWrap
- **`window.rs`** [55] — WindowToken, WindowConfig, PendingWindow
- **`window_renderer.rs`** [367] — WindowRenderer, FrameTarget, FramePresent

## `primitives/`  — 23 files, 4661 loc
- `primitives/approx.rs` [74]
- **`primitives/background.rs`** [80] — Background
- **`primitives/bezier.rs`** [180] — CubicControls, CurveBounds
- **`primitives/brush.rs`** [843] — FillAxis, Stop, Raw, Spread, Interp, LinearGradient, RadialGradient, ConicGradient, Brush
- **`primitives/color.rs`** [664] — Color, ColorU8, ColorF16
- **`primitives/corners.rs`** [405] — Corners
- **`primitives/half_simd.rs`** [183] — F16x4
- **`primitives/image.rs`** [72] — ImageFit, Image
- **`primitives/interned_str.rs`** [86] — InternedStr
- **`primitives/lane_serde.rs`** [108] — LaneCodec, LaneVisitor
- **`primitives/mesh.rs`** [465] — MeshVertex, Mesh
- `primitives/mod.rs` [22]
- **`primitives/num.rs`** [37] — Num
- **`primitives/paint.rs`** [95] — FillKind, LutRow
- **`primitives/rect/mod.rs`** [233] — Rect
- **`primitives/shadow.rs`** [67] — Shadow
- **`primitives/size.rs`** [118] — Size, Raw
- **`primitives/spacing.rs`** [346] — Spacing, Sums
- **`primitives/span.rs`** [85] — Span
- **`primitives/stroke.rs`** [59] — Stroke
- **`primitives/transform.rs`** [165] — TranslateScale
- **`primitives/urect/mod.rs`** [184] — URect, URect16
- **`primitives/widget_id.rs`** [90] — WidgetId

## `common/`  — 6 files, 528 loc
- **`common/clipboard.rs`** [102] — Inner
- **`common/hash.rs`** [156] — Hasher
- **`common/live_arena.rs`** [160] — LiveArena
- `common/mod.rs` [9]
- **`common/platform.rs`** [21] — Platform
- `common/time.rs` [80]

## `forest/`  — 13 files, 4771 loc
- **`forest/element/mod.rs`** [861] — Gaps, LayoutMode, BoundsExtras, PanelExtras, LayoutCore, Salt, Element, ElementColumns, Configure, NodeFlags
- **`forest/frame_arena.rs`** [504] — FrameArena, FrameArenaInner, ChromeHashBytes, LoweredBrush
- **`forest/mod.rs`** [361] — CollisionRecord, Chrome, Layer, Forest
- **`forest/node.rs`** [99] — NodeRecord, SubtreeEnd
- **`forest/per_layer.rs`** [70] — PerLayer
- **`forest/rollups.rs`** [207] — NodeHash, CascadeInputHash, SubtreeRollups
- **`forest/seen_ids.rs`** [399] — IdHasher, WidgetIdMap, Endpoint, PendingExplicitCollision, EndpointOutcome, SeenIds
- `forest/shapes/hash.rs` [176]
- **`forest/shapes/mod.rs`** [214] — Shapes
- **`forest/shapes/record.rs`** [620] — GradientId, ShapeBrush, ShapeStroke, ChromeRow, LoweredShadow, ShadowGeom, LoweredGradient, ShapeRecord
- **`forest/tree/mod.rs`** [821] — NodeId, OpenFrame, RecordingScratch, RootSlot, PendingAnchor, Slot, ExtrasIdx, Tree, ChildIter, TreeItem, Child, TreeItems, GridArena
- **`forest/tree/paint_anims.rs`** [413] — PaintAnim, PaintMod, PaintAnimEntry, PaintAnims
- **`forest/visibility.rs`** [26] — Visibility

## `layout/`  — 28 files, 8024 loc
- **`layout/axis.rs`** [82] — Axis
- **`layout/cache/integration_tests.rs`** [588] — Build, Build
- **`layout/cache/mod.rs`** [470] — ArenaSnapshot, AvailableKey, SubtreeArenas, CachedSubtree, NodeArenas, CompactEntry, MeasureCache
- `layout/canvas/mod.rs` [128]
- `layout/cross_driver_tests/convergence.rs` [223]
- `layout/cross_driver_tests/fill_propagation.rs` [316]
- `layout/cross_driver_tests/mod.rs` [15]
- **`layout/cross_driver_tests/no_overlap.rs`** [366] — Case
- `layout/cross_driver_tests/stretch_semantics.rs` [178]
- `layout/cross_driver_tests/support.rs` [87]
- `layout/cross_driver_tests/text_wrap.rs` [820]
- **`layout/grid/mod.rs`** [1089] — HugKind, GridShape, AxisScratch, HugBound, GridScratch, GridContext, GridDepthStack, GridHugStore, GridHugSlot
- **`layout/intrinsic.rs`** [380] — LenReq
- **`layout/layoutengine.rs`** [874] — LayoutScratch, LayoutEngine
- **`layout/mod.rs`** [85] — LayerLayout, Layout, ShapedText
- **`layout/scroll/mod.rs`** [401] — ScrollLayoutState, OffsetBounds, TrackPage, ScrollStates
- **`layout/stack/mod.rs`** [447] — FillEntry, StackScratch, StackPlan
- **`layout/support.rs`** [404] — TextCtx, LeafTextShape, AxisCtx, JustifyOffsets, AxisAlignPair, AxisPlacement
- **`layout/types/align.rs`** [162] — HAlign, VAlign, Align, AxisAlign
- **`layout/types/clip_mode.rs`** [34] — ClipMode
- **`layout/types/display.rs`** [84] — Display
- **`layout/types/grid_cell.rs`** [38] — GridCell
- **`layout/types/justify.rs`** [20] — Justify
- `layout/types/mod.rs` [7]
- **`layout/types/sizing.rs`** [196] — Sizing, Sizes
- **`layout/types/track.rs`** [98] — Track, GridDef
- **`layout/wrapstack/mod.rs`** [345] — ChildPack, WrapScratch
- `layout/zstack/mod.rs` [87]

## `input/`  — 7 files, 2037 loc
- **`input/keyboard.rs`** [284] — Key, Modifiers, TextChunk, KeyPress, KeyboardEvent
- **`input/mod.rs`** [1085] — Capture, FocusPolicy, InputEvent, InputDelta, DragState, ResponseState, InputState
- **`input/pointer.rs`** [81] — PointerButton, PointerEvent
- **`input/policy.rs`** [27] — InputPolicy
- **`input/sense.rs`** [92] — Sense
- **`input/shortcut.rs`** [345] — Mods, Shortcut
- **`input/subscriptions.rs`** [123] — PointerSense, KeyboardSense, Subscriptions

## `text/`  — 2 files, 1581 loc
- **`text/cosmic.rs`** [612] — CacheEntry, CosmicMeasure, RenderSplit, BufferLookup, ShapedExtent
- **`text/mod.rs`** [969] — SelectionRects, FontFamily, TextShaper, ShaperInner, TextCacheKey, MeasureResult, CursorPos, TextReuseEntry, WrapReuse, LineFit

## `animation/`  — 4 files, 713 loc
- **`animation/animatable.rs`** [105] — Animatable
- **`animation/easing.rs`** [44] — Easing
- **`animation/mod.rs`** [449] — AnimSlot, AnimSpec, AnimRow, AnimMapTyped, TickResult, AnyTyped, AnimMap
- **`animation/spring.rs`** [115] — SpringStep

## `renderer/`  — 34 files, 11131 loc
- **`renderer/backend/curve_pipeline.rs`** [178] — CurvePipeline
- **`renderer/backend/debug_overlay.rs`** [224] — DebugOverlay
- **`renderer/backend/dynamic_buffer.rs`** [140] — DynamicBuffer
- **`renderer/backend/format_pipelines.rs`** [74] — FormatPipelines
- **`renderer/backend/gpu_ctx.rs`** [56] — GpuCtx
- **`renderer/backend/gpu_pass_stats.rs`** [224] — BatchKind, PipelineStats, Inner, GpuPassStats
- **`renderer/backend/gpu_timings.rs`** [453] — Slot, Inner, GpuTimings
- **`renderer/backend/gradient_resources.rs`** [143] — GradientResources
- **`renderer/backend/image_pipeline.rs`** [279] — ImagePipeline
- **`renderer/backend/mesh_pipeline.rs`** [202] — MeshPipeline
- **`renderer/backend/mod.rs`** [1128] — WgpuBackendConfig, Backbuffer, WgpuBackend, Bound
- **`renderer/backend/pipeline_utils.rs`** [183] — PipelineRecipe, StencilVariant, ColorVariantSpec
- **`renderer/backend/quad_pipeline.rs`** [382] — QuadPipeline
- **`renderer/backend/queue.rs`** [46] — Queue
- **`renderer/backend/schedule.rs`** [376] — RenderStep, ScheduleCursors, PerGroupBatch
- `renderer/backend/stencil.rs` [42]
- **`renderer/backend/text/atlas.rs`** [480] — GlyphSlot, Side, PendingGrow, GlyphAtlas, PendingCopy
- **`renderer/backend/text/encode.rs`** [408] — ResolvedRun, EncodedKey, EncodedRunKey, EncodedGlyph, EncodedEntry, EncodedCache, EncodeCtx
- **`renderer/backend/text/mod.rs`** [772] — StencilMode, GlyphInstance, Params, ContentType, TextBackend, MissEntry, BenchText
- **`renderer/backend/viewport.rs`** [91] — ViewportPush
- **`renderer/backend/write_stats.rs`** [29] — Stats
- **`renderer/caches.rs`** [23] — RenderCaches
- **`renderer/frontend/cmd_buffer/mod.rs`** [615] — BrushSource, GpuFillFields, CmdKind, PushClipPayload, DrawRectPayload, DrawShadowPayload, DrawTextPayload, DrawPolylinePayload, DrawMeshPayload, DrawImagePayload, DrawCurvePayload, RenderCmdBuffer
- **`renderer/frontend/composer/mod.rs`** [998] — Composer, ClipFrame, GroupCursors, OpenBatch
- **`renderer/frontend/composer/occlusion.rs`** [174] — Occluder, OcclusionPruner
- **`renderer/frontend/composer/text_grid.rs`** [304] — TileBucket, TextRectGrid
- **`renderer/frontend/encoder/mod.rs`** [658] — LayerCtx, Resolved
- **`renderer/frontend/mod.rs`** [107] — Frontend
- **`renderer/gradient_atlas.rs`** [870] — LutRowTexels, GradientCpuAtlas, GradientAtlas
- **`renderer/image_registry.rs`** [235] — ImageId, ImageHandle, ImageToken, ImageRegistry, Inner
- `renderer/mod.rs` [31]
- **`renderer/quad.rs`** [128] — Quad
- **`renderer/render_buffer.rs`** [385] — RenderBuffer, DrawGroup, TextBatch, MeshBatch, ImageBatch, RoundedClip, MeshScene, ImageScene, ImageDrawRow, ImageInstance, MeshDraw, MeshDrawRow, MeshInstance, TextRun, CurveBatch, CurveInstance
- **`renderer/stroke_tessellate/mod.rs`** [693] — StrokeStyle, ColorPlan, EdgeColors, Geo, InteriorJoin, Emitter, TessColorMode, TessStyle

## `ui/`  — 7 files, 3852 loc
- **`ui/cascade/mod.rs`** [797] — Paint, PaintArena, EntryRow, Frame, LayerCascades, Cascades, HitTargets, CascadesEngine, CascadePrefixBytes, PaintRectCtx
- **`ui/damage/mod.rs`** [925] — NodeSnapshot, PaintSnapArena, DamageEngine, DamageInput, Damage, ChangedLeg
- **`ui/damage/region/mod.rs`** [234] — DamageRegion
- **`ui/frame_report.rs`** [139] — RenderPlan, FrameProcessing, FrameReport
- **`ui/frame_state.rs`** [30] — State, FrameState
- **`ui/mod.rs`** [1529] — WakeReasons, FrameStamp, Wake, FramePlan, Ui
- **`ui/state.rs`** [198] — StateMap, Store, AnyTyped

## `widgets/`  — 34 files, 7045 loc
- **`widgets/button.rs`** [111] — Button
- **`widgets/checkbox.rs`** [115] — Checkbox
- **`widgets/combo_box.rs`** [158] — ComboState, ComboBox
- **`widgets/context_menu.rs`** [344] — ContextMenuState, ContextMenu, ContextMenuResponse, MenuItem
- **`widgets/drag_value.rs`** [170] — DragAnchor, DragValue
- **`widgets/frame.rs`** [45] — Frame
- **`widgets/grid.rs`** [148] — Grid
- **`widgets/mod.rs`** [394] — WidgetEntry, Response, ResponseSnapshot, InnerResponse
- **`widgets/modal.rs`** [123] — Modal, ModalResponse
- **`widgets/panel.rs`** [141] — Panel
- **`widgets/popup.rs`** [227] — ClickOutside, PopupHandle, PopupResponse, Popup
- **`widgets/progress_bar.rs`** [125] — ProgressBar, WeightSplit
- **`widgets/radio.rs`** [113] — RadioButton
- **`widgets/scroll.rs`** [788] — ZoomModifier, ZoomPivot, ZoomConfig, BarGeometry, BarLayout, BarPlan, BarMode, ScrollWrappers, Scroll
- **`widgets/separator.rs`** [79] — Separator
- **`widgets/slider.rs`** [258] — Slider
- **`widgets/spinner.rs`** [220] — Spinner
- **`widgets/switch.rs`** [181] — ToggleSwitch, SwitchGeom
- **`widgets/text.rs`** [112] — Text
- **`widgets/text_edit/mod.rs`** [1565] — TextEditState, EditSnapshot, EditKind, ShapeCtx, TextEdit, InputResult, VerticalMotion, VerticalDir, CharKind
- **`widgets/theme/button.rs`** [147] — ButtonTheme
- **`widgets/theme/context_menu.rs`** [120] — ContextMenuTheme, MenuItemTheme
- **`widgets/theme/mod.rs`** [313] — Theme
- `widgets/theme/palette.rs` [20]
- **`widgets/theme/progress_bar.rs`** [25] — ProgressBarTheme
- **`widgets/theme/scrollbar.rs`** [57] — ScrollbarTheme
- **`widgets/theme/slider.rs`** [32] — SliderTheme
- **`widgets/theme/text_edit.rs`** [132] — TextEditTheme
- **`widgets/theme/text_style.rs`** [93] — TextStyle
- **`widgets/theme/toggle.rs`** [147] — ToggleTheme
- **`widgets/theme/tooltip.rs`** [65] — TooltipTheme
- **`widgets/theme/widget_look.rs`** [146] — WidgetLook, AnimatedLook, StatefulLook
- **`widgets/toggle.rs`** [83] — ToggleChrome
- **`widgets/tooltip.rs`** [248] — PlacedAnchor, TooltipState, TooltipGlobal, Tooltip

## `winit_host/`  — 4 files, 863 loc
- **`winit_host/config.rs`** [44] — WinitHostConfig
- **`winit_host/gpu.rs`** [181] — Gpu, WindowSurface, GpuInit
- **`winit_host/handle.rs`** [88] — MainTask, UserEvent, HostHandle
- **`winit_host/mod.rs`** [550] — AppBuilder, App, WindowState, Bootstrap, Running, WinitHost

---

# Organization assessment

**Verdict: well-organized.** The layering is clean and matches the architecture documented in `CLAUDE.md`:

```
primitives ─► forest ─► layout ─► renderer ─► ui ─► widgets ─► winit_host / offscreen_host
(leaf types)  (tree)   (measure)  (frontend+   (recorder    (built on   (event loop / headless)
              common ──────────────backend)     + passes)    ui)
```

`primitives/` depends on nothing in-tree; each layer up depends only on those below it. Module boundaries map to responsibilities, not to convenience.

### What's good
- **Consistent naming idioms:** `*Pipeline` (GPU pipelines), `*Theme` (theme structs under `widgets/theme/`), `*Bits` (bit-packed enum reprs in `shape.rs`), `*Scratch` (reusable per-frame buffers), and the `handle + Inner` interior-mutability pattern (`GpuPassStats`/`GpuTimings`/`ImageRegistry`/`clipboard`/`HostContext`).
- **One canonical home per concept:** authoring `Shape` (`shape.rs`) vs lowered `ShapeRecord` (`forest/shapes/`); `Sizing`/`Align`/`Justify` vocab isolated in `layout/types/`.
- **Test split honored:** single-`mod.rs` dirs (`canvas/`, `zstack/`, `stack/`, `grid/`, …) exist because each carries a `tests.rs` sibling — the documented `foo/{mod.rs, tests.rs}` convention, not stray nesting.

### Watch list (by impact — optional polish, none are structural defects)
1. **`forest/tree/mod.rs` (821 loc, ~13 real types)** — the biggest grab-bag: `Tree` + 4 iterator types (`ChildIter`/`TreeItem`/`TreeItems`/`Child`) + recording scratch (`OpenFrame`/`RecordingScratch`/`RootSlot`/`PendingAnchor`) + `GridArena`. The iterators → `tree/iter.rs` and the record-scratch → `tree/record.rs` would leave `mod.rs` as just the `Tree` + storage.
2. **`renderer/render_buffer.rs` (16 types)** — a flat bundle of per-pipeline batch/instance/scene structs (`Mesh*`, `Image*`, `Curve*`, `Text*`). Cohesive as "the GPU draw buffer," but if it grows, split per pipeline.
3. **`ui/mod.rs` (1529 loc)** — the central `Ui` recorder plus frame-lifecycle helpers (`WakeReasons`/`Wake`/`FramePlan`/`FrameStamp`). The helpers could move to a `ui/frame.rs`; `Ui` itself is legitimately large.
4. **`input/mod.rs` (1085 loc, 7 types)** — `InputState` bundled with response/delta types (`ResponseState`/`InputDelta`/`DragState`/`Capture`). Those could split to `input/response.rs` (keyboard/pointer already are separate).
5. **`widgets/text_edit/mod.rs` (1565 loc)** — largest file; already in its own dir. Splittable (state / view / input handling) but text editing is inherently complex — low priority.
6. **Two `debug_overlay.rs`** — top-level (`DebugOverlayConfig`, the Ui-facing config) vs `renderer/backend/debug_overlay.rs` (`DebugOverlay`, the GPU resource). Each is correctly placed; the shared filename across layers is mildly confusing when grepping.

### Caveats about this map
- Counts exclude `#[cfg(test)]` modules (heuristic brace-tracking) and the whole `showcase/` demo (42 files / ~4.7k loc of example content, not architecture).
- `type` aliases that are trait-impl associated types (`Item`/`Output`/`IntoIter`/`Target`) are filtered out — they're not real type definitions.
- `GridArena` lives on the tree (`forest/tree/`) despite its layout-sounding name — intentional per the design (per-tree grid storage), flagged only as a grep heads-up.

