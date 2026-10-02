# Crate review

Whoever addresses an item deletes it from this file. A group goes when its last item goes.

Groups run from the most severe to the least: panics on reachable input first, then wrong output, then per-frame cost, then design, docs and style. Items tagged **bug** were traced through the code. Items tagged **bug (plausible)** name the check that would confirm them.

## Theme and preference file data reach asserts
`F32Ext::themed_length` says theme scalars are hand-edited file data that "cannot assert". Most widgets screen them. These do not, and `SpinnerTheme`/`ToggleTheme`/… derive `Deserialize`:
- [ ] `src/display/user_scale.rs:27-29,71-77` **bug**: the doc tells apps to read a persisted preference back through `UserScale::new`, which `assert!`s on a non-finite value. A config file containing `nan` crashes the app. Public API: a fallible constructor needs a go-ahead.

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

## TabbedView reorder and identity
- [ ] `src/widgets/tabs/tabbed_view.rs:276`: chips are keyed `i as u64`, which TabItem's doc (`tab_item.rs:8-12`) says hands one chip's state to another. After Closed/Reordered, the look animation and hover state of slot i transfer to whatever page slid in.

## Scroll reach and paging
- [ ] `src/widgets/scroll/state.rs:174-181` **bug (plausible)**: the offset band is `content*zoom - viewport`, where viewport is the outer box less gutter and padding (`scrollbars_def.rs:62`). Padding sits inside the zoomed transform, so at zoom z the last `pad_left*(z-1)` px of content is unreachable. Confirm with a zoomable `Scroll::both().padding(10)` at zoom 2, scrolled to the end.
- [ ] `src/widgets/scroll/bars.rs:33-36` vs `:126`: the doc says the track pages "on press", but `drive` pages on `clicked()`, i.e. on release, once per click.

## Smaller widget bugs
- [ ] `src/widgets/expander/mod.rs:247-253` **bug (plausible)**: on the frame the reveal tween settles, `response_for(body_id)` returns the previous frame's clipped rect (`max_size = openness_prev * full`), and that is stored as `height`. A settled `animate` requests no repaint. If the next interaction is keyboard-only (Space on the focused header), the collapse tween clips against e.g. 0.97×full and the body visibly jumps.
- [ ] `src/widgets/response.rs:65`: says external authors reach `Response::lazy` "through `Widget::response`". That returns a `ResponseState`. The lazy route is `Widget::show`.

## Frame-start snapshots read outside the pass that took them
- [ ] `src/input/input_state/mod.rs:855` against `app.rs:13` **bug (plausible)**: `frame_quiescent` is re-snapshotted only in `pre_record`, but `App::update` (documented as exposing "the current frame's ... unsuppressed input") runs before that. Scenario: the last pass ran with the pointer off-surface, then the pointer enters onto a button and clicks before any frame runs. In `update`, `ui.response_for(id).clicked()` returns false (stale snapshot), while `ui.pointer_actions()` reports the click.
- [ ] `src/ui/frame_cycle.rs:233-241` **bug (plausible)**: warmup runs record against a scratch `InputState`. It keeps state rows, animations, wakes and window commands, but discards `set_focus` / `clear_focus` / `release_input_scope`. An app that does a one-shot first-frame `ui.set_focus(id)` from `record` loses it, and `update` cannot do it instead because it gets `&Ui`.

## `IconId` is u16 but icon sets are unbounded
- [ ] `src/icons/icon_set.rs:165` with `icon_table.rs:283` **bug (low)**: `from_svgs` accepts any number of sources, but `by_name` mints `IconId(i as u16)`. In a 70 000-icon set, the name at index 65 540 silently resolves to icon 4. The `gpu/icon` prewarm has the same truncation.
  - `from_svgs` also neither rejects nor dedupes duplicate names, though `IconDef::name` is documented "unique within a set". `by_name` then picks one arbitrarily.
- [ ] `src/icons/icon_set.rs:141-143`: the comment says a cross-set id "fails at the call site that mixed them". Only an out-of-range id fails. An in-range id from another set draws the wrong icon silently.

## Composer worst-case per-frame cost
- [ ] `src/renderer/frontend/composer/occlusion.rs:140-149` **bug (perf)**: the prune is O(N·K) whenever covers equal quad sizes. Sharp, pixel-aligned opaque quads (the default under `pixel_snap`) record their full rect as the cover (`aa_inset` is 0). `q.rect.size > suffix_max` is then never true, and every quad scans all later occluders. A 100×100 grid of equal cells stays in one group: about 5·10⁷ `contains_rect` calls per full frame.
- [ ] `src/renderer/frontend/composer/higher_kind.rs:221-232,300-302` **bug (perf, plausible)**: the module doc's bound ("a query that survives the union pre-reject flushes, so a scan happens once per group") is false. `any_overlap` can scan every rect, find no hit, and not flush. Curve-after-curve never flushes, so curves accumulate. 2000 short `Shape::line` strokes plus 500 labels in the gaps costs about 10⁶ rect tests per frame.
- [ ] `src/renderer/gpu_paint/gpu_views.rs:117-121,138-143` with `session.rs:446-481` **bug (contract)**: `repaint(false)` is documented to cull the view and skip its GPU paint. Any partial damage that intersects the view makes the encoder re-emit it, the composer unconditionally pushes a `RenderTargetDraw`, and the backend calls `GpuPaint::paint` again. A hover over a static 3D view re-runs the app's render every frame, though recompositing the retained target would do. `RenderTargetDraw` carries no epoch, so the backend cannot tell the two cases apart.

## Damage worst-case-per-frame spike
- [ ] `src/scene/damage/walk.rs:362-378`: `emit_inverted_overlaps` pushes one rect per inverted overlapping pair into `raw_rects`, so the push count is O(rows²). Reversing the order of N fully overlapping children (a card deck, a canvas z-sort) pushes about N²/2 rects (N=1000 gives about 500k). Each goes through `DamageRegion::add`'s 8-slot scan and grows `raw_rects` to a new high-water mark.

## GPU per-frame cost and worst-case spikes
- [ ] `src/gpu/overlay_pass.rs:83-90`: the dim quad is re-uploaded through the belt on every Partial frame, although its content changes only with the viewport. `QuadPipeline::upload_clear` (`quad_pipeline.rs:108-126`) caches the same shape with `last_clear`. Two full-viewport single-quad buffers with different caching should share one mechanism.
- [ ] `src/gpu/viewport.rs:38`: a release `assert!` on the per-frame path (`PartialScissors::new`). Per-frame contract checks are `debug_assert!`.

## Icon prewarm warms keys the frame never asks for
- [ ] `src/gpu/icon/mod.rs:124` **bug (plausible)**: prewarm keys on `def.view_box * display_scale`. The composer keys on the drawn box `phys_rect.size`, which includes ancestor transforms (`composer/session.rs:381`). So prewarm hits only icons drawn at exactly their view-box size. Any other size still takes the 10-20× filtered raster lazily.
  - Prewarm rasterizes every filtered icon of every loaded set in one frame on each DPI change or set load: a worst-case-frame spike. Those slots are stamped current-frame, so they cannot be evicted while that frame's real draws compete for space.
  - To confirm: compare showcase icon box sizes against their SVG view boxes.

## Small widgets per-frame cost
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
- [ ] Keyboard support differs across siblings. ColorField/ColorStrip and Expander are focusable and key-driven. Slider, Checkbox, Radio and Switch are not focusable and have no key path.
- [ ] `color_button/mod.rs`: lacks ColorPicker's `swatches(&[RgbaF32])` and `downsample(n)`. Its `history` default (true) also differs from ColorPicker's (Hidden).
- [ ] Naming: `Tooltip::on(&snapshot)` vs `ContextMenu::attach(ui, &snapshot)` are two names for "attach to a trigger snapshot".
- [ ] File layout: `TooltipResponse`, `ExpanderResponse`, `ClickOutside` (`popup/mod.rs`) and `SplitHalf` (`splitter/mod.rs`) are standalone public types inside a widget's file, while `ValueResponse`/`SelectResponse`/`OverlayResponse` each get their own file.

## Input and host structure
- [ ] `src/host/winit/error.rs:63-88`: `WinitHostError::Gpu` and its `From<GpuRequestError>` are never constructed in production (only tests). Device failures surface as `Surface{source: SurfaceError::Device}`, so one failure has two documented routes. Public API: removal needs a go-ahead.
- [ ] `src/input/input_state/mod.rs:43-131`: `focused`, `modifiers`, `pointer_pos`, `hovered`, `focus_policy`, `input_policy` and `signal_since_last_frame` are `pub(crate)` fields read and written directly from `Ui` and `FrameCycle`, though `set_focus` exists as a method. This contradicts `Ui`'s own "every field is private" rule one layer down.
- [ ] `src/host/winit/runtime.rs:63`: `bootstrap.config.clone()` deep-copies the whole `WinitHostConfig` (title `String`, icon pixel `Vec`) only to read fields that can be borrowed.

## Text and primitives duplicated sources of truth
- [ ] `src/text/cosmic/mod.rs:876` vs `:576`: `shape_truncated` discards the `left` that `shaped_geometry` just measured and hardcodes `0.0`, relying on a comment about how cosmic places unbounded lines. `shape_wrapped` stores the measured `left`.
- [ ] `src/primitives/span.rs:33` vs `:65`: `Span::range()` and `From<Span> for Range<usize>` are the same conversion written twice.

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
