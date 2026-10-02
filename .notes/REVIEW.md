# Crate review

Whoever addresses an item deletes it from this file. A group goes when its last item goes.

Groups run from the most severe to the least: panics on reachable input first, then wrong output, then per-frame cost, then design, docs and style. Items tagged **bug** were traced through the code. Items tagged **bug (plausible)** name the check that would confirm them.

## Theme and preference file data reach asserts
`F32Ext::themed_length` says theme scalars are hand-edited file data that "cannot assert". Most widgets screen them. These do not, and `SpinnerTheme`/`ToggleTheme`/… derive `Deserialize`:
- [ ] `src/display/user_scale.rs:27-29,71-77` **bug**: the doc tells apps to read a persisted preference back through `UserScale::new`, which `assert!`s on a non-finite value. A config file containing `nan` crashes the app. Public API: a fallible constructor needs a go-ahead.

## Several gestures in one input batch collapse into contradictory state
- [ ] `src/input/input_state/mod.rs:893-921` against `src/input/response/button_state.rs:40` **bug**: all before the next frame: drag W past the threshold, release (`Release{W, DragStopped}`), then press W again (fresh press with `drag=None`). `response_for(W)` builds `phase=Down` (the live press wins) and `drag=Stopped` (from the stale release). `ButtonState::new(Down, Stopped)` then fails its debug assert, which allows `Stopped` only with `Up{click:None}`. A panic on reachable input in debug builds, easy to hit when a slow frame batches a quick drag-release-re-press on a slider.
- [ ] `src/input/input_state/mod.rs:866-872` and `capture.rs:94` (`end_press` overwrites `release`) **bug (plausible)**: same-batch gestures lose clicks.
  - press/release/press: the widget sees `Down`, and the first click never reaches `response_for`.
  - press/release/press/release: the second release overwrites the first, so `clicked()` fires once instead of twice. A counter button undercounts on a slow frame.
  - `pointer_actions()` reports both `Clicked` and `Pressed` in the press/release/press case, so the two collations disagree, against its own doc at :265 ("cannot disagree").
- [ ] `src/widgets/text_edit/input_pass.rs:134-160` **bug**: caret placement, word select and select-all run only under `resp_state.left.held()`. The router collapses a press and release in the same batch to `Up{click}` (`src/input/input_state/mod.rs:867-890`), so `held()` is false. Scenario: touchpad tap-to-click, synthetic clicks, or a stalled frame. The field gains focus, but the caret stays where it was, and a double-tap selects no word. The press edge (`Up{click: Some(n)}`) is never consulted.
- [ ] `src/input/input_state/mod.rs:270-271` and `:288-289`: the comments "at most two filled" and "never both" are wrong. Release + re-press + re-latch in one batch fills all three slots.

## Widget identity tracking
- [ ] `src/scene/cascade/engine.rs:249` with `src/scene/seen_ids/mod.rs:261` **bug (per-frame alloc)**: `curr` and `prev` swap every frame, so `by_id.clone_from(&forest.ids.curr)` alternates between two tables. hashbrown's `clone_from` reallocates whenever bucket counts differ. One frame with a widget-count spike grows only one of the two maps, permanently. From then on, every full cascade rebuild frees and allocates `by_id`.

## Paint output wrong or missing
- [ ] `src/renderer/frontend/composer/session.rs:381-394` **bug**: an icon whose physical box is larger than `MAX_RASTER_PX` (512) is drawn at 512 px, centred, rather than filling its box. `IconRasterKey::for_box` clamps the key to 512. `IconDrawRow` carries only `key` and `origin`, and the backend (`AtlasSlot::quad`) draws at the raster's own `size`. Example: a 300×300 logical icon at scale 2 paints 512×512 with a 44 px gap on every side. The `MAX_RASTER_PX` doc says "the largest cached raster is reused and magnifies", but nothing magnifies it. Related (**plausible**): in the coarse band the raster rounds up to 4 px, so the drawn quad can stick out up to ~2 px past the `urect` used for cull, overlap tracking and damage.
- [ ] `src/renderer/frontend/composer/session.rs:915-921` **bug**: a triangle's points are packed into the f16 `corners` / `fill_axis` lanes, measured from the covering rect's min. f16 spacing is 1 px in [1024,2048), 2 px in [2048,4096), and anything past 65504 becomes `inf`. A triangle that spans 3000 physical px gets its vertices moved by up to 1 px, so its edges no longer meet neighbouring f32 quad edges. A zoomed-canvas triangle with a vertex more than 65504 px from its rect's min gets an `inf` lane, and the SDF becomes NaN. The `FillAxis` doc promises "sub-pixel up to ~2048 px", which already fails between 1024 and 2048 px.
- [ ] `src/renderer/frontend/composer/session.rs:875` **bug**: corner radii go to the GPU as `corners.scaled_by(scale_phys)` with no clamp to half the rect's extent. `sdf_rounded_box_centered` (`quad.wgsl:138`) assumes `r ≤ b`. On a 100×40 rect: `corners(30)` at p=(49,0) gives q=(29,10), d=+0.68, so the painted width shrinks to about 96.6 px. `corners(9999)` (the "pill" idiom) gives d≈+4092 at the centre, so nothing paints. A radius × zoom above 65504 overflows the f16 lane to `inf`, giving NaN. CSS rescales radii whose sum exceeds a side. Rounded clip masks use the same SDF and share the defect.
- [ ] `src/renderer/gradient_atlas/bake.rs:149` **bug**: texel `i` is baked at `t = i/255`, but both shaders sample `u = t` directly (`quad.wgsl:239`, `curve.wgsl:424`). With linear filtering, texel centres sit at `(i+0.5)/256`, so the colour shown at parameter `t` is `C(t + (t−0.5)/255)`. That is up to half a texel of skew toward the ends. A hard stop at 0.1 on a 1000 px linear gradient lands at 101.6 px.
- [ ] `src/shape/polyline.rs:262-280` **bug**: a NaN channel in a per-point or per-segment colour passes both screens. `is_noop` drops the shape only when all colours are no-ops, and `has_nan` reads only `stroke` and `bbox`, so the encoder's `debug_assert!(!shape.has_nan())` also passes. `per_point(&[RED, RgbaF32::new(NAN,0,0,1), BLUE])` is staged as NaN f16 (`stage_polyline`). The composer writes NaN into `color0/color1` and the averaged join colour, so the curve shader outputs hardware-dependent garbage.
- [ ] `src/shape/polyline.rs:262-272` **bug (minor)**: `per_point(&[])`, or a wrong-length slice whose colours are all transparent, is dropped by `is_noop` before lowering reaches `assert_matches`. A cardinality error that should panic becomes a silently missing stroke.
- [ ] `src/shape/triangle.rs:224-241` **bug (plausible)**: `triangle_paint_empty` ignores `radius`. A thin triangle (0,0),(100,0),(50,0.004) with `radius(3)` has normalized area 4e-5 and is dropped, yet its rounded SDF (`sdf_triangle − r`) would paint a 6 px-thick bar. Confirm what `sdf_triangle` returns once the winding sign `s` reaches 0.
- [ ] `src/renderer/frontend/encoder/collision_overlay.rs:43-62` **bug (debug-only)**: the magenta outline uses `layout[layer].rect`, which is pre-transform layout space, and is emitted with an empty transform stack. A duplicate id inside a panned or zoomed subtree is outlined at the wrong place.

## Platform key events the translation layer gets wrong
- [ ] `src/host/winit/input/mod.rs:88` **bug**: the `KeyboardInput` arm drops `is_synthetic` (`{ event, .. }`). On X11 and Windows, winit 0.30 sends a synthetic Pressed event for every key still held when the window gains focus. Example: the user confirms a dialog in another app with Enter, or dismisses it with Escape, and focus comes back while the key is still down. That key arrives as a real `KeyDown` and reaches the focused TextEdit or an `escape_pressed()` overlay, so it submits a form or closes a modal.
- [ ] `src/input/key_class.rs:87` with `src/host/winit/input/mod.rs:244` **bug (macOS)**: `normalize_modifiers` maps Option to `alt`, and `Modifiers::any_command()` counts `alt` as a command modifier. So any character typed with Option is never classified `KeyClass::Text`:
  - German Mac `@`=⌥L, `{`=⌥8, `[`=⌥5, `|`=⌥7 classify as `Accel`.
  - US ⌥A=`å` classifies as `Edit`, through the `layout_retry` of physical `a`.
  - `TEXT_FIELD` excludes `Accel`, and TextEdit's `apply_key` (`widgets/text_edit/input_pass.rs:254`) also requires `!any_command()`, so none of these characters can be typed into a TextEdit on macOS.
  - Also ™ (⌥2) on US layouts. Windows AltGr arrives as Ctrl+Alt and hits the same gate (**plausible**: confirm with a winit AltGr `KeyboardInput` dump).
  - **plausible**, opposite direction on Linux/Windows: `Modifiers` has no super bit, so Super+L arrives as bare `l` with `text="l"` and a focused field types it. Confirm what winit puts in `text` under Super on X11/Wayland.

## Hug WrapStack: arrange breaks lines against a rounded-down budget
- [ ] `src/layout/wrapstack/mod.rs:183` **bug**: arrange takes its line budget from the arranged `inner.size`, but measure took it from `inner_avail` (`:140`). For a Hug wrap stack the arranged width is `max_line_main`, and `canonical_px` rounds half away from zero (`src/primitives/num/mod.rs:258`). So when the widest line's extent has a fractional part in (0, 0.5), the budget lands below that line. Example: Hug `wrap_hstack` with two children 100.2 wide, gap 0. Measure budget is `round(INF)`, giving one line, desired w = 200.4. Arrange budget is `round(200.4) = 200`, and 100.2 + 100.2 > 200, so the second child wraps onto a second line below the measured height. It overflows the rect and overlaps whatever follows. Text widths are fractional, so this is the common case. The test `a_subpixel_resize_keeps_the_break_its_cache_key_stands_for` covers only the measure side.

## ZStack/Canvas measure Hug axes at INFINITY, ignoring a finite `inner_avail`
- [ ] `src/layout/pass.rs:122` **bug**: `measure_per_axis_hug` offers `INF` on every Hug axis, though `AxisSlot::resolve_node` already computed a finite `inner_avail` there. A Hug ZStack resolves to `min(content, available)` (`src/layout/axis_slot.rs:92`), but its child keeps the INF-measured desired at arrange.
  - Case: `Scroll::vertical()` with default (Hug) width in a 300-wide column, holding a wrapping `Text`. The outer wrapper is a Hug `Widget::zstack()` (`src/widgets/scroll/mod.rs:121`) around a FILL viewport (`:140`). The viewport is measured at w = INF, the inner `Stack::measure` gets `cross_avail = INF`, and the text shapes as one 2000 px line.
  - At arrange the viewport is clipped to 300 (`clip_scroll_to_slot`), but the text keeps desired 2000. Result: a vertical scroll that shows one horizontally clipped line instead of wrapped text.
  - The same text under a Hug Stack wraps at 300, because `Stack::measure` passes the finite `cross_avail` (`src/layout/stack/mod.rs:122`). Canvas's Hug axes share the path.
- [ ] `src/layout/zstack/mod.rs:30-32`: the stated rationale is wrong twice. Stack does not use this pattern on its cross axis (it passes `cross_avail`). And the "ZStack hugs its own Fill child" loop cannot happen, because `AxisSlot::resolve` makes Fill report content at measure, not `available`.

## Paint bounds smaller than the painted pixels (damage and cull miss them)
- [ ] `src/scene/cascade/paint_rect.rs:342-349` **bug**: the `Image`/`Icon` arm bounds the shape by `local_rect.unwrap_or(owner_local)`. `ImageFit::None` (and `IconFit::None`, which maps to it in `shape/icon.rs:39-45`) paints at intrinsic size, centred and uncropped (`encoder/geometry.rs:109-117`, "an image larger than the rect overflows it"). Example: a 100×100 node shows a 300×300 texture with `fit(ImageFit::None)` and no clipping ancestor. Its `Paint.screen` and `subtree_paint_rects` cover only the 100×100 base. Moving or removing it damages only the base, so a 100 px ring of stale pixels stays behind. A neighbour's damage rect that crosses the overflow but not the base makes the encoder cull the image, so the overflow is cleared and never repainted.
- [ ] `src/primitives/bezier/mod.rs:105` **bug**: `solve_quadratic` uses the textbook `(-b ± √disc)/2a`. When `a` is tiny but still above `NEGLIGIBLE_COEFF` (1e-12), the small root is lost to cancellation. A promoted quadratic always hits this case, because `quadratic_to_cubic`'s `2/3` rounding leaves `a = -v0+3v1-3v2+v3` at about 1e-5 instead of 0. Worked case on the y axis: quadratic `p0=7, c=49, p2=8` gives `c1=35, c2=35.333334` and `a=-7.63e-6`. The computed root falls outside `(0,1)`, so `cubic_bbox` reports `hi.y = 22.81`, while the true peak at t≈0.506 is 28.25. The curve overshoots its bbox by 5.4 px, so culling and damage miss it. In an f32 simulation, 9.2% of random integer quadratics in 0..800 miss by more than 1 px, and the worst case missed by 1950 px.
- [ ] `src/scene/cascade/paint_rect.rs:219-234` with `src/scene/tree/mod.rs:490-496`: a no-op chrome that keeps a row only for `ClipMode::Rounded` pushes a paint row whose screen is the full `visible_rect`. A transparent rounded-clip container damages its whole rect on add, remove, or any `cascade_input` change, though it paints nothing. A chromeless `Rect`-clip container emits no row. Over-damage only.

## Paint-row inputs that no hash or gate covers (retained cascade and damage go stale)
- [ ] `src/scene/forest.rs:331-348` and `src/scene/tree/mod.rs:256-375` **bug**: a `PaintAnim` is folded into no shape, node or subtree hash, and not into `cascade_fingerprint` (`cascade/engine.rs:288-312`). `spun_if_animated` (`paint_rect.rs:99-105`) still changes the row's screen.
  - Example: through public `Ui::add_shape_animated`, a caller switches a stroke from `add_shape` to `add_shape_animated(.., PaintAnim::turn(..))` on an otherwise identical shape. Frame N+1 has the same fingerprint, so the cascade is skipped (or the incremental repair skips the subtree), and the row keeps the unspun bbox. `extend_predamaged` scissors only that bbox every frame, so the rotating line is clipped to its horizontal box. The encoder cull uses the same stale rect.
  - Reverse case: dropping the anim, or changing its channel or range without crossing a wake, raises no damage. The shape stays frozen at the last sampled alpha or angle.
- [ ] `src/scene/cascade/engine.rs:155-199` **bug (plausible)**: `can_update` gates on `static_hash`, `paint_counts` and the layer `rect_hash`. `compute_paint_rect`'s text arm (`paint_rect.rs:256-277`) also reads `layout.text_spans`' shaped `measured`. Only `cascade_fingerprint` folds `font_epoch`.
  - Example: after `Ui::load_font`, a fixed-width button's label re-measures wider while every rect stays put. The incremental walk skips the button and keeps the old text row and `subtree_paint_rects`.
  - The epoch frame repaints in full, but later partial frames cull or damage against the stale, narrower extent: glyphs outside it are cleared and not redrawn.
  - Root cause: two cascade-skip gates with different input lists.

## Damage ignores paint order across roots
- [ ] `src/scene/damage/walk.rs:131-136,167-171` **bug**: every root's `parent_key` is the layer constant, and root order reaches no node hash, since child markers exist only under a parent. Example: two overlapping popups recorded A then B in one frame and B then A in the next (raise-to-front), with identical rects. Both classify as `SubtreeUnchanged`, nothing is damaged, and the overlap keeps the old stacking.

## Drop shadows ignore spread when sizing corner radii
- [ ] `src/gpu/quad.wgsl:324-334` **bug**: the drop-shadow path grows or shrinks the box by `spread`, but it still uses the source `in.radius`. The inset path adjusts its radius (line 359, `max(radius - spread, 0)`). The CSS rule is radius + spread, floored at 0.
  - Negative spread: a 20×20 circle (r=10) with spread −6 gives `shadow_half=4` with r=10 > half-extent. Then `q=|p|+6`, and the coverage boundary sits at x≈2 on the axes and ≈1.5 on the diagonal. The result is a tiny squircle instead of a circle of radius 4.
  - Positive spread: r=4 with spread=10 draws 4 px corners instead of 14, so the shadow is boxier than its source. `encoder/layer_ctx.rs:613-628` passes `corners` through unchanged, so nothing upstream compensates.

## Colour is interpolated on straight alpha, then premultiplied
- [ ] `src/gpu/mesh.wgsl:29` (fs at :39) **bug**: per-vertex `color*tint` is interpolated straight and premultiplied only in `fs`. An edge from opaque white (1,1,1,1) to transparent black (0,0,0,0) reaches the midpoint as (.5,.5,.5,.5), which premultiplies to .25 grey at α .5. Premultiplied interpolation gives .5, so fades get a dark fringe. With a transparent vertex of another hue, the fringe is that hue.
- [ ] `src/gpu/curve_pipeline/curve.wgsl:331` (fs :429-430) **bug**: same defect for `mix(color0, color1, t)` on a stroke whose end alphas differ. An alpha fade-out stroke darkens toward the end, or picks up `color1`'s rgb.
- [ ] `src/gpu/quad.wgsl:302-303`, `curve.wgsl:423-424` (LUT is straight alpha per `gpu_gradient_atlas.rs:166-172`) **bug**: the linear sampler filters adjacent straight-alpha LUT texels. At a hard stop from red α1 to blue α0, the bilinear midpoint is (.5,0,.5,.5), a purple band one LUT texel wide (≈4 px on a 1024 px quad). A premultiplied LUT gives red at α .5. The CPU bake (`gradient_atlas/bake.rs:128`) also lerps straight, so the whole gradient chain shares this.
- [ ] `src/renderer/gradient_atlas/bake.rs:209,242-260`, `src/renderer/frontend/composer/session.rs:780`: the stop lerp and the polyline join-colour average are also straight alpha. Opaque red → `TRANSPARENT` passes through (0.5,0,0,0.5). CSS Images 4 interpolates premultiplied. Whichever space is chosen, it is undocumented today.

## A retarget frame spends time that passed before the target changed
- [ ] `src/animation/anim_map_typed.rs:145-163,208-217` **bug**: when the target changes, the row resets (`elapsed = 0` for a duration, or keeps its velocity for a spring) and then, in the same tick, advances by the full `dt`. That `dt` is the time since the previous frame, clamped to `MAX_ANIM_DT` = 0.1 in `FrameRuntime::advance_clock`. The usual case is a window idle for ≥100 ms, then a hover or press, so the first frame of the change gets dt = 0.1.
  - `AnimSpec::FAST` (0.12 s, OutCubic): progress 0.833, eased 0.995. 99.5% of the fade is gone before the first painted frame, so it reads as a snap.
  - `MEDIUM` lands at 87.5%.
  - `SPRING` (170/26): x(0.1) = e^-1.3·(cos 0.1 + 13·sin 0.1) ≈ 0.625, so 37.5% of the travel is skipped. This is the default `Switch` knob and track.
  - Tests hide it by passing `dt = 0.0` on every retarget tick (`tests/duration.rs`).
  - The behaviour is also inconsistent: a retarget first seen in pass B does not advance (`already_advanced`, line 201), but the same retarget in pass A spends the whole `dt`.

## ColorPicker readout and edits read the clamped Okhsv coords, not the bound colour
- [ ] `src/widgets/color_picker/mod.rs:406,309` **bug**: the hex text, the R/G/B values and the preview chip are built from `state.coords.to_color()`. Seeding from `#0000ff` in Okhsv (the default model) clamps to s=v=1, which is `#0037ff`. The hex field reads `#0037FF`, G reads 55 and the preview shows `#0037ff`, while the bound colour is still `#0000ff`.
- [ ] `src/widgets/color_picker/mod.rs:482-489` **bug**: R/G/B edits rebuild `exact` from the same coords-derived `rgb[]`. Nudging R by 1 on `#0000ff` writes `(1, 55, 255)`: G jumps 0→55 although the user never touched it. The type doc promises this "quietly shift #0000ff to #0037ff" never happens.

## Hue wraps at exactly 1.0, so the clamped right end reads as the left
- [ ] `src/widgets/color_strip/mod.rs:194,255` **bug**: `press_fraction` clamps to 0..=1, and `ColorCoords::set_hue` does `rem_euclid(1.0)`. Dragging the hue bar to (or past) its right edge stores hue 0.0, and the marker (`:142`, `read() * size.w`) jumps to the left edge. `End` sets 1.0, which is 0.0, so End behaves exactly like Home. At hue 0, both report `changed == false`.
- [ ] `src/widgets/color_picker/mod.rs:472` **bug**: same root cause. Setting H to 360 in the value cell stores hue 0, and the cell reads back 0.

## Numeric widgets mishandle range ends
- [ ] `src/widgets/slider/mod.rs:220-222` **bug**: `fraction_to_value(f, 0.0, INFINITY)` is `f*inf`, which is +inf for any f > 0. `store_f64` clamps that to `hi = inf`, so any click right of the left edge stores `+inf` into the bound `f64`. For `-inf..=inf`, `0.5*inf - inf` is NaN, and `Limits::clamp` maps NaN to `lo = -inf`. `value_to_fraction` claims to handle unbounded ranges, but the write path does not, and the knob is then drawn at 0 while the value is ±inf.
- [ ] `src/widgets/drag_num/mod.rs:149` **bug**: `store_i64` clamps to `lo as i64` / `hi as i64`, which truncate toward zero. `DragValue::new(&mut i).range(0.5..=10.0)` scrubbed down stores 0 (< 0.5). `range(-10.0..=-0.5)` lets 0 through (> -0.5). Typed entry via `parse_from` behaves the same.

## Fields and strips that cannot scroll still take the wheel
- [ ] `src/widgets/text_edit/mod.rs:140-141` **bug**: every TextEdit senses `SCROLL` unconditionally. The scroll target is the topmost `SCROLL` row with no chaining (`src/scene/cascade/mod.rs:308-313`). Scenario: a form of single-line fields inside `Scroll::vertical()`. With the pointer over any field, the wheel goes to the field, which turns it into a horizontal pan (`view_state.rs:105-109`). That pan is a no-op when the text fits, and the page does not scroll.
- [ ] `src/widgets/tabs/tab_strip.rs:243-246` **bug**: same root cause. The chip band is `Scroll::horizontal()`, so a vertical wheel over any tab strip is swallowed (`pan_y=false` discards it). The page around a TabbedView or dock pane does not scroll from over the strip, and mouse-only users cannot pan overflowing chips. TextEdit maps y onto x for this case; the band does not.

## Scopes claim key classes their owners never act on
- [ ] `src/widgets/text_edit/mod.rs:389` with `src/input/key_class.rs:91-100` **bug**: a focused field declares the whole `MOTION` class, which includes `Tab`, `PageUp`/`PageDown` and every modified arrow. `apply_key` ignores Tab and PgUp/PgDn, and Up/Down in single-line mode. Those keys are granted to the field and dropped. An app polling `ui.key_pressed(Shortcut::key(Key::Tab))` for its own focus traversal never sees Tab while a field is focused.
- [ ] `src/widgets/tabs/tab_strip.rs:145` **bug**: same cause. A focused strip claims `MOTION` but handles only Left/Right/Home/End and Ctrl(+Shift)+Tab, so plain Tab, Up/Down and PgUp/PgDn are swallowed.

## Theme looks freeze `TextStyle::default()` instead of inheriting `Theme::text`
- [ ] `src/widgets/theme/text_edit.rs:153`, `tabs.rs:149-150`, `button.rs:65`, `toggle.rs:158`, `expander.rs:109`, `context_menu/menu_item.rs:85` **bug**: `WidgetLook.text` is all-or-nothing. To change only the colour, these recipes bake a full 16 px SANS style. Any app that sets `theme.text` (13 px, mono…) gets mixed results:
  - A disabled TextEdit/Button switches to 16 px, and the single-line field's height changes (`paint_input.rs:51-53`).
  - Unselected tab chips render at 16 px while the selected chip uses 13 px.
  - Pressing an unselected chip flips it to 13 px while held, because `inactive.active` is `None`.
  - The close glyph resizes on hover.

## A press can leave its capture without a release edge
- [ ] `src/input/input_state/mod.rs:720-727` with `src/ui/frame_cycle.rs:154` **bug**: eviction calls `abandon_press()` after `drain_per_frame_queues()` and raises no input signal, so nothing schedules the frame that should deliver the `DragStopped` edge. If the next frame is `PaintOnly` (an ANIM-only wake such as a caret blink elsewhere), its `drain_per_frame_queues()` wipes the edge unseen. That reopens the case the comment at :712-719 says this path closes: a Slider that skips one frame mid-drag loses its commit.
- [ ] `src/input/capture.rs:76` and `src/input/input_state/mod.rs:491` **bug (plausible)**: `begin_press` overwrites a live `press`, and a missed press runs `cap.press = None`. Both drop a press without writing a `Release`, against `end_press`'s stated invariant ("the only way a press leaves a capture"). If a release is lost (the platform swallows the up event without `Focused(false)`), the next press silently kills an `Active` drag and `drag.stopped()` never fires.

## Multi-click run survives intervening presses
- [ ] `src/input/input_state/mod.rs:491` / `capture.rs:61-76` **bug**: a press that misses every widget, or a press of another button, leaves `run` untouched. Click A at t=0, click empty surface at t=150 ms, click A again at the same spot at t=300 ms: the third press gets `count=2`, so `double_clicked()` fires despite the intervening click. Native platforms reset the run on any intervening down.

## Input uses clock and scale state that differ from what the layout used
- [ ] `src/host/winit/window.rs:207` with `mod.rs:393` **bug**: events are converted with `effective_scale()`, which reads the live app-global user scale. `set_user_scale` takes effect on layout only at the next frame. A click queued between the write and the relayout (Ctrl+= then click) is divided by the new scale and hit-tested against a cascade laid out at the old one, so it lands on the wrong widget. The same holds for `ScaleFactorChanged`. The laid-out `ui.display().scale_factor()` is the correct divisor, and the doc on `effective_scale` states the reverse rationale.
- [ ] `src/host/winit/window.rs:245-252` with `:174-178` **bug**: `clock.skip(hidden)` rewinds the one clock that also stamps input (`Window::on_input`). Press A, hide the window for 5 s, show it, press A again within 500 ms of clock time: `count=2`, a double-click across a 5 s gap. Input stamped while occluded is also later than post-skip `now`, and `saturating_sub` turns that into 0.
- [ ] `src/host/winit/mod.rs:426-446` with `gpu/surface_manager` `clamp_extent` **bug (plausible, Windows)**: Windows reports minimize as `Resized(0,0)`, and winit does not support `Occluded` there. The size is clamped to 1×1, so the minimized window keeps laying out the whole UI at about 0.67 logical px and painting, animations included. Layout-sensitive widget state (scroll-to-caret) can be mutated by the 1×1 layout.
- [ ] `src/host/winit/window.rs:394-405` **bug (plausible)**: a `Suboptimal` texture is dropped unpresented and the swapchain is reconfigured with the same config. If suboptimal persists, every frame does a full CPU frame plus reconfigure and presents nothing.

## TextProbe byte↔cursor mapping disagrees with what cosmic shaped
- [ ] `src/text/probe/mod.rs:437,450` **bug**: `cursor_from_byte` and `cursor_to_byte` count only `\n`, but cosmic's `LineIter` also splits on lone `\r` and on `\n\r` (and treats `\r\n` as one ending).
  - For `"ab\rcd"`: `caret_at(4)` maps to `Cursor{line 0, index 4}`, so the caret lands at the end of "ab" instead of after "c". A hit on line 1 index 1 returns `text.len()=5` instead of 4.
  - For `"ab\n\rcd"`: hit `Cursor{1,1}` maps to byte 4 instead of 5.
  - Reachable: multi-line `TextEdit` paste keeps `\r` (only the single-line mode sanitizes).
- [ ] `src/text/shaper.rs:290` with `src/text/probe/mod.rs:293` **bug**: for a `Truncate`/`Ellipsis` run with a width it does not fit, `layout` resolves the bounded key, so the probe's buffer holds `prefix + "…"`. Every answer is still mapped through the full `run.text`. With `text="ééééé"` (10 bytes) truncated to `"é…"`, a click right of the marker gives cosmic index 5, and `cursor_to_byte` returns 5. That is inside the third `é` (bytes 4..6), so a caller's `text[..5]` panics. `caret_at` for any offset past the cut lands inside the "…" glyph. Reachable through the public `Ui::probe_text`.

## Clipboard folds "no text" into "unavailable"
- [ ] `src/common/clipboard.rs:44,114` **bug**: `map_err(|_| ClipboardUnavailable)` treats arboard's `ContentNotAvailable` (empty clipboard, or non-text content) as an unavailable backend.
  - The documented contract "an empty clipboard answers `Ok("")`" (line 174) is false with the system backend.
  - With `fallback_current` set (after any in-app copy or any successful paste), the error arm returns the stale in-process text. Scenario: paste "foo" from a browser, copy an image in another app, paste into a `TextEdit`: "foo" is inserted.
- [ ] `src/common/clipboard.rs:101-103,130-132` **bug (plausible)**: one failed primary `set_text` makes `Authority::Fallback` sticky. Every later `text()` skips the system clipboard until the next successful in-app copy, so text copied in other apps meanwhile is invisible. A transient arboard/X11 write failure would confirm it.

## TextEdit editing edge cases
- [ ] `src/widgets/text_edit/editor.rs:304-315` **bug**: when `max_chars` leaves no room (`max_chars(0)`, or host content already over the cap), typing or pasting over a selection still runs `replace_range(sel, "")`. The rejected insertion deletes the selection, though the doc says over-cap input "is dropped".
- [ ] `src/widgets/text_edit/input_pass.rs:184-201` **bug**: the drain continues after `Blur` or single-line `submitted`. A batch `[Escape, 'a']` reports `cancelled` with 'a' inserted. A batch `[Enter, 'x']` reports `submitted`, but the caller reads the buffer after `show` and gets `value + "x"`.
- [ ] `src/widgets/text_edit/edit_state.rs:234-258` (doc at `editor.rs:136-137`) **bug**: `normalize` repairs offsets only to char boundaries, although `Editor::normalize` claims grapheme boundaries. The host sets "e\u{301}" with the caret at 1: the caret stays mid-cluster, and typing 'x' gives "ex\u{301}", which moves the accent onto x.
- [ ] `src/widgets/text_edit/input_pass.rs:213-235` **bug**: platform conventions are missing. On macOS, Cmd+Left/Right moves one grapheme instead of to the line edge, and Cmd+Backspace deletes one grapheme. Ctrl+Home/End in multiline goes to the visual line, not the document start/end. Shift+click re-anchors instead of extending (`press` always calls `arm_drag`, `editor.rs:149-162`).

## TabbedView reorder and identity
- [ ] `src/widgets/tabs/tabbed_view.rs:304-310` **bug**: the no-op guard catches only `to == from`. Releasing a chip just right of its own centre gives `insertion_slot == from + 1`, which under the documented "pre-move index" contract is also a no-op. It is still reported as `Reordered{from, to: from+1}`.
- [ ] `src/widgets/tabs/tabbed_view.rs:342-347` **bug**: `dropped_slot` uses the pointer x wherever the release happens: deep in the page body, or over another widget. Any drag-release reorders by x. DockState drops only when the pointer is inside a pane rect (`dock_state.rs:797-800`).
- [ ] `src/widgets/tabs/tabbed_view.rs:304-310` **bug**: the view owns `*selected` (it writes it for `Activated`) but leaves it alone on `Reordered`. Pages [A,B,C], selected 0, drag A to the end: the caller moves A as told, `selected` stays 0, and the view now shows B.
- [ ] `src/widgets/tabs/tabbed_view.rs:276`: chips are keyed `i as u64`, which TabItem's doc (`tab_item.rs:8-12`) says hands one chip's state to another. After Closed/Reordered, the look animation and hover state of slot i transfer to whatever page slid in.

## TabStrip overflow and keyboard travel
- [ ] `src/widgets/tabs/tab_strip.rs:584-586,637-641` **bug**: neither an overflow-menu pick nor an arrow/Ctrl+Tab move scrolls the band. The newly selected chip can stay fully out of sight, which defeats `TabOverflow::Menu`. The band is never driven with `pan_by`.
- [ ] `src/widgets/tabs/tab_strip.rs:534-543` **bug (plausible)**: `hidden` compares chips against the band's rect. Scroll content is clipped to that rect deflated by the band's padding (6 px by default), per the comments at `scroll/mod.rs:487-489` and `text_edit/mod.rs:423-427`. A chip cut up to 6 px under the padding reads as visible, and the chevron does not appear.
- [ ] `src/widgets/tabs/tab_strip.rs:607` **bug**: with no selection, `here = 0`, so ArrowRight activates chip 1 and chip 0 is skipped.

## A disabled trigger does not disable what it opened
- [ ] `src/widgets/combo_box/mod.rs:185-191`, `color_button/mod.rs:116-122` **bug**: the open flag toggles only on `clicked()`, which is empty while disabled, and the popup is recorded whenever `open`. If the trigger becomes disabled while its list or picker is open, the popup stays open and live. Picking a row still writes `*selected`, and the picker still writes the colour. The popup sits in another layer, so the per-layer disabled cascade never reaches it.
- [ ] `src/widgets/expander/mod.rs:178`, `color_picker/mod.rs:186` **bug**: the caller's `.disabled(true)` lands on the outer vstack. The interactive children read it only through the cascade, one frame late. On the first disabled frame a click or Space still toggles the Expander, and drags still write the colour. `Widget::response`'s own-flag fold exists to prevent this lag, but neither widget folds `authored_disabled()` into its children.

## Hug scroll overflows a stack's main axis
- [ ] `src/layout/axis_placement.rs:140` **bug (plausible)**: the doc says a stack's main axis needs no scroll clip because "its flex solver shrinks against the zero min-content a panned scroll reports". That holds only for a `Fill` scroll. A Hug scroll is a non-Fill child: `Stack` never shrinks it, and it is measured against the full `main_avail`, not what its siblings leave. Example: VStack 400 tall with a 50 px Hug header and `Scroll::vertical()` (Hug, content 1000). The scroll's desired is `min(1000, 400) = 400`, it is arranged at y 50..450, and its last 50 px (bottom bar included) fall outside the stack.

## Text clipped by its own batch scissor
- [ ] `src/renderer/frontend/composer/session.rs:809,859,1058` **bug (plausible)**: the text batch's GPU scissor is the union of the runs' measured-box `bounds`, even when no ancestor clips. Glyph ink outside the advance box is cut: italic overhang, a negative left side bearing, and the AA fringe lost when snapping rounds a 100.4 px box down to 100. It also depends on the batch: a run alone is cut at its own box, but batched with a wider neighbour its overhang shows, even past its own ancestor clip when the run is non-strict. Confirm with italic text whose last glyph overhangs, with no clip.

## Render target colour encoding is not enforced
- [ ] `src/gpu/render_target.rs:48-56` (`From<&wgpu::Texture>`) **bug**: any format is accepted. The pipeline writes linear light and relies on an sRGB (or float) target to encode it. An `OffscreenHost` given an `Rgba8Unorm`/`Bgra8Unorm` texture renders everything too dark, with no error: sRGB 0x80 grey → linear .216 → stored as 0x37. The contract is neither documented nor asserted (`surface_manager` enforces sRGB for windows only).

## RTL wrap-floor scan reads segments in visual order
- [ ] `src/text/cosmic/geometry.rs:117` **bug**: segment boundaries are detected by `g.start ∈ breaks`, i.e. at a segment's logical first glyph. In an RTL run that glyph is visited last, so each reset happens one glyph late. Two neighbouring words merge into one segment, with the space between them counted inside it. Example: `"אב גד"` with letters 10 px and the space 5 px. Visual order is ד(7) ג(5) ' '(4) ב(2) א(0), with the break at 5. The scan yields segments {ד}=10 and {ג+' '+ב+א}=35, so `intrinsic_min=35` instead of 20. `WrapWithOverflow` min-content and target width are inflated for RTL text. No RTL case exists in the wrap-floor tests.

## Non-finite targets: a row never settles, and a spring row stays broken
- [ ] `src/animation/anim_map_typed.rs:125,145,156` **bug**: `Ui::animate` and `tick` accept a NaN target without any check.
  - `row.target != target` is true every frame (NaN ≠ NaN), so the row retargets every frame and never settles. For a duration spec, `elapsed` resets to 0 each frame, so `progress >= 1` is never reached. Result: a repaint every frame, forever.
  - For a spring, one NaN frame is enough (f32 spring, targets 0 → NaN → 1.0 → 1.0 …): velocity becomes NaN, and `dot(NaN, …) < 0` is false, so the NaN velocity is kept. `current` stays NaN and `settled` stays false for as long as the slot is touched, even after the target is finite again.
  - A caller logic error should crash: a `debug_assert!` on finiteness rather than a silent full-rate repaint loop.

## Spring settle floor is in pixels but is applied to colours and mixed compounds
- [ ] `src/animation/spring.rs:315-318` with `anim_map_typed.rs:179` **bug**: `POS_EPS` = 0.01 is justified as pixel-scale ("the last 0.01 px"). The spring path applies it to `RgbaF32` and to `AnimatedLook`/`Background`, whose `magnitude_squared` adds linear-RGB channels to px stroke widths and shadow offsets.
  - Example: a dark-theme hover from `#121212` (linear 0.00606) to `#1c1c1c` (linear 0.01162) gives Δ = 0.00556 per channel, Σ² = 9.3e-5 < 1e-4. The retarget hits the snap-if-close path and jumps with no animation under a spring spec.
  - The same change under a duration spec animates, because that floor is 1e-4. `duration.rs` documents exactly this failure for durations.
  - Every spring colour fade also ends with a snap of up to ~0.006 linear per channel, which is several 8-bit sRGB levels near black.
  - This reaches the default `Switch` track cross-fade (`toggle.rs:144`, `AnimSpec::SPRING`).

## Duration snap check runs mid-curve
- [ ] `src/animation/anim_map_typed.rs:175-194` **bug (low)**: the duration snap-if-close check runs on every unsettled tick, not only on retarget.
  - With `Easing::OutBack`, the curve crosses the target before overshooting. A frame that lands within 1e-4 of the target at that crossing snaps and settles, cutting off the overshoot.
  - For small deltas, e.g. a 0.01 colour change at 60 fps over 0.2 s, the per-frame step is about 2e-3, so roughly 10% of such animations are truncated.

## ColorPicker history fills with non-picks
- [ ] `src/widgets/color_field/mod.rs:126`, `color_strip/mod.rs:138` → `color_picker/mod.rs:381` **bug**: every keyboard nudge sets `committed`, and `apply` pushes on every commit. 16 ArrowRight presses (or a held key) evict all 16 presets with near-identical shades, against the dedupe rationale in `History::push`.
- [ ] `src/widgets/color_picker/mod.rs:451-456` **bug (plausible)**: hex `lost_focus` with an unchanged, valid buffer still commits. Tabbing through the hex field reorders history and reports `committed`.

## Theme derivations that break their own invariants
- [ ] `src/widgets/theme/color_picker.rs:195` **bug**: `value.editor = mono_edit(p)` replaces the `from_chip`-derived editor with a TextEditTheme box, against DragValueTheme's "chip and editor are pixel-identical" invariant. The stroke is 1.5 vs the chip's 1.0, folded into padding, so a channel value grows 1 px taller and its text shifts 0.5 px when it becomes editable.
- [ ] `src/widgets/theme/color_picker.rs:126`: `ambient()` re-derives `Theme::from_palette`'s `text` (`mod.rs:306`) — a second source of truth.

## Scroll reach and paging
- [ ] `src/widgets/scroll/state.rs:174-181` **bug (plausible)**: the offset band is `content*zoom - viewport`, where viewport is the outer box less gutter and padding (`scrollbars_def.rs:62`). Padding sits inside the zoomed transform, so at zoom z the last `pad_left*(z-1)` px of content is unreachable. Confirm with a zoomable `Scroll::both().padding(10)` at zoom 2, scrolled to the end.
- [ ] `src/widgets/scroll/bars.rs:33-36` vs `:126`: the doc says the track pages "on press", but `drive` pages on `clicked()`, i.e. on release, once per click.

## Smaller widget bugs
- [ ] `src/widgets/expander/mod.rs:247-253` **bug (plausible)**: on the frame the reveal tween settles, `response_for(body_id)` returns the previous frame's clipped rect (`max_size = openness_prev * full`), and that is stored as `height`. A settled `animate` requests no repaint. If the next interaction is keyboard-only (Space on the focused header), the collapse tween clips against e.g. 0.97×full and the body visibly jumps.
- [ ] `src/widgets/context_menu/menu_item.rs:88` **bug**: `MenuItem::separator()` lacks `#[track_caller]`. Every separator in the program gets this line's auto id and relies on occurrence-count disambiguation, against the contract in `widgets/mod.rs` ("every widget constructor is `#[track_caller]`").
- [ ] `src/widgets/response.rs:65`: says external authors reach `Response::lazy` "through `Widget::response`". That returns a `ResponseState`. The lazy route is `Widget::show`.

## Frame-start snapshots read outside the pass that took them
- [ ] `src/input/input_state/mod.rs:855` against `app.rs:13` **bug (plausible)**: `frame_quiescent` is re-snapshotted only in `pre_record`, but `App::update` (documented as exposing "the current frame's ... unsuppressed input") runs before that. Scenario: the last pass ran with the pointer off-surface, then the pointer enters onto a button and clicks before any frame runs. In `update`, `ui.response_for(id).clicked()` returns false (stale snapshot), while `ui.pointer_actions()` reports the click.
- [ ] `src/ui/frame_cycle.rs:233-241` **bug (plausible)**: warmup runs record against a scratch `InputState`. It keeps state rows, animations, wakes and window commands, but discards `set_focus` / `clear_focus` / `release_input_scope`. An app that does a one-shot first-frame `ui.set_focus(id)` from `record` loses it, and `update` cannot do it instead because it gets `&Ui`.

## Release builds lack screens the primitive docs promise
- [ ] `src/primitives/mesh/mod.rs:165,113,189` **bug (plausible)**: triangle indices are range-checked only by `debug_assert` in `triangle`, and not at all in `append` or `is_noop`. The module doc promises "index … screens that keep a malformed one from reaching the renderer". In release, an index ≥ vertex count reaches `draw_indexed` with `base_vertex` into the shared arena (`gpu/mesh_pipeline.rs:180`). It reads a neighbouring mesh's vertices and paints triangles outside the recorded bbox, which escapes culling and damage.
- [ ] `src/primitives/rect/mod.rs:356,340` **bug (latent)**: `clamp_to` uses IEEE `max`/`min`, which drop a NaN operand. So `Rect::NAN.clamp_to(Rect::new(0,0,100,100))` returns `(0,0,100,100)`, and `Rect::NAN.intersect(b)` returns `Some(b)`. Callers: `scene/cascade/paint_rect.rs:44`, `damage/walk.rs:375`. Today only the upstream `Shapes::add` gate shields them.

## Encoded text cache identity is coarser than the extraction input
- [ ] `src/text/cosmic/mod.rs:661,687` vs `src/gpu/text/encode/mod.rs:85` **bug (plausible, minor)**: the `EncodedKey` stores only the subpixel bin of `row.origin`. Extraction bins each glyph from the exact `origin.x - left*scale + glyph.x*scale`. Two origins in the same bin can produce different per-glyph bins: fractions 0.13 and 0.37 are both bin One, and a glyph at +0.25 lands in bin One vs Two. The cached template from whichever origin came first is replayed for the other, so glyphs are off by up to 0.25 px depending on cache history.

## `IconId` is u16 but icon sets are unbounded
- [ ] `src/icons/icon_set.rs:165` with `icon_table.rs:283` **bug (low)**: `from_svgs` accepts any number of sources, but `by_name` mints `IconId(i as u16)`. In a 70 000-icon set, the name at index 65 540 silently resolves to icon 4. The `gpu/icon` prewarm has the same truncation.
  - `from_svgs` also neither rejects nor dedupes duplicate names, though `IconDef::name` is documented "unique within a set". `by_name` then picks one arbitrarily.
- [ ] `src/icons/icon_set.rs:141-143`: the comment says a cross-set id "fails at the call site that mixed them". Only an out-of-range id fails. An in-range id from another set draws the wrong icon silently.

## Unpinned numbering invariant in the curve shader
- [ ] `src/gpu/curve_pipeline/curve.wgsl:260`: `in.kind >= KIND_JOIN_ROUND` assumes every join kind is ≥ JOIN_ROUND and every non-join kind is below it. `renderer/render_buffer/curve.rs:27-28` says the joins' "order among themselves is free", and no const assert pins it. Renumbering BEVEL=3, ROUND=4 would silently send bevel joins down the strip path.

## Measure cache keyed on a paint-inclusive hash
- [ ] `src/layout/pass.rs:276` / `src/layout/engine.rs:221`: the measure cache and `matches_forest` key on `rollups.subtree`. That hash folds shape hashes and the chrome hash (`src/scene/tree/mod.rs:292,328`), so it changes on colour, hover tint or any paint animation.
  - Every paint-only change misses the measure cache on the whole ancestor chain (driver re-dispatch plus intrinsic queries per ancestor).
  - It also forces a rebuild frame: `capture_tree` copies every column of every layer, rebuilds `text_bounds` and the descriptors, and recomputes the identity, O(N) on every animated frame.
  - A layout-only rollup (sizing, bounds, panel, text content/font/wrap, grid/scroll defs, child ids) would let paint-only frames hit at the root.

## Composer worst-case per-frame cost
- [ ] `src/renderer/frontend/composer/occlusion.rs:140-149` **bug (perf)**: the prune is O(N·K) whenever covers equal quad sizes. Sharp, pixel-aligned opaque quads (the default under `pixel_snap`) record their full rect as the cover (`aa_inset` is 0). `q.rect.size > suffix_max` is then never true, and every quad scans all later occluders. A 100×100 grid of equal cells stays in one group: about 5·10⁷ `contains_rect` calls per full frame.
- [ ] `src/renderer/frontend/composer/higher_kind.rs:221-232,300-302` **bug (perf, plausible)**: the module doc's bound ("a query that survives the union pre-reject flushes, so a scan happens once per group") is false. `any_overlap` can scan every rect, find no hit, and not flush. Curve-after-curve never flushes, so curves accumulate. 2000 short `Shape::line` strokes plus 500 labels in the gaps costs about 10⁶ rect tests per frame.
- [ ] `src/renderer/gpu_paint/gpu_views.rs:117-121,138-143` with `session.rs:446-481` **bug (contract)**: `repaint(false)` is documented to cull the view and skip its GPU paint. Any partial damage that intersects the view makes the encoder re-emit it, the composer unconditionally pushes a `RenderTargetDraw`, and the backend calls `GpuPaint::paint` again. A hover over a static 3D view re-runs the app's render every frame, though recompositing the retained target would do. `RenderTargetDraw` carries no epoch, so the backend cannot tell the two cases apart.

## Damage worst-case-per-frame spike
- [ ] `src/scene/damage/walk.rs:362-378`: `emit_inverted_overlaps` pushes one rect per inverted overlapping pair into `raw_rects`, so the push count is O(rows²). Reversing the order of N fully overlapping children (a card deck, a canvas z-sort) pushes about N²/2 rects (N=1000 gives about 500k). Each goes through `DamageRegion::add`'s 8-slot scan and grows `raw_rects` to a new high-water mark.

## GPU per-frame cost and worst-case spikes
- [ ] `src/gpu/image_store.rs:191-224`: the 64 K-entry premultiply table is built lazily on the first non-opaque image write. That is about 65 536 × (decode + powf + 3 Newton steps) ≈ several ms, on whichever frame first registers a soft-edged image. Build it at backend construction or as a static table.
- [ ] `src/gpu/overlay_pass.rs:83-90`: the dim quad is re-uploaded through the belt on every Partial frame, although its content changes only with the viewport. `QuadPipeline::upload_clear` (`quad_pipeline.rs:108-126`) caches the same shape with `last_clear`. Two full-viewport single-quad buffers with different caching should share one mechanism.
- [ ] `src/gpu/viewport.rs:38`: a release `assert!` on the per-frame path (`PartialScissors::new`). Per-frame contract checks are `debug_assert!`.

## Icon prewarm warms keys the frame never asks for
- [ ] `src/gpu/icon/mod.rs:124` **bug (plausible)**: prewarm keys on `def.view_box * display_scale`. The composer keys on the drawn box `phys_rect.size`, which includes ancestor transforms (`composer/session.rs:381`). So prewarm hits only icons drawn at exactly their view-box size. Any other size still takes the 10-20× filtered raster lazily.
  - Prewarm rasterizes every filtered icon of every loaded set in one frame on each DPI change or set load: a worst-case-frame spike. Those slots are stamped current-frame, so they cannot be evicted while that frame's real draws compete for space.
  - To confirm: compare showcase icon box sizes against their SVG view boxes.

## Input per-event and per-frame cost
- [ ] `src/input/input_state/mod.rs:606-612`: KeyDown of a bare modifier (Shift/Ctrl/Alt → `Key::Other`, empty text) counts as observable whenever anything is focused. It is queued and `settle`s, so every modifier press while a field is focused costs two record passes (Ctrl+C costs four).
- [ ] `src/host/winit/mod.rs:427`: `Resized` invalidates all `SystemFacts`. During an interactive resize every frame is preceded by `Resized`, so `outer_position`, `is_maximized` and `current_monitor` (X11 round trips plus a `String` allocation) run per frame — the hot path the cache's doc says it removed. A resize can change only `maximized`; position changes arrive as `Moved`.

## Small widgets per-frame cost
- [ ] `src/widgets/color_picker/mod.rs:197` with `history.rs:68`: `with_state` does `mem::take`, which runs `PickerState::default()` → `History::presets()`. That is 16 Okhsv→RGB conversions per picker per frame, to build a placeholder that is immediately overwritten.
- [ ] `src/widgets/gpu_view/mod.rs:109-114`: an eager `response_for` probe on a widget that senses nothing by default and needs nothing before record. `Widget::show` (lazy) covers it.

## Big widgets reach `pub(crate)` internals ("widgets use only the public API")
- [ ] TextEdit: `src/widgets/text_edit/mod.rs:29,140` (`ScrollAxes`, `Widget::scroll`), `mod.rs:417` (`TextStyle::metrics_valid`), `mod.rs:431` (`Background::border_inset`), `text_geometry.rs:107` (`Align::place_in`), `view_state.rs:9` and `paint_input.rs:13` (`ScrollState`/`ScrollBounds` with `apply_wheel_pan`, `clamp_to_natural`, `transform`).
- [ ] Scroll: `src/widgets/scroll/mod.rs:16-17,139,213,425` (`ScrollbarsDef`/`BarGeometry`, `ScrollAxes`, `Widget::scroll`, `Ui::scroll_content`), `bars.rs:145-148` (`Widget::scrollbars`, `scrollbar_def`), `state.rs:277` and `bars.rs:134` (`Axis::main_v`/`main`).
- [ ] Tabs/dock: `src/widgets/tabs/tab_strip.rs:212` `TabStrip::insertion_slot` (used from `tabbed_view.rs:346` and `dock/pane_geometry.rs:71`), `tab_item.rs:83` `TabItemBuf` (used by TabbedView and DockView), `dock_view.rs:141,360` (`DockState::drag` / `drop_target`).
- [ ] Theme helpers that other widgets call: `combo_box.rs:42` `chevron_pts` (built on the `pub(crate)` `Arrow`), `toggle.rs:86` `check_polyline`, `expander.rs:85` `arrow_angle`.

## Small widgets reach non-public API ("widgets use only the public API")
- [ ] `popup/mod.rs:214`, `modal/mod.rs:111`, `tooltip/mod.rs:238`: `OverlayScope`/`Backdrop` are `pub(super)`. Popup, Modal and Tooltip cannot be rebuilt outside the crate.
- [ ] `gpu_view/mod.rs:112`: `Ui::gpu_view` and `GpuPaintRef` are `pub(crate)`.
- [ ] `slider/mod.rs:130,138`, `drag_value/mod.rs:233,253,271,365,382`: `DragNum::read/commit_drag/commit_value/parse_from/edit_string`, `Num` and `Limits` are `pub(crate)`. Only the `DragNum` enum is public.
- [ ] `drag_value/mod.rs:392`: `Response::lazy` is `pub(super)`.
- [ ] `splitter/mod.rs:169,213,224,247`: `Axis::main/main_v/rows_cols/compose_spacing` and `CursorIcon::resize_along` are `pub(crate)`.
- [ ] `checkbox/mod.rs:89`, `combo_box/mod.rs:169`, `expander/mod.rs:196`: theme helpers `check_polyline`, `chevron_pts` and `arrow_angle` are `pub(crate)`, as is `Arrow` (`arrow.rs`).
- [ ] `toggle_chrome/mod.rs` (whole file), `checkerboard.rs`, `color_surface.rs` (`ColorSurface`, `texel_size`, `checked_downsample`, `DOWNSAMPLE`), `axis_keys.rs`: crate-only scaffolding the shipped widgets depend on.
- [ ] `widget/mod.rs:128,148,435`: `Widget::scroll`, `scrollbars` and `scrollbar_def` are `pub(crate)`, yet documented as authoring surface ("as `crate::Scroll` does").

## Public API leaks third-party crate types
- [ ] `src/diagnostics/gpu_pass_stats.rs:32`: public `BatchKind` derives strum's `EnumIter`/`EnumCount`, so iterating it requires the caller to depend on the same strum major. `PointerButton` hides this behind an inherent `iter()`, and `flag_set` removed bitflags for exactly this reason.
- [ ] `src/golden/mod.rs:14,45,108,172`: the public `golden` API takes and returns `image::RgbaImage` without re-exporting `image`. A consumer has to keep a semver-identical `image` dependency by hand — the hazard `lib.rs:225-238` cites for re-exporting `wgpu`.

## Dependencies that could go
- [ ] `Cargo.toml` `golden = ["dep:image", "dep:rayon"]`: rayon is used only for the row-parallel scan in `golden/mod.rs:77-83`. A sequential scan of a 2560×1440 diff is milliseconds. Dropping it removes rayon-core and crossbeam from `golden` builds.
- [ ] `Cargo.toml` `memchr`: a direct dependency for a single `memchr2` call (`widgets/text_edit/unicode.rs:21`). It is already transitive via roxmltree, and std `str::contains(['\n','\r'])` covers the use.

## Layout cache plumbing hazards and missed replay
- [ ] `src/layout/cache/mod.rs:468`: on the first non-empty layer, `capture_tree` swaps `scratch.desired` and `scratch.available_q` into the snapshot. Until the next `resize_for`, both scratch columns are empty Vecs, and the container-text pass runs in that window (`engine.rs`, after :274). Later layers copy instead, so the two cases are asymmetric. Nothing reads the columns there today, but the invariant is undocumented, and a future read would index out of bounds.
- [ ] `src/layout/pass.rs:289`: `arrange_src` is stamped only on the hit subtree's root. When that root is arranged at a new size, the entire subtree falls back to full driver dispatch, though every descendant's desired and authoring is equally proven. Inner subtrees whose slot size is unchanged could still replay.

## Renderer design and duplication
- [ ] `src/renderer/frontend/mod.rs:73` with `composer/mod.rs:91,163-174`: the host holds a `NonZeroU32` or `TextureLimit`, calls `.get()`, and `Composer::new` rewraps it with `.expect(...)`. The round trip adds a panic path and threads the device ceiling as a bare `u32`, which `TextureLimit`'s doc says it exists to prevent.
- [ ] `src/renderer/render_buffer/image.rs:35` with `session.rs:477`: `RenderTargetDraw.display_scale` copies `RenderBuffer.display.scale_factor()` into every target — a second source of truth for one per-frame value.
- [ ] `src/renderer/frontend/encoder/mod.rs:26-79` with `layer_ctx.rs:66-68,86-97`: `gradients`, `gradient_atlas` and `gradient_resolver` are three `LayerCtx` fields for one concern, and both resolver methods take the slice and the atlas on every call.
- [ ] `src/renderer/frontend/payload/draw_quad_payload.rs:114-156` with `layer_ctx.rs:166-174`: `rect` / `rect_window` / `rect_impl(window: bool)` exist only so the encoder can match on `RectKind` to pick one. Taking `RectKind` removes the bool parameter and the dispatch.
- [ ] Style: `pub(crate)` free functions that could be methods: `stroke_bounds::bbox` (`shape/stroke_bounds/mod.rs:17`), `bake::row` (`bake.rs:95`), `cap_lanes` (`render_buffer/curve.rs:115`). Missing `const fn`: `RenderPlan::cull_margin`, `TextureLimit::{from_device,max_dimension}`, `PaintTier::idx`, `cap_lanes`, `geometry::phys_scale`, `Display::{scale_factor,from_physical}`. Comments that narrate history: `text_grid/mod.rs:1-3` ("Replaces a flat…"), `:279-283` ("Profiling motivation"), `session.rs:439-445` ("which is how this was found"), `encoder/mod.rs:106-109` ("pre-fusion capture").

## Duplication and asymmetry in the GPU render loop
- [ ] `src/gpu/mod.rs:892-901`: `macro_rules! rebind` breaks the no-macro rule. A small helper taking `&mut Bound` plus a bind closure does the same.
- [ ] `src/gpu/mod.rs:450` vs `:868`: `ViewportPush::for_buffer` is computed in `submit` and recomputed inside `render_groups` once per damage rect. The dim and overlay passes are handed `viewport`. The main pass should be too.
- [ ] `src/gpu/quad_pipeline.rs:283`, `mesh_pipeline.rs:103`, `image_pipeline/mod.rs:97`, `curve_pipeline/mod.rs:132`, `raster_program.rs:363`, `blit_pipeline.rs:46`: each `build_variants`/`build` creates its pipeline layout per swapchain format. Layouts are format-independent, so they belong in each pipeline's `new`, beside the shader.
- [ ] `src/gpu/mod.rs:660,677,969,1009` with `text/mod.rs:46`, `icon/mod.rs:42`: the backend reaches `self.text.pass.flush` / `self.text.pass.render_batch` through a `pub(super)` field but calls `prepare_batch` as a method. Each raster tenant's API is split between methods and a reached-through field.

## Scene design and simplification
- [ ] `src/scene/cascade/engine.rs:254-266`: the `layout_hashes` doc says layers with no tree "keep the default" because `iter_paint_order` skips them. It skips nothing: every layer is visited, and the `PerLayer::default()` it overwrites is dead.
- [ ] `src/scene/damage/mod.rs:97-101`: `DamageEngine::budget_px` is a production field that exists only so a test can change it. Production always uses `DEFAULT_PASS_BUDGET_PX`.
- [ ] `src/scene/forest.rs:44,443`: `Forest::scratch` is `pub(crate)` but read only inside `forest.rs`. `current_scratch()` hands `ui` (`ui/mod.rs:1135`) the whole recording scratch when it only needs `ancestor_disabled()`.
- [ ] `src/scene/node/ident.rs:72,86`: the `Ident::Resolved` arms of `raw_id` and `is_explicit` cannot be reached, since `Widget::resolve` short-circuits `Resolved` before `Forest::widget_id`. A `Resolved` id fed back would be re-disambiguated, so a silent passthrough misstates the contract.
- [ ] Free functions that should be methods:
  - `src/scene/shapes/hash.rs:36` `compute_record_hash(&ShapeRecord)`
  - `src/scene/shapes/record/mod.rs:355,383` `mesh_paint_bbox_local` / `text_paint_bbox_local`
  - `src/scene/cascade/engine.rs:260,288` `layout_hashes(forest, ..)` / `cascade_fingerprint(forest, ..)`. The latter belongs beside `can_update` on `CascadeEngine`, which also closes the two-gate drift above.
- [ ] More than one major type per file:
  - `src/scene/shapes/paint.rs` (no `Paint` type in it) holds ten types. `QuadShape`, `ChromeRow`, `ShapeStroke`, `LoweredShadow` and `ImageSource` stand alone.
  - `src/scene/cascade/mod.rs` holds `Cascade`, `LayerCascade` and `CascadeInputHash` (the last is also used by damage's `NodeSnapshot`).
  - `src/scene/damage/mod.rs` holds `DamageEngine` beside `Damage`.
  - `src/scene/tree/paint_anims/mod.rs` holds `PaintMod`, `PaintAnims` and `PaintAnimCursor`.
  - `src/scene/layer.rs` holds `Layer` beside the generic `PerLayer`.
- [ ] Missing `const fn`:
  - `src/scene/tree/node_id.rs:18` `NodeId::idx`
  - `src/scene/tree/subtree_end.rs:29-78` (all five methods)
  - `src/scene/cascade/mod.rs:65-72` `CascadeInputHash::{pack, invisible}`
  - `src/scene/node/node_flags.rs:117-179` getters and setters
  - `src/scene/node/gaps.rs:133` `Gaps::as_u32`
  - `src/scene/shapes/paint.rs:74,406` `ShapeBrush::hash_parts`, `LoweredShadow::inset`
  - `src/scene/damage/mod.rs:252` `Damage::is_partial`

## Big widgets design and consolidation
- [ ] `src/widgets/dock/mod.rs:5` vs `dock_state.rs:716-838`: the module doc says the model is "pure data with no `Ui` in sight", but `DockState` carries `scan`, `drag`/`set_drag`, `drop_target` and `content_size`, all of which take `Ui`. That is view code on the model.
- [ ] `dock_state.rs:835` and `dock_tabs.rs:200`: a size is passed as `Option<Vec2>` where `Size` exists.
- [ ] ButtonTheme/TextEditTheme/TabsTheme/ToggleTheme/MenuItemTheme each have a public `pick` that duplicates `ThemeSlot::look`, while ExpanderTheme has none: two entry points with an asymmetric set.
- [ ] `src/widgets/scroll/state.rs:21-27`: TextEdit's `ViewState.scroll` carries `zoom` and the thumb `drag_anchor`, which a text viewport never uses. The shared type is wider than TextEdit needs, and that width forces the `pub(crate)` reach-in.
- [ ] Stale docs: `text_edit/mod.rs:86,98-99` and `unicode.rs:13` claim an "IME/text commit" insertion path that does not exist (`KeyText` caps at 14 bytes and carries no IME commit). `view_state.rs:35-38` speaks of a host that "assigns `EditState::caret`", which the host cannot do (the field is private). `scrollbars_def.rs:17,25` link `Widget::scrollbars` / `scrollbar_def` as public API.
- [ ] `src/widgets/text_edit/mod.rs:351`: `pass` re-resolves the id that `show` already resolved.

## Small widgets design and consolidation
- [ ] `color_button/mod.rs:46` vs `combo_box/mod.rs:28`: `ChipState`/`ComboState` are identical `{open}` structs. The "probe flag → toggle on click → `Popup::below(rect)` → close on `closed()` → write back on flip" block is duplicated verbatim.
- [ ] `color_field/mod.rs:176-204` vs `color_strip/mod.rs:236-258`: duplicated key handling with diverging semantics. PageUp/PageDown jump the field's value axis to 1/0, but step the strip by ±0.1. The press → keyed → committed scaffolding and the surface/texel code are also copied.
- [ ] Keyboard support differs across siblings. ColorField/ColorStrip and Expander are focusable and key-driven. Slider, Checkbox, Radio and Switch are not focusable and have no key path.
- [ ] Escape differs across siblings. In DragValue edit mode (`drag_value/mod.rs:367-386`) Escape commits the typed text (the buffer is parsed live). The picker's hex field (`color_picker/mod.rs:451`) treats Escape (`cancelled`) as revert.
- [ ] `color_button/mod.rs`: lacks ColorPicker's `swatches(&[RgbaF32])` and `downsample(n)`. Its `history` default (true) also differs from ColorPicker's (Hidden).
- [ ] `color_picker/mod.rs:93-97`: `written: RgbaF32` + `seeded: bool` is one fact stored twice.
- [ ] `color_picker/mod.rs:427-500`: the value grid offers H and S but no V. With alpha off, cell (0,2) is empty.
- [ ] `overlay_scope.rs:103`: the `owner` argument duplicates the root's resolved id at every call site, and Tooltip passes the wrong one: `tooltip/mod.rs:239` passes `bubble_id` even when the caller set an explicit `.id()` on the bubble.
- [ ] Naming: `Tooltip::on(&snapshot)` vs `ContextMenu::attach(ui, &snapshot)` are two names for "attach to a trigger snapshot".
- [ ] File layout: `TooltipResponse`, `ExpanderResponse`, `ClickOutside` (`popup/mod.rs`) and `SplitHalf` (`splitter/mod.rs`) are standalone public types inside a widget's file, while `ValueResponse`/`SelectResponse`/`OverlayResponse` each get their own file.

## Input and host structure
- [ ] `src/host/winit/error.rs:63-88`: `WinitHostError::Gpu` and its `From<GpuRequestError>` are never constructed in production (only tests). Device failures surface as `Surface{source: SurfaceError::Device}`, so one failure has two documented routes. Public API: removal needs a go-ahead.
- [ ] `src/window/window_commands.rs:33`: `open` dedupes per token but `close` does not. Replayed passes (warmup, pass A, pass B) push the same close up to three times, and the host drains them as no-ops.
- [ ] `src/input/input_state/mod.rs:43-131`: `focused`, `modifiers`, `pointer_pos`, `hovered`, `focus_policy`, `input_policy` and `signal_since_last_frame` are `pub(crate)` fields read and written directly from `Ui` and `FrameCycle`, though `set_focus` exists as a method. This contradicts `Ui`'s own "every field is private" rule one layer down.
- [ ] `src/host/offscreen.rs:337`: a top-level `#[cfg(any(test, feature="internals"))] impl OffscreenHost` with `pub fn`s. Gated impl blocks belong inside the file's gated `internals` mod.
- [ ] `src/ui/frame_runtime/mod.rs:165`: a mid-impl `#[cfg(test)] pub(crate) fn cascade_ran` is a mid-file gate that can move to the end-of-file test mod.
- [ ] `src/host/winit/runtime.rs:63`: `bootstrap.config.clone()` deep-copies the whole `WinitHostConfig` (title `String`, icon pixel `Vec`) only to read fields that can be borrowed.

## Text and primitives duplicated sources of truth
- [ ] `src/text/cosmic/mod.rs:876` vs `:576`: `shape_truncated` discards the `left` that `shaped_geometry` just measured and hardcodes `0.0`, relying on a comment about how cosmic places unbounded lines. `shape_wrapped` stores the measured `left`.
- [ ] `src/primitives/span.rs:33` vs `:65`: `Span::range()` and `From<Span> for Range<usize>` are the same conversion written twice.
- [ ] `src/primitives/spacing/mod.rs` (f16 lanes): `Spacing` is layout input, but f16 drops whole pixels past 2048 (margin 2049→2048, 3001→3000). A silent layout change, unlike `Corners`, which is paint-only.

## Renderer docs that state the opposite of the code
- [ ] `src/renderer/quad.rs:24-28`: says a gradient's `fill` is "unused (set to zero)". It is the white multiplier the shader applies (`c * in.fill`), and it carries the fade.
- [ ] `src/renderer/frontend/payload/draw_quad_payload.rs:225-231`: says "`gpu_fill` zeroes its colour lane". It sets `WHITE`.
- [ ] `src/renderer/frontend/paint_sink/mod.rs:58-67`: says `draw_polyline` "gates on nothing, and asserts instead". It asserts and also gates on `is_noop` (lines 211-219).
- [ ] `src/renderer/render_buffer/text.rs:342`: says "log-multiplicative ladder". `snap_text_scale` is additive (`TEXT_SCALE_STEP` doc).
- [ ] `src/renderer/frontend/payload/draw_curve_payload.rs:13-19` and `stroke_bounds.rs:78` name fields `bbox`/`rotation` that no longer exist and cite a "pivot contract in the module doc" that `payload/mod.rs` does not contain. `render_buffer/mod.rs:176` cites a nonexistent `Composer::compose`.

## Stale, broken or misplaced GPU docs
- [ ] `src/gpu/backbuffer.rs:4-13`: `Backbuffer`'s doc comment sits above `use glam::UVec2;`, so it documents the import and the struct (line 18) has none.
- [ ] `src/gpu/text/mod.rs:53`: broken intra-doc link `RasterPass::build_variants`. It is `RasterProgram::build_variants`.
- [ ] `src/gpu/text/mod.rs:11,22`: claims an "RgbaF32" colour side (it is `Rgba8UnormSrgb`) and "20-byte instances … uv high bit" (`RasterQuad` is 24 bytes, kind at bit 15).
- [ ] `src/gpu/text/mod.rs:93`: "Rebinds the atlas bind group if it grew" — the rebind happens inside `RasterAtlas::grow`.
- [ ] `src/gpu/image_pipeline/image.wgsl:5-8`: the header says the shader premultiplies the texel at write time. Texels are premultiplied at upload (`image_store::premultiply_into`), and `fs` treats `s` as already premultiplied.
- [ ] `src/gpu/prelude.wgsl:10-11`: names `TextBackend::render_batch` as the atlas-size writer. It is `RasterAtlas::draw_span`.
- [ ] `src/gpu/raster_atlas/mod.rs:313-315`: says "Both halves of the shared immediate region get written", but `draw_span` writes only the params half. The viewport comes from the backend's rebind.
- [ ] `src/gpu/gpu_timings.rs:5`: references `host/winit/gpu/mod.rs`, which does not exist.
- [ ] `src/gpu/dynamic_buffer.rs:42`: `index` says "typically `u16`". The only index `DynamicBuffer` is `u32`, and curve's `u16` buffer is a plain `wgpu::Buffer`.
- [ ] `src/gpu/window_surface.rs:58`: `set_vsync` claims to compare against "what the surface resolved the policy to". It compares the config's own `AutoVsync`/`AutoNoVsync`, which wgpu never rewrites.

## Stale input docs that state wrong behavior
- [ ] `src/input/input_state/mod.rs:415-424` and `src/input/policy.rs:66-70` (`InputSignal::Inert`): both claim an `Inert` event disqualifies the paint-only path. Under the default `OnDelta`, `record_threshold()` is `Repaint` (`frame_runtime/mod.rs:271`), so it does not. `frame_cycle.rs:148-153` documents the opposite.
- [ ] `src/input/event_outcome.rs:16`: names a `Text` event that `InputEvent` does not have.
- [ ] `src/input/policy.rs:15,79`: refer to "Keys / IME" and "IME text", but there is no IME path (winit `Ime` is never enabled or translated). There is also no Tab focus traversal. Both are feature gaps, but the docs imply IME exists.

## Text docs that contradict the code
- [ ] `src/text/request.rs:60-63`: says `ShapedTextRef::new`'s pairing check "holds in release". It is a `debug_assert_eq!` (`shaped_ref.rs:30`), so no release pairing check exists.
- [ ] `src/text/probe/mod.rs:57`: refers to a key field `halign_q` that does not exist. The align lives in `FaceBits` bits 11..12.
- [ ] `src/text/glyphs/mod.rs:68,80`: `TextGlyphs::line` claims "one unwrapped line" (a `\n` yields several lines) and "not binned at all … one entry per glyph per size rather than four". Per-glyph `glyph.x*scale` fractions are still binned, so a caller's atlas gets up to four entries per glyph.

## Support-module docs that contradict the code
- [ ] `src/lib.rs:124-127`: says the derive emits `::palantir::Animatable`. It emits `::palantir::widget::Animatable`.
- [ ] `src/lib.rs:209-212` **(plausible)**: says the GPU reach-in is "never in a plain `cargo test` build". But `Cargo.toml`'s self dev-dependency `palantir = { features = ["internals"] }` is documented there as unifying into every test target, which makes `internals` always on under `cargo test`. One of the two comments is wrong. The same unification would make `required-features = ["internals"]` on `[[test]] alloc` redundant.
- [ ] `src/icons/icon_registry/mod.rs:520-538`: `resident` is documented as `None` for a free slot. A released-but-undrained slot still answers `Some`, and the epoch bump on release triggers a prewarm walk that includes the doomed set.

## Stale or false layout docs
- [ ] `src/layout/axis_slot.rs:59`: "Both floor at `max(content, intrinsic_min)`" is false for Hug. That branch floors only at `intrinsic_min` after `.min(available - margin)` (`:92-94`), so a Hug rect can be smaller than its measured content while Fill's cannot.
- [ ] `src/layout/layout_scratch.rs:181`: says `desired` is restored by "the measure-hit site itself (`LayoutPass::replay_arranged`'s caller)". `replay_arranged`'s caller is `arrange`. `desired` is restored in `LayoutPass::measure`.
- [ ] `src/layout/layout_scratch.rs:88`: calls `arrange_src` "the one category-(2) field", but it is neither listed in category 2 nor captured or restored by the cache. It is frame-local.
- [ ] `src/layout/counters.rs:26`: claims `LayoutPass::arrange` "walks every node with full driver dispatch regardless". Arrange replay contradicts this.
- [ ] `src/layout/engine.rs:28`: says scratch is "Cleared at the top of every `run`". It is reset per layer in `LayoutScratch::resize_for`.
- [ ] References to functions that no longer exist: `resolve_desired` (`src/layout/pass.rs:68`) and `resolve_sizing` (`src/layout/grid/measuring.rs:215`, `src/layout/scroll/mod.rs:65`, `src/layout/types/scroll_axes.rs:143`, `src/layout/stack/mod.rs:97`). The current function is `AxisSlot::resolve_node` / `resolve`.
- [ ] `src/layout/text_shape_input.rs:54`: says `LayoutPass::measure_dispatch` drives wrap shaping. It is `MeasureOp::leaf`.
- [ ] `src/layout/stack/mod.rs:197`: "since the `AxisSlot::resolve` change pins Fill at content" narrates a past change.

## Layout style-rule violations
- [ ] `src/layout/mod.rs`: hosts three major types with impls (`LayerLayout`, `Layout`, `ShapedText`).
- [ ] `src/layout/cache/mod.rs`: `MeasureSnapshot` and `NodeArenas` have their own impls and share the file with `MeasureCache`.
- [ ] `src/layout/intrinsic/mod.rs`: holds `LenReq`, `IntrinsicRange`, `IntrinsicWalk`, `IntrinsicQuery` and `IntrinsicOp` in one file.
- [ ] `src/layout/scrollbars/scrollbars_def.rs:122`: `BarGeometry` stands on its own (it has an impl and widget consumers) but lives in `scrollbars_def.rs`.
- [ ] `src/layout/types/layout_mode.rs`: `PackedLayoutMeta` shares the file with `LayoutMode`.
- [ ] Exposed free functions where methods are preferred: `pub(crate) fn compute` (`src/layout/intrinsic/mod.rs:261`) and `pub(crate) fn quantize_available` (`src/layout/cache/mod.rs:77`).
- [ ] `measure_inner(pass, node, idx, depth, inner_avail)` and `arrange_inner(pass, node, inner, idx, depth)` (`src/layout/grid/measuring.rs:13`, `arranging.rs:13`) are sibling free functions with mismatched argument order.
- [ ] Missing `const fn`: `Axis::{bit, other, main, cross, main_v, cross_v, compose_size, compose_point, compose_rect, compose_spacing}`, `IntrinsicRange::get`, `IntrinsicQuery::includes`, `FillItem::new`, `JustifyOffsets::new`, `AxisSlot::{outer, inner_avail, resolve}`, `AxisAlignPair::resolve_axis`, `GridDepthStack::exit`.
- [ ] `src/layout/canvas/mod.rs:113`: tests Hug with `matches!(.., Sizing::HUG)`, while `measure`/`arrange` in the same file use `hug_mask()` / `is_hug()`.
- [ ] `src/layout/counters.rs:197,206`: `#[cfg(feature = "bench")]` and `#[cfg(test)]` impl blocks sit as free gated impls rather than inside the end-of-file gated module.

## Text and primitives style-rule violations
- [ ] `src/primitives/color/mod.rs:276,450,469`: one file holds two major public types (`RgbaF32`, `RgbaF16`) plus the Oklab conversions as `pub(crate)` free functions.
- [ ] `src/primitives/brush/gradient/mod.rs:185`: `FillAxis` is a standalone GPU-wire type that shadows use too, but it sits in the gradient module file.
- [ ] `src/text/cosmic/mod.rs:128`, `src/text/cosmic/cluster_glyph.rs:44`: `pub(crate)` free functions `warm_matches` and `fitting_prefix` could be methods.
- [ ] `src/text/font_family.rs:180`: `NameVisitor` has no `#[derive(Debug)]`.
- [ ] Missing `const fn`: `GlyphFont::metrics_are_valid`/`metrics_valid`, `key::dequantize`, `LineFit::resolves_to_unbounded`, `TextWrap::{line_fit, floor_scan, min_content, max_content, content_size}`, `TextRoot::wrap_floor`, `Rect::{deflated, square_about, spin_pivot}`, `Size::{room_past, select}`.

## Simplifications and style-rule violations (support modules)
- [ ] `src/animation/anim_map_typed.rs:103-110,208-237`: `row.motion` and `spec.motion` are matched separately: once for `same_motion`, then again with two `unreachable!` arms. One match on the `(&mut row.motion, spec.motion)` pair removes both.
- [ ] `src/animation/mod.rs:83,90,98`: `typed_mut`, `try_typed_mut` and `is_empty` are `pub(crate)`, but only `animation` and its child test modules call them.
- [ ] `src/icons/icon_table.rs:202,283`: `IconDef::name: &'static str` forces `from_svgs` to take `&'static str` names. A runtime set built from files on disk has to `Box::leak` every name, which contradicts "owns its buffers, so nothing leaks".
- [ ] `src/common/index16.rs:236`: `From<Index16> for u16` returns the encoded value (index + 1), not the index. It pairs with `from_raw`, so it should be a named `to_raw()`. `u16::from(idx)` reads as the index.
- [ ] `src/frame_fixture/mod.rs:59,63,73`: `#[cfg(any(test, feature = "internals"))]` sits inside a module already gated `#[cfg(feature = "internals")]` (`lib.rs:172`), so the `test` arm is dead.
- [ ] `src/bench/mod.rs:55-59`: `use cli::Cli; use driver::DRIVERS;` are relative paths. The rule is `crate::bench::…`.
