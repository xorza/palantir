# Structural redesign and implementation plan

This plan closes the findings in `.notes/REVIEW.md`, `.notes/TEST_REVIEW.md` and `.notes/ISSUES.md`
with no change to the public API. Every change that adds, renames, removes or re-signs an exported
item is in `.notes/API_CHANGES.md`. Where a step here is only an interim for an item in that file,
the step says so.

When a step is done, delete it. When a phase has no steps, delete the phase. When you close a
finding, delete it from its source file too.

Line numbers are against `50b34a49` (the later commits touch only `.notes/`).

---

## How the findings group

| Id | Root cause | Findings it closes |
|----|------------|--------------------|
| D1 | Input events apply as they arrive, but widgets read one collapsed state per frame | Same-batch gestures, TextEdit `held()`, lost clicks, `ButtonState` panic, keys after Escape/Enter, capture edges lost on `PaintOnly`, multi-click run |
| D2 | Platform text and timing policy is split between the host and the classifier | macOS Option text, AltGr, Super, synthetic keys, Shift+wheel, clock skip, scale at conversion, 0×0 minimize, `Suboptimal`, `SystemFacts` |
| D3 | One authoring hash serves layout, cascade and damage, and the gates list their inputs separately | `PaintAnim` not hashed, font epoch in one gate only, measure cache misses on paint, root order, id reservation, `by_id` reallocation |
| D4 | The cascade computes paint bounds with its own geometry instead of the encoder's | `ImageFit::None` overflow, Bézier bbox, icons above 512 px, text scissor, no-op rounded-clip rows, NaN rect ops, mesh indices |
| D5 | Radii are not normalized, and triangle vertices lose precision in f16 lanes | Corner radii above half extent, drop-shadow spread radius, triangle lanes, curve kind order |
| D6 | Colour interpolates in straight alpha, and LUT texels are sampled off-centre | Mesh, curve and gradient fringes, LUT skew, render target encoding |
| D7 | The animation tick has no rule for what a motion's first frame spends | Retarget `dt`, NaN targets, spring floor on colours, mid-curve snap |
| D8 | File data is screened at each use site instead of at the deserialization boundary | Theme scalar panics, `default_*` bound collisions, dock depth on load |
| D9 | Wheel routing picks the topmost scroll sense, whatever it can pan | TextEdit and TabStrip take the page's wheel |
| D10 | Overlay layers do not inherit their owner's cascade state | Disabled trigger with open popup, Tooltip owner, composite widget disabled lag |
| D11 | Text maps bytes to cursors and segments with its own rules, not cosmic's | Probe `\r` mapping, truncated probe, RTL wrap floor, font name table, encode key |
| D12 | Widget behaviour defects with local causes | ColorPicker, hue, numeric ranges, Tabs, TextEdit, clipboard, scroll, expander |
| D13 | Worst-case frame cost is not bounded | Occlusion prune, higher-kind scan, damage inversions, premultiply table, prewarm, GPU view repaint |
| D14 | Layout drivers disagree between measure and arrange | WrapStack budget, ZStack/Canvas Hug, cache plumbing, Hug scroll doc |
| D15 | The test harness cannot express "this pass" or "this widget" | TEST_REVIEW groups 1–5, 10–16 |
| D16 | No test compares a retained result with the result computed from scratch | Every "gate misses an input" bug in D3 and D4, and the next one |
| D17 | Docs, style rules and dependencies drifted | REVIEW doc and style groups, ISSUES.md, TEST_REVIEW 17–18 |

Two principles apply to every design below:

- **Derive, don't list twice.** Where two consumers need overlapping inputs, one is computed from
  the other, so an input added to the narrow one reaches the wide one by construction. Most of the
  scene bugs are two lists that drifted apart.
- **Prove a retained result with a cold one.** Every cache and every incremental path gets a
  differential test (D16). A missed input then fails a test instead of leaving stale pixels.

---

# Part 1: Designs

## D1. Input: trickle the event queue

### Problem

`Ui::on_input` applies each event to `InputState` at once, and a widget reads one collapsed state
per frame. `Capture` has three slots (`press`, `release`, `run`), and `frame_keyboard_events` is
one list. So when several gestures arrive between two frames:

- drag-release-press gives `phase = Down` with `drag = Stopped`, and `ButtonState::new` fails its
  debug assert (`src/input/response/button_state.rs:40`);
- press-release-press-release keeps the second release only, so `clicked()` fires once;
- press-release gives `Up{click}` with no `Down` frame, so `TextEdit` never places the caret
  (`src/widgets/text_edit/input_pass.rs:134-160`);
- `[Escape, 'a']` reports `cancelled` with `a` inserted, and `[Enter, 'x']` reports `submitted`
  while the buffer already holds `x` (`input_pass.rs:184-201`).

Three paths also end a press without the release edge that both collations read: `abandon_press`
in `end_frame` raises no signal, `begin_press` overwrites a live press, and a missed press sets
`cap.press = None` (`src/input/capture.rs:76`, `src/input/input_state/mod.rs:491,706-727`). A
missed press or another button's press leaves `run` untouched.

### How other frameworks do it

- **Native toolkits and browsers** (Win32, AppKit, GTK, Qt, the DOM) dispatch one event at a time
  to the current focus owner. When Escape's handler blurs a field, the next `a` goes to the new
  focus owner. A press and its release are two dispatches, so a widget always sees the press.
- **Dear ImGui** (immediate mode, one state read per frame) gets the same result with input
  trickling, on by default since 1.87 (`io.ConfigInputTrickleEventQueue`). `UpdateInputEvents`
  stops at the first event that would make the frame ambiguous: a second change of the same mouse
  button, a second change of the same key, or text after a key change in the same frame
  ("interleaved keys and text"). That event and all later ones wait for the next frame, in order.
- **egui** applies every event in one frame and has `TextEdit` `break` out of its event loop on
  Escape and on a single-line Enter. The keys after the break are not lost to the frame, but they
  are still read under the old focus.

Trickling is the immediate-mode form of the native model: each frame sees at most one
state-changing event of each kind, so focus and capture move between events as they would under
per-event dispatch. egui's `break` fixes only the widget that remembers to do it. This plan takes
trickling (decision D-3).

### Design

**`InputQueue`.** New `src/input/input_queue.rs`, owned by `InputState`, with
`pending: VecDeque<(InputEvent, Duration)>` (retained capacity) and a per-frame record of what
already changed: `buttons: [bool; PointerButton::COUNT]` and `command_key: Option<Key>`.

`InputState::on_input` asks the queue first. The event waits when `pending` is not empty (order is
kept), or when one of these rules holds:

1. It is a press or release of a button that already changed this frame.
2. It is a `KeyDown` after this frame's command key, unless it is a repeat of that same key. A
   *command key* is a `KeyDown` with empty text whose key is not a bare modifier: Escape, Enter,
   Tab, arrows, Backspace, Delete, and every chord. A run of text-producing presses therefore
   stays in one frame, and each command key ends the frame's keyboard batch. Repeats of the held
   key stay together, so a held Backspace does not fall behind on slow frames.

`InputEvent` has no key-up variant, so these two rules cover the keyboard. Pointer moves, wheel
and modifier changes wait only to keep order. ImGui also holds pointer moves after a button
change; here the press hit-tests at event time with its own position and drag travel accumulates
on the press, so that rule buys nothing.

**Replay.** `FrameCycle` clears the per-frame record and replays `pending` at the start of a frame,
**before `take_frame_plan`**, so the replayed events raise the signal that makes this frame a
record frame. Replay stops at the first event that must wait again. Each event keeps its arrival
time, so double-click timing is unchanged. A waiting event returns an `InputDelta` that asks for a
repaint, and a frame that ends with a non-empty queue sets `repaint_requested`, so every host
(winit, offscreen, harness) runs the next frame with no API change.

**Results.**

- `Capture` never holds a release and a new press for one button in one frame, so the
  `ButtonState` invariant holds by construction. Keep the debug assert, and fix the slot comments
  at `input_state/mod.rs:270-271,288-289`.
- `[Escape, 'a']`: Escape blurs the field in frame N; `a` arrives in frame N+1 and goes to the new
  focus owner. `[Enter, 'x']`: the caller reads the submitted value in frame N, and `x` is typed in
  frame N+1. This is the native result.
- `TextEdit`'s drain then never meets a key after its own terminal key in one frame. It still stops
  at `Blur` and at a single-line submit, with a `debug_assert!` that no key follows, so a later
  change to the trickle rules fails loudly.

**One exit for a press.** `Capture::end_press` stays the only way a press ends: `begin_press` on a
live press first ends it (`Miss`, or `DragStopped` if a drag latched); a missed press calls
`abandon_press`. Every `end_press` that writes `Click` or `DragStopped` raises
`InputSignal::Action`. In `end_frame`, eviction runs after `drain_per_frame_queues` (which clears
`signal_since_last_frame`), so the signal it raises survives to the next `take_frame_plan`, and
that frame records instead of `PaintOnly`.

**A press resets the run.** A miss sets `run = None`, and a press of another button clears this
button's run. This is the native rule (AppKit `clickCount`, the Win32 double-click test).

**Modifier-only keys do not settle** (`input_state/mod.rs:606-612`). A bare modifier `KeyDown` is
not a command key either, so it does not split a typing run.

### Harness impact

`h.click_at(p); h.frame(record)` becomes two frames: the press frame and the release frame. Phase 0
adds `UiHarness::frames_until_input_idle(record) -> Vec<Passes<R>>` (one entry per frame, values per
pass), and the click helpers say in their docs how many frames a gesture takes. Expect churn in the
29 `click_at`, 43 `press_at` and the widget click sites (TEST_REVIEW 4). Move them in the same step
as the queue, not after.

### Tests

- Batch table, each row hand-derived: `[P]`, `[P,R]`, `[P,R,P]`, `[P,R,P,R]`, `[P,move>4,R,P]`,
  `[Esc,'a']`, `[Enter,'x']`, `['a','b',BS,'c']`, `[BS, BS repeat ×5]`. Assert the exact
  `ButtonState`, the keys each frame delivers, and the number of frames until the queue is empty.
- Differential (D16): for each row, one event per frame and the batch give the same final widget
  state and the same click count.
- TextEdit: a tap places the caret at the hit index.
- Eviction mid-drag: the next frame is `FullRecord`, and the slider commits once.
- Run reset: click A, click empty, click A within 300 ms → `count == 1`.
- Ctrl+C with a field focused costs two record passes, not four.

## D2. Host: one rule for what a key typed, one clock for input

### Problem

`KeyClass::of` treats a press as text only when `!mods.any_command()`, and `any_command` counts
`alt`. On macOS, Option is the text modifier, so `@ [ { |` on German layouts and `™ å` on US never
type (`src/input/key_class.rs:87`, `src/host/winit/input/mod.rs:244`). `TextEdit::apply_key`
repeats the gate (`input_pass.rs:254`). AltGr on Windows is Ctrl+Alt and is blocked too. Super has
no modifier bit, so Super+L types `l`. Synthetic presses on focus gain are not filtered
(`host/winit/input/mod.rs:88`).

Input is stamped with the frame clock, which `clock.skip` rewinds after occlusion
(`host/winit/window.rs:159-164,245-252`). Events are converted with the live user scale, not the
scale the cascade used (`window.rs:207`).

### Design

**One predicate for "this press typed text".** `KeyPress::types_text(self) -> bool`
(crate-private), used by `KeyClass::of` and by `TextEdit`:

```
!self.text.is_empty() && !(self.mods.ctrl && !self.mods.alt)
```

Ctrl without Alt is a command on every platform; Ctrl+Alt with text is AltGr; Alt or Option with
text is a composed character. The rule is platform-neutral, so it also classifies a `KeyPress` that
an app or a test builds by hand. `any_command` keeps deciding `Edit` vs `Accel` for presses that
did not type. This changes the behaviour of the public `KeyClass::of`, not its signature.

**The host clears text that is not input.** `host/winit/input` stays the place that knows the
platform. It empties `KeyPress::text` for a Cmd chord on macOS (the host maps Cmd to `ctrl`, which
the predicate already rejects, so this is defence in depth) and for any press with Super held
(`ModifiersState::super_key`). It drops `KeyboardInput { is_synthetic: true, .. }` presses. Write
the translation as a pure function that takes a `Platform`, so Linux CI checks every platform's
table (TEST_REVIEW 13 asks for the same). A Super bit on `Modifiers` is API_CHANGES A10; this fix
does not need it.

Verify each row against a real key dump before you trust it: the macOS rows on the macOS test
laptop, the Linux rows here. No Windows host is available, so the AltGr row stays marked unverified
in its test name until someone dumps it.

**Shift+wheel is horizontal on Windows and Linux** (decision D-4). In `InputState`'s scroll arm, a
`ScrollLines` or `ScrollPixels` with Shift held and a zero x component swaps its axes when the
platform is not macOS (macOS sends a horizontal delta itself). The platform comes in at
`InputState` construction from `common::platform`, so tests sweep it. Shift is not a
`ZoomModifier`, so nothing else reads Shift+wheel.

**Two clocks.** The winit `Window` stamps input from a private monotonic `Instant` origin that is
never skipped. The frame clock keeps `skip` for animation. `Capture` compares input stamps only
with input stamps. The public `Clock` trait does not change, and `OffscreenHost` keeps one clock
(it never occludes).

**Convert with the laid-out scale.** Divide by `ui.display().scale_factor()`, the scale the current
cascade used. `resync_pointer` already restates the pointer when the scale moves at the frame. Fix
the `effective_scale` doc, which argues the opposite.

**Window lifecycle.**

- `Resized(0, 0)` (a Windows minimize) sets the window occluded and skips frames, as
  `Occluded(true)` does, instead of clamping to 1×1 (`host/winit/mod.rs:426-446`).
- A `Suboptimal` texture is presented, then the surface is reconfigured; wgpu documents that a
  suboptimal texture is still presentable (`window.rs:394-405`).
- `Resized` invalidates only `maximized`; `Moved` covers position (`host/winit/mod.rs:427`).

### Tests

- Translation table per platform (pure function), each row from a key dump.
- `types_text` table: every modifier set × text present or not.
- Shift+wheel table: Linux and Windows swap, macOS does not, Ctrl+Shift+wheel still zooms.
- Clock: press, occlude 5 s, press within 500 ms of frame time → `count == 1`.
- Scale: `set_user_scale`, then a click before the next frame hits the widget under the old layout.

## D3. Scene: derive the layout hash, the full hash and the gates from one source

### Problem

`Tree::compute_rollups` folds layout inputs and paint inputs into one `node` hash and one `subtree`
hash (`src/scene/tree/mod.rs:256-375`), and three consumers gate on lists they keep separately:

- the measure cache keys on `rollups.subtree`, so a hover tint misses the cache on every ancestor
  and forces `capture_tree` (`src/layout/pass.rs:276`, `src/layout/engine.rs:221`);
- `cascade_fingerprint` folds `font_epoch`, but `can_update` does not
  (`src/scene/cascade/engine.rs:155-199,288-312`);
- no hash folds a `PaintAnim` (`src/scene/forest.rs:331-348`);
- no hash folds root order, so raising one popup above another damages nothing
  (`src/scene/damage/walk.rs:131-136`).

### Design

**Layout half first, full hash derived from it.** In the rollup loop, compute the node's layout
hash, then the full hash as `H(layout_hash, chrome, shape paint hashes)`. Subtrees fold the same
way. An input added to the layout half reaches the full half by construction. The rule for a new
input: if layout might read it, it goes in the layout half. An extra input costs a cache miss; a
missing one is a stale-layout bug.

- Layout half: `LayoutCore` with every flag (conservative: `disabled` and `focusable` stay in,
  although layout ignores them), bounds, panel, grid and scroll defs, child ids, and each text
  shape's layout hash.
- A text shape's layout hash covers what measure reads: text, font, size, line height, wrap mode
  and the other shaped-text key inputs. `Shapes::add` stores it beside the shape hash, for text
  shapes only. Layout reads no other shape kind (`src/layout/text_runs.rs` is its only shape
  reader); a debug assert in `TextRuns` keeps it so.
- `SubtreeRollups` gains `layout_node` and `layout_subtree` (16 bytes per node).

The measure cache and `matches_forest` key on `layout_subtree`. This is safe because a cache hit
replays only layout-derived data: `MeasureSnapshot` holds rects, tracks and
`ShapedText { measured, key }`, and none of those depends on colour. D16's layout oracle proves it
on every mutation it scripts.

**Animated shapes fold their static animation.** `add_shape_animated` folds the anim's channel,
timing, steps and repeat (not its phase and not the time) into the stored shape hash, through one
new `Shapes::fold_paint_anim(idx, &PaintAnim)`.

**One cascade key.** `CascadeKey`, built once per frame by
`CascadeEngine::key(forest, layout, display, font_epoch)`, replaces the free functions
`cascade_fingerprint` and `layout_hashes`. It has a frame-wide part (surface, scale, font epoch)
and a per-layer part (`static_hash`, `paint_counts`, `rect_hash`, root ids, root order,
placement). The frame skip compares the whole key; `can_update` compares the frame-wide part and
each layer's part. One list of inputs, in one struct.

**Roots are children of their layer.** The damage walk treats a layer's root list as the child list
of a virtual layer parent, and runs the existing child-marker inversion logic over it. A root swap
then damages the overlap of the two roots, exactly as a sibling swap does. Folding a root ordinal
into `parent_key` would also work, but it damages every root after an inserted one.

**Reserve an id at resolve.** `SeenIds::resolve` counts the occurrence and records the resolved id
in a per-pass `pending` set (retained capacity); `record_endpoint` moves it to `curr`. A second
resolve of the same raw id then disambiguates. Trade-off, stated in the doc: a widget that resolves
and never shows still takes its occurrence. That is the same class of id shift that conditional
recording already causes under occurrence counting. The release panic in `record_endpoint` stays.

**Stop the `by_id` reallocation.** `run_full` refills `cascade.by_id` with `clear()` + `extend`,
which keeps the larger capacity. `clone_from` from two alternating tables reallocates whenever their
bucket counts differ.

### Tests

- Hash table with `assert_ne!` and `assert_eq!` rows: colour → `subtree` changes and
  `layout_subtree` does not; text → both; padding → both; add anim → `subtree` changes.
- A hover-tint frame is a measure-cache hit at the root (0 driver dispatches).
- D16 oracles over the mutation script, which includes add/drop anim, `load_font` and a root swap.
- Two auto-id widgets from one call site, both resolved before either shows: distinct ids, no panic.
- `alloc`: after a widget-count spike and drop, `run_full` does not allocate.

## D4. Paint bounds come from the geometry the encoder draws

### Problem

`compute_paint_rect` derives bounds with its own rules, and where they differ from what is drawn,
damage and cull miss pixels: `ImageFit::None` overflow (`src/scene/cascade/paint_rect.rs:342-349`),
the Bézier root lost to cancellation (`src/primitives/bezier/mod.rs:105`), icons above 512 px
(`src/renderer/frontend/composer/session.rs:381-394`), and text scissored at its advance box
(`session.rs:809,859,1058`).

### Design

**One resolver per shape kind.** Move `ImageFit` resolution from
`renderer/frontend/encoder/geometry.rs` to a method beside the record,
`ImageFit::resolve(base, intrinsic) -> FitRect`. The cascade bounds the shape by `FitRect::rect`,
and the encoder draws with `rect` and the UV. Check first that the intrinsic size is available at
cascade time, for an image handle and for an icon's view box. If one is not, stamp it on the record
at lowering, where the source is at hand.

**Stable quadratic roots.** `solve_quadratic` uses the cancellation-free form (Numerical Recipes
§5.6): `q = -½(b + sign(b)·√disc)`, roots `q/a` and `c/q`, keeping the `a ≈ 0` linear arm.

**Icons fill their box.** Above `MAX_RASTER_PX`, `IconDrawRow` carries the destination rect, and
`AtlasSlot::quad` stretches the 512 px raster to it, as the `MAX_RASTER_PX` doc promises. In the
coarse band the drawn quad is the box, so the cull rect and the drawn rect agree.

**Text scissors at ink.** The batch scissor is the union of each run's `inflate_text_damage` bounds
(the ink bounds damage already uses), intersected with the ancestor clip where one exists.

**No row for a no-op chrome.** A chrome row kept only for `ClipMode::Rounded` keeps its clip role
with an empty screen rect (`paint_rect.rs:219-234`).

**NaN is screened at the boundary and asserted inside.** `Shapes::add` already rejects NaN. Inside,
`Rect::clamp_to` and `Rect::intersect` `debug_assert!` that no operand is NaN, instead of answering
`Some(b)` for `Rect::NAN.intersect(b)` (`src/primitives/rect/mod.rs:340,356`). `URect::covering`
clamps to the `u32` range before `ceil_px` (`src/primitives/urect/mod.rs:90`).

**Meshes are screened, not asserted.** `Mesh` keeps a running `max_index` (one `max` per pushed
index), and `append` updates it. `Mesh::is_noop` answers true when `max_index >= vertex_count`, so
a malformed mesh is dropped like every other screened shape, with a `debug_assert!` that catches
the bug in development. This is what the module doc promises, at one compare per mesh
(`src/primitives/mesh/mod.rs:113,165,189`).

**Polyline colours are screened per colour.** `has_nan` reads every per-point and per-segment
colour. `is_noop` drops only when every colour is a no-op and the count matches, so a wrong-length
slice reaches `assert_matches` (`src/shape/polyline.rs:262-280`). `triangle_paint_empty` counts the
rounding radius (`src/shape/triangle.rs:224-241`).

**Debug overlay in screen space** (`encoder/collision_overlay.rs:43-62`).

### Tests

- For every `ImageFit` variant: cascade rect == encoder draw rect.
- `cubic_bbox` against sampling: for every integer quadratic on a grid, the bbox contains 1024
  sampled points. Worked case `7, 49, 8` → `hi.y = 28.25` within 1e-3.
- Icon 300×300 at scale 2 draws 600×600.
- Italic run, no clip: the scissor contains the ink bounds.
- D16 damage oracle rows for an `ImageFit::None` move and a curve move.

## D5. GPU geometry: normalize radii, encode triangles exactly

### Problem

- Radii reach the shader unnormalized (`session.rs:875`), and `sdf_rounded_box_centered` assumes
  `r ≤ b`: `corners(9999)` paints nothing, and `corners(30)` on 100×40 paints 96.6 px wide.
- The drop shadow keeps the source radius while it grows the box by `spread`
  (`src/gpu/quad.wgsl:324-334`).
- Triangle vertices ride f16 lanes: 1 px steps above 1024 px, `inf` above 65504
  (`session.rs:915-921`).

### Design

**CSS radius normalization in one function.** `Corners::fit_to(size) -> Corners` implements CSS
Backgrounds 3 §5.5 "Overlapping curves": `f = min over the four sides of (side length / sum of its
two adjacent radii)`; when `f < 1`, every radius is scaled by `f`. The quad composer, the rounded
clip mask and the shadow path call it once, after the physical scale and before f16 packing. After
it, `r ≤ b` holds, so the f16 lane overflows only when the rect itself is larger than 65504 px; the
`fit_to` doc names that limit.

**Shadow radius follows the CSS spread rule** (CSS Backgrounds 3 §7.1, `box-shadow`). For a drop
shadow with spread `s > 0`, each corner radius `r` becomes `r + s` when `r ≥ s`, and
`r + s·(1 + (r/s − 1)³)` when `r < s`, so a sharp corner (`r = 0`) stays sharp. For `s < 0`, the
radius is `max(r + s, 0)`. The inset hole uses the same rule with `−s`. Apply it in the encoder
(`encoder/layer_ctx.rs:613-628`), then `fit_to` against the shadow box. The WGSL drop arm needs no
change.

**Triangle vertices as rect-relative unorm16.** Keep the lanes and change the encoding. Each vertex
is stored as `(p − rect.min) / rect.size` in unorm16, and the shader decodes with
`unpack2x16unorm`. The six vertex values use the same six 16-bit slots that f16 uses today, so the
instance stride and the pipelines do not change. The covering rect is the vertices' bbox, so every
value is in `[0, 1]`. The error is `size / 65535`: 0.046 px for a 3000 px triangle, and no `inf` at
any size. Fix the `FillAxis` doc's "sub-pixel up to ~2048 px" claim.

**`Spacing` stays f16** (decision D-2). Fix the precision docs instead (ISSUES.md item 1: the error
is up to ±1 px below 4096 and up to ±2 px above), and delete the REVIEW item as a documented
trade-off.

**Pin the curve kind order** with a const assert in `render_buffer/curve.rs` (`curve.wgsl:260`).

### Tests

- `fit_to` table with `f = min(side / sum of its two radii)`: `(100×40, all 9999)` → all 20;
  `(100×40, all 30)` → all 20; `(100×40, tl 30, tr 10, br 0, bl 0)` → `f ≥ 1`, unchanged;
  `(100×40, tl 40, bl 40)` → `f = 40/80`, tl = bl = 20.
- Spread table: `(r 0, s 10)` → 0; `(r 10, s 10)` → 20; `(r 4, s 10)` →
  `4 + 10·(1 + (0.4 − 1)³) = 4 + 10·0.784 = 11.84`; `(r 10, s −6)` → 4.
- A triangle spanning 3000 px: decoded vertices within 0.05 px.
- Visual goldens: pill button, 100×40 `corners(30)`, drop shadow with spread ±6 on a circle and on a
  sharp box.

## D6. Colour: interpolate premultiplied, sample texel centres

### Problem

Mesh vertex colours, curve `color0→color1`, gradient stops, the LUT filter and the polyline join
average interpolate straight alpha and premultiply after (`src/gpu/mesh.wgsl:29`, `curve.wgsl:331`,
`quad.wgsl:302`, `gradient_atlas/bake.rs:128,209`, `session.rs:780`). White to transparent black
reaches 0.25 at the midpoint instead of 0.5. The LUT bakes texel `i` at `t = i/255` but samples
`u = t` (`bake.rs:149`).

### Design

**Premultiplied end to end.** CSS Color 4 §12.3 and CSS Images 4 interpolate gradients
premultiplied, and GPU pipelines interpolate premultiplied colour to avoid this fringe.

- Bake the LUT premultiplied.
- **The fade multiplier becomes a scalar.** Today the shader computes `c * in.fill` with
  `fill = (1, 1, 1, fade)`, which is correct only for a straight-alpha sample. With a premultiplied
  sample, all four channels scale by the fade: `fill = (fade, fade, fade, fade)`. Change
  `BrushSource::gpu_fill` and the `quad.rs` doc in the same step.
- `mesh.wgsl` premultiplies `color * tint` in `vs` and interpolates the result.
- `curve.wgsl` premultiplies `color0` and `color1` before `mix`.
- The composer's join average uses premultiplied channels.
- Document the interpolation space once, in `primitives/brush`.

**Sample texel centres.** One `prelude.wgsl` helper maps `u = (t·(N−1) + 0.5) / N`, with
`N = LUT_ROW_TEXELS`, used by the quad and curve shaders. `t = 0` and `t = 1` land on the first and
last texel centres.

**Render targets must encode.** `OffscreenHost` construction asserts an sRGB or float target format
(a release `assert!` on public-API misuse, on a cold path). A fallible conversion is
API_CHANGES A16.

**Premultiply table at construction**, not on the first soft-edged image
(`src/gpu/image_store.rs:191-224`). Fix the `image.wgsl` header.

### Tests

- Bake: a red α1 → blue α0 hard stop gives texels (1,0,0,1) and (0,0,0,0); the bilinear midpoint is
  (0.5,0,0,0.5).
- Fade 0.5 on a premultiplied sample (0.8,0.4,0,0.8) gives (0.4,0.2,0,0.4).
- `u` mapping: `t = 0` → `0.5/256`, `t = 1` → `255.5/256`.
- Visual: white→transparent mesh fade, alpha fade-out curve, hard-stop gradient, faded gradient.

## D7. Animation: a motion that starts from rest spends nothing on its first frame

### Problem

A retarget resets the segment and advances by the whole `dt` in the same tick
(`src/animation/anim_map_typed.rs:145-163,208-217`). After an idle window, `dt` is the 0.1 s clamp,
so `AnimSpec::FAST` is 99.5 % done on its first painted frame. NaN targets never settle. The spring
floor (`POS_EPS = 0.01`, `spring.rs:23`) is in pixels but applies to colours. The duration snap runs
mid-curve and cuts `OutBack` overshoot.

### Design

**Start from rest at zero.** CSS transitions start at the style change, so the first frame shows the
start value. But a target that moves every frame (an animation that follows a drag) must keep
moving, so the rule is narrower than "a retarget spends nothing":

- A row that was **settled** when its target changed spends 0 on that tick. The `dt` it would have
  spent is time that passed while it was at rest.
- A row **in flight** spends `dt` as now. A duration row restarts its segment from `current`; a
  spring keeps its velocity rule.

Pass A and pass B then agree for the from-rest case, and `already_advanced` keeps guarding the
in-flight case.

**NaN target is a logic error.** `debug_assert!` that the target is finite, through
`target.sub(target).magnitude_squared().is_finite()`, with a message that names the slot.

**Snap only on retarget.** The snap-if-close check runs once, when the target changes. A running
segment ends by its own rule.

**One absolute floor until A5.** Until the per-type tolerance in API_CHANGES A5 lands, the spring
uses the duration floor (`EPS = 1e-4`, below one 8-bit sRGB step near black) for both the retarget
snap and settle. Cost, stated in `spring.rs`: the default spring (stiffness 170, damping 26, near
critical) runs about 0.4 s longer on a pixel travel before it settles. That is repaint cost only;
the motion is the same. A relative floor was the alternative, but it needs an extra absolute floor
for tiny travels and a rule for mid-flight retargets: three rules where one is enough.

**One match on the motion pair** (`anim_map_typed.rs:103-110,208-237`).

### Tests

- `FAST` from rest after a 1 s idle: frame 1 == start; frame 2 at 16 ms == ease(16/120).
- A target that changes every frame for 10 frames: the value moves on every frame.
- Pass A and pass B retarget from rest give the same value.
- `OutBack` with a small delta reaches its overshoot.
- Dark-theme hover `#121212 → #1c1c1c` under `SPRING` animates over more than one frame.
- Replace the `dt = 0.0` retarget workaround in `tests/duration.rs` with real `dt`.

## D8. File data is validated where it is parsed

### Problem

Theme scalars are screened at some use sites (`F32Ext::themed_length`) and not at others, so zero,
negative or NaN values from a theme file reach asserting sinks. `TextStyle` alone validates at
deserialization (`text_style.rs:37,242`). `default_min_size` can collide with a caller's `max_size`
(`src/scene/node/mod.rs:169-180`). `DockState` validates depth against the default cap while it
loads (`dock_state.rs:141,600-612`).

### Design

**Parse, don't validate.** Every theme scalar is validated once, in `Deserialize`; the rest of the
crate treats it as a contract.

- Validators in `src/widgets/theme/serde.rs` (which already holds the duration one): `length`
  (finite, ≥ 0, within the f16 range for f16-stored fields), `positive` (finite, > 0), `fraction`
  (0..=1), `aspect` (finite, > 0). Each is a `deserialize_with` function, so field types stay `f32`
  and no exported item changes.
- Use sites assert their contract (`debug_assert!` in per-frame paths). `themed_length` keeps only
  its floor role (`max(min)`), which is a widget design rule.

**The test enumerates the fields, not a hand list.** Serialize `Theme::default()` to RON, walk every
numeric leaf, and for each one deserialize a copy with that leaf set to `-1`, `0`, `NaN` and `inf`.
Each result must be either `Err`, or a theme that renders one frame of `FrameFixture` without a
panic. A new field with no validator then fails this test the day it is added.

**Defaults yield to authored bounds.** `Node::fill_min_size` clamps the default to an authored
`max_size`, and `fill_max_size` clamps to an authored `min_size`. A default means "when the caller
said nothing", so it never contradicts what the caller said. The `set_*` asserts stay for two
authored values that conflict.

**Load checks structure; the app applies policy.** `DockState` deserialization validates against the
addressable maximum (`DockPath`'s limit), not `DEFAULT_MAX_DEPTH`; `max_depth` applies the app's cap
after load, as its doc says.

### Tests

- The theme mutation walk above.
- `Modal … max_size((240, 400))`, `ContextMenu … max_size((120, 300))`,
  `Tooltip … min_size((300, 0))`: no panic; widths 240, 120, 300.
- Dock: save at `max_depth(6)` with depth 5, load → `Ok`.

## D9. Wheel routing: the target must be able to pan that way

### Problem

`hit_test_targets` picks the topmost `Sense::SCROLL` row (`src/scene/cascade/mod.rs:308-313`). Every
`TextEdit` senses `SCROLL`, and a tab strip band is a horizontal scroll, so a vertical wheel over
either is swallowed and the page does not scroll.

### Design

**Route each axis to the nearest scroller that can pan along it.** This is browser scroll chaining
(CSS Overscroll Behavior §2).

- The cascade stamps each hit row of a `LayoutMode::Scroll` node with `pan_axes`: its declared
  `ScrollAxes` intersected with the axes on which layout's content extent exceeds the viewport (or
  zoom is above 1). A `SCROLL` sense on any other node (a canvas that pans by wheel) gets both axes.
- `hit_test_targets` keeps the topmost row per axis. A wheel delta splits by axis, after D2's Shift
  swap, and each component goes to its axis target.
- TextEdit's y→x mapping moves into routing: a horizontal-only scroller takes a pure-y delta only
  when no row under the pointer can pan y.

Stage 2, not in this plan: chain at the edge (a scroller at its end passes the rest on). It needs the
scroll offset at routing time; check first whether layout holds it for `LayoutMode::Scroll` nodes.

### Tests

- Single-line fields in `Scroll::vertical()`: wheel y over a field scrolls the page.
- Overflowing tab strip: wheel x and Shift+wheel y (Linux) pan the chips; wheel y scrolls the page.
- A lone field with overflowing text: wheel y pans the text.

## D10. Overlays inherit their owner's state

### Problem

The disabled cascade is per layer, so a popup stays live when its trigger is disabled
(`combo_box/mod.rs:185-191`, `color_button/mod.rs:116-122`). Composite widgets probe children before
their own node opens, so `ancestor_disabled` misses the composite's own flag for one frame
(`expander/mod.rs:178`, `color_picker/mod.rs:186`). Tooltip passes its bubble id as the overlay
owner (`tooltip/mod.rs:239`).

### Design

- **Owner state carries to the overlay root.** A side-layer root pushed inside an open scope seeds
  its first frame with the anchor scope's `ancestor_or_self_disabled`. Check first that every
  overlay records inside its owner's scope; where one does not, `OverlayScope` carries the bit.
- **Disabled closes.** ComboBox and ColorButton close their popup when the trigger is disabled, as
  native combo boxes do.
- **Composites forward their flag** to the children they record, as `Scroll` does
  (`scroll/mod.rs:123`): Expander, ColorPicker, ColorButton, ComboBox, TabStrip, DragValue,
  ContextMenu.
- **`OverlayScope::claim` takes the owner from the root** (it is `pub(super)`).

### Tests

- Open ComboBox, disable the trigger: the next frame records no popup, and a click at the old row
  writes nothing.
- Expander disabled on frame N: a click on frame N does not toggle.

## D11. Text uses cosmic's line and segment model

### Problem

- The probe's byte↔cursor maps count `\n` only; cosmic also ends lines at `\r`, `\r\n` and `\n\r`
  (`src/text/probe/mod.rs:425-460`).
- A truncated run probes `prefix + "…"` but maps through the full text, so a hit can return a byte
  inside a char (`src/text/shaper.rs:290`, `probe/mod.rs:293`).
- The wrap-floor scan finds segment starts in visual order (`src/text/cosmic/geometry.rs:117`).
- `load_font` uses the panicking `FontFamily::named` (`src/text/cosmic/mod.rs:390`).
- `EncodedKey` keeps the origin's bin, but extraction bins from the exact origin
  (`cosmic/mod.rs:661,687`).

### Design

- **Line starts come from the shaped buffer.** The probe builds its line-start table from its own
  `Buffer`'s lines: each `BufferLine` contributes `text().len() + ending().as_str().len()`
  (cosmic-text 0.19 `LineEnding`: `Lf`, `CrLf`, `Cr`, `LfCr`, `None`). That is exactly what cosmic
  shaped, so the two maps cannot disagree. The table lives in retained probe scratch.
- **A truncated run maps through its displayed text.** The probe records the cut byte; an index in
  the kept prefix maps 1:1, and any index at or past the ellipsis maps to the cut.
- **Segments scan in logical order.** Per layout line, walk glyph indices sorted by `start` (in a
  retained scratch buffer), then apply the break test. This handles mixed-direction lines, not only
  all-RTL runs.
- **`load_font` returns `Err`** when the family table is full, through `try_named`.
- **The encode key is exact**: snap the origin to its bin before `extract_glyphs`.

### Tests

- Probe table over `"ab\rcd"`, `"ab\r\ncd"`, `"ab\n\rcd"`: every offset round-trips.
- `"ééééé"` truncated to `"é…"`: every hit is a char boundary ≤ the cut.
- RTL `"אב גד"`: `intrinsic_min == 20`; also mixed `"ab אב"`. Needs the Hebrew test face
  (TEST_REVIEW 13).

## D12. Widget behaviour fixes

Local causes, one step each in phase 8.

- **ColorPicker** (`color_picker/mod.rs`): readouts, hex and preview read the bound colour, not
  `coords.to_color()`; R/G/B edits start from the bound colour; `written: Option<RgbaF32>` replaces
  `written` + `seeded`; history pushes on a pointer release or an explicit commit, not on keyboard
  nudges or an unchanged hex `lost_focus`; `History::default` is empty and the presets seed lazily;
  the value grid shows V in the free cell.
- **Hue**: stored in `[0, 1]` closed; `set_hue` clamps, and only arithmetic wraps
  (`color_strip/mod.rs:194,255`, `color_picker/mod.rs:472`).
- **Numeric ranges**: `Slider::range` asserts a finite range (public-API misuse, cold path);
  `store_i64` clamps to `ceil(lo)` and `floor(hi)` (`drag_num/mod.rs:149`).
- **TabbedView**: no `Reordered` for `to == from + 1`; a drop needs the pointer inside the strip;
  `*selected` follows the moved page. Chip identity by page is API_CHANGES A12.
- **TabStrip**: a keyboard or overflow-menu selection pans the band to the chip; `hidden` tests
  against the clip rect (deflated by padding); with no selection, Right selects chip 0.
- **TextEdit**: `max_chars` with no room keeps the selection (`editor.rs:304-315`); `normalize`
  snaps to grapheme boundaries (`edit_state.rs:234-258`); Cmd+Left/Right and Cmd+Backspace go to
  the line edge on macOS; Ctrl+Home/End go to document start and end; Shift+click extends. The key
  drain is D1.
- **Theme derivations**: the picker's value editor derives from the chip (`theme/color_picker.rs:195`);
  `ambient()` reads `Theme::text`.
- **Scroll**: the offset band counts zoomed padding (`state.rs:174-181`; confirm with the zoom-2,
  padding-10 case first); the track paging doc says "on click".
- **Expander**: store the full body height from layout, not the clipped response rect
  (`expander/mod.rs:247-253`).
- **MenuItem::separator** gets `#[track_caller]`.
- **Clipboard**: `ContentNotAvailable` answers `Ok("")`; only a backend error falls back; a failed
  `set_text` retries the primary on the next read.
- **Shared popup trigger**: one crate-internal `PopupTrigger { open }` for ComboBox and ColorButton.
- **ColorField / ColorStrip keys**: one handler; PageUp/PageDown step 0.1 on both.
- **DragValue Escape** reverts the typed text, as the hex field does.
- **WindowCommands::close** dedupes per token.
- **Frame-start snapshot**: take `frame_quiescent` before `App::update` too.
- **Warmup keeps focus requests**: replay `set_focus` / `clear_focus` / `release_input_scope` from the
  warmup pass into the real `InputState`.

## D13. Bound the worst-case frame

- **Occlusion prune** (`composer/occlusion.rs:140-149`): bucket occluders into a tile grid of the
  `text_grid` shape and test only the tiles a quad touches.
- **Higher-kind overlap** (`composer/higher_kind.rs:221-232`): the same grid for curves. Fix the
  module doc's bound.
- **Damage inversions** (`damage/walk.rs:362-378`): union inverted pairs per row before pushing.
- **GPU view `repaint(false)`**: `RenderTargetDraw` carries the view's epoch; the backend
  recomposites the retained target unless the epoch moved.
- **Icon prewarm**: prewarm last frame's requested keys, rescaled, under a fixed per-frame raster
  budget.
- **Single-quad cache** shared by the overlay dim quad and `upload_clear`.
- **Per-frame `assert!`** to `debug_assert!` (`gpu/viewport.rs:38`); lazy `gpu_view` probe.

Each gets a counter or `alloc` test that pins its bound, for example: prune calls for a 100×100 grid
of equal cells stay below `N × tiles_per_quad`.

## D14. Layout drivers agree between measure and arrange

- **WrapStack** (`wrapstack/mod.rs:183`): `would_wrap` compares the quantized extent
  `canonical_px(line.main + gap + child)` with the budget, in both passes. A Hug stack of two
  100.2 px children then stays on one line at its 200 px arranged width.
- **ZStack and Canvas Hug axes** (`layout/pass.rs:122`): measure against `inner_avail`, as `Stack`
  does on its cross axis. Fix the `zstack/mod.rs:30-32` rationale. Tests: wrapping text in a Hug
  `Scroll::vertical()` in a 300-wide column wraps at 300; a Hug ZStack with a Fill child still sizes
  to its content.
- **Hug scroll in a stack's main axis** (decision D-1: no shrink). Keep the WPF stack contract: only
  `Fill` children share the remaining space. Fix the false doc at `axis_placement.rs:140` to say so,
  and document on `Scroll` that a scroll meant to take a stack's remaining space is `Fill`. Delete
  the REVIEW item as resolved by contract.
- **Cache plumbing** (`cache/mod.rs:468`): one rule for every layer (copy, or swap), documented;
  `arrange_src` stamps every node of a hit subtree (`pass.rs:289`).

## D15. Test harness redesign

TEST_REVIEW gives the design for each group. The core that later phases need:

- `UiHarness::frame_passes(record) -> Passes<R>` (`a()`, `b()`, `len()`, `count_where`), and
  `frames_until_input_idle` for D1; `frame_value` and `try_frame_value` become wrappers.
- `UiHarness::state::<S>(id) -> &S`, which panics on a missing row.
- `assert_panics_with(fragment, f)` in a `cfg(test)` subsystem module.
- `UiHarness::point_in(id, local)`, `click_in`, `press_in`, hit-checked.
- `UiHarness::frame_app`; per-button `pressed_at` updated in `on_input`; delete `Ui::inject_input`.
- `alloc`: export the expiry ring length and derive the audit windows from it; a CPU
  `FrontendHarness` for strict-zero renderer fixtures.
- Visual: `Capture { image, paint }`, golden bookkeeping, a cached adapter failure. The `Tolerance`
  shape is public (API_CHANGES A7).

`internals` gating (TEST_REVIEW 8) waits on API_CHANGES A17.

## D16. Differential oracles

Most scene bugs in the review are a retained result that an input change did not invalidate, and
each was found by reading. These tests find that class by construction, and they keep D3 and D4
true as new inputs are added.

- **Mutation script.** A `cfg(test)` scene driver over `FrameFixture` that applies a list of
  mutations, one per frame: colour change, text change, `load_font`, add/drop/change a `PaintAnim`,
  root swap, `ImageFit` change, scroll, resize, theme swap, widget add and remove.
- **Layout oracle.** After each frame, drop the measure cache (`MeasureCache::forget_all`), lay out
  again, and assert equal rects, scroll content and `ShapedText`.
- **Cascade oracle.** After each frame that took the incremental or skip path, run `run_full` into a
  second `Cascade` and assert equal entries, hits and paint rows.
- **Damage oracle (CPU).** For each frame, every paint row whose (id, row, hash, screen) differs from
  the previous frame must have both its old and its new screen rect inside the damage region. This
  is exact and needs no GPU.
- **Damage oracle (pixels).** In the visual suite, for a subset of the script, a partial repaint
  equals a full repaint of the same frame, bit for bit.
- **Input oracle.** D1's batch-versus-one-per-frame comparison.

Keep each CPU oracle test under the 1 s limit by sizing the script. The pixel oracle lives in the
visual suite.

# Decisions

- **D-1. Hug children in an overflowing stack: no shrink.** Applied in D14 as a contract and doc
  fix.
- **D-2. `Spacing` in f32: no.** Applied in D5 as a doc fix.
- **D-3. Keys after Escape or Enter in one batch: trickle.** See D1, "How other frameworks do it":
  native toolkits and browsers dispatch per event, Dear ImGui trickles keys and text into separate
  frames, and egui breaks out of TextEdit's loop. Trickling gives the native result for every
  widget, not only the ones that remember to break. TextEdit keeps its stop as a debug-checked
  guard.
- **D-4. Shift+wheel horizontal on Windows and Linux: yes.** Applied in D2.

---

# Part 2: Implementation plan

Each phase ends with the AGENTS.md chain:

```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --lib --test alloc --features internals,bench,golden,gpu-debug-markers
```

A phase that moves pixels also runs `cargo test --test visual --features internals,golden`. Run it
first on the unchanged tree (rewrite stale goldens with `UPDATE_GOLDEN=1` if that run fails), and
review every diff image before you accept a rewrite. A phase that a user can see ends with
`cargo run --example showcase`.

Order: the harness and the oracles first, because every later test uses them. Then panics, wrong
output, cost, cleanup.

## Phase 2. Host and platform input (D2)

- [ ] Key dumps on Linux and on the macOS test laptop, recorded as the translation test table.

## Phase 5. Layout (D14)

- [ ] Quantized `would_wrap` in both passes.
- [ ] ZStack/Canvas Hug axes against `inner_avail`.
- [ ] Stack main-axis contract doc and the `Scroll` Fill note.
- [ ] Cache snapshot rule; `arrange_src` on every node of a hit subtree.

## Phase 6. Animation (D7)

- [ ] From-rest first frame spends 0; in-flight retargets unchanged.
- [ ] NaN target debug assert; snap on retarget only; one floor (`EPS`) for the spring.
- [ ] Single match on the motion pair.
- [ ] Split the 1.9 s spring test (TEST_REVIEW 13).

## Phase 7. Text (D11)

- [ ] Hebrew and Arabic subset test faces.
- [ ] Probe line table from the shaped buffer; truncated-run mapping.
- [ ] Logical-order wrap-floor scan.
- [ ] Exact encode key.

## Phase 8. Widgets and overlays (D9, D10, D12)

- [ ] D9: `pan_axes` and per-axis targets.
- [ ] D10: owner state, disabled closes, composites forward, `claim` owner.
- [ ] D12, one step per bullet, in the listed order.
- [ ] Key-class claims wait for API_CHANGES A3. Interim, only if you want it: a single-line TextEdit
      drops `Motion` from its filter while focused with no selection, so Tab reaches the app.

## Phase 9. Worst-case cost (D13)

- [ ] Occlusion and higher-kind tile grid; damage per-row union.
- [ ] GPU view epoch; icon prewarm budget; single-quad cache; per-frame asserts; lazy probe.

## Phase 10. Cleanup (D15 rest, D17)

- [ ] REVIEW doc groups, one commit per subsystem.
- [ ] REVIEW style groups: `const fn`, one major type per file, free functions to methods,
      `macro_rules! rebind`, gated items to end-of-file modules, relative `use` paths.
- [ ] REVIEW design groups that stay internal: `LayerCtx` gradient trio, `RectKind` dispatch,
      `TextureLimit` round trip, `RenderTargetDraw.display_scale`, pipeline layouts in `new`, raster
      tenant methods, `DamageEngine::budget_px`, `Forest::scratch`, `Ident::Resolved` arms,
      `InputState` field encapsulation, `OffscreenHost` gated impl, `WinitHostConfig` clone,
      `Index16::to_raw`, `IconId` bound and duplicate-name asserts.
- [ ] Drop `memchr` and `rayon`.
- [ ] ISSUES.md items.
- [ ] TEST_REVIEW groups 2, 6, 7, 9, 11–18 that no earlier phase closed.
