# Merge `shapes-and-materials` into master as reviewable commits

`shapes-and-materials` (`89b0c79dd`) is one commit of 422 files. This plan splits it into ten
commits on master. The last commit makes the tree identical to `89b0c79dd`.

## Method

- **Sources.** The old branch commits are still in the object store, although the
  `effects-shapes` branch is deleted: `ebf25775c` (materials), `f410241e4` (geometry and paint),
  `fbfb89b1f` (pane to backdrop), and the review, `fbfb89b1f..89b0c79dd`. The three TODO-only
  commits fold into the step that touches `.notes/TODO.md` next.
- **Mechanical first, semantic after.** Each pure rename or move becomes a commit of its own.
  A script applies it to master or to the step before it. Then the semantic step takes the tree
  of the old commit with `git checkout <commit> -- .`. Its diff then holds only what the rename
  did not explain.
- **Each step is green.** Each step runs the full gate (`cargo fmt --all` and the eight `gate-*`
  aliases) and `cargo drive surface`. A step that fails gets its fix in that same step. The next
  tree checkout absorbs the fix.
- **The end is exact.** After step 10, `git diff 89b0c79dd` shows only the fixes that the
  status lists.
- **Removed from history:** `.notes/shape-paint-proposal.md`. `ebf25775c` adds it and
  `f410241e4` deletes it, so no step adds it.

## Commits

| # | Commit | Kind | Built from | Size |
|---|--------|------|-----------|------|
| 1 | Move effect name, kind and params into `primitives::effect` | move | script on master | 25 files |
| 2 | Rename the overlay `Backdrop` to `Barrier` | rename, public API | script on 1 | ~16 files |
| 3 | Replace backdrop filters with materials | semantic, public API | tree of `ebf25775c` | ~160 files |
| 4 | Rename `Stroke` to `Border` | rename, public API | script on 3 | ~104 files |
| 5 | Split a shape's geometry from its paint | semantic, public API | tree of `f410241e4` | ~250 files |
| 6 | Rename the renderer's pane to backdrop | rename | tree of `fbfb89b1f` | 141 files |
| 7 | Name a box's edge a border, and an outside rect "outside", in the renderer | rename | script on 6 | ~35 files |
| 8 | Move the record's geometry into `shape::geometry` | move | script on 7 | ~40 files |
| 9 | Name the material and shape API by its own vocabulary | API names | hunks of the review | ~25 files |
| 10 | Rename the material and outside-coverage goldens and fixtures | test names | tree of `89b0c79dd` | ~15 files |

### 1. Move effect name, kind and params into `primitives::effect`

`EffectName`, `EffectKind` and `EffectParams` move down a layer, so a paint value in
`primitives` can name an effect. Only paths change. No public item changes, as `lib.rs`
re-exports the same names.

### 2. Rename the overlay `Backdrop` to `Barrier`

`widget::Backdrop` becomes `Barrier`, `Backdrop::Barrier` becomes `Barrier::Surface`, and
`OverlaySpec::backdrop` becomes `barrier`. Prose changes only where it names this enum. Step 3
then uses "backdrop" for the captured pixels alone.

### 3. Replace backdrop filters with materials

`BackdropFilter` becomes `Material` over `MaterialInput`, with the stock `BLUR`, `FROSTED` and
`GLASS` effects. `Scrim` goes, and a modal's scrim becomes a `Chrome`. Reduced transparency
reads the window's ground (`ReducedBackdrop`). `PaneForm` folds into effect stages. This step
stays whole: `Scrim` holds a `BackdropFilter`, and the effect stages replace the pane shader, so
each part needs the others to compile. It also uses `ebf25775c`'s message body, with a subject
that says what it does. The subject that the commit has now describes step 5.

### 4. Rename `Stroke` to `Border`

The old `Stroke { color, width }` is a box's edge. A script renames the type, its module and
its uses. For this one step, lines and curves also take a `Border`. Step 5 gives them the new
`Stroke`. Thus about a third of `f410241e4`'s files leave the semantic step.

### 5. Split a shape's geometry from its paint

This is `Ui::add_shape(shape, paint)`, `Paint`, the new `Stroke` with cap, join and ramp,
`ImagePaint`, `IconPaint`, `EffectPaint`, `Fit` (which replaces `IconFit`), `Coverage`, the circle
and the arc in a rect, and records that place owner-relative geometry. This step stays whole:
each builder loses its paint and gains `at` and `fit` in the same change, and each caller changes
with it. `IconFit` to `Fit` is not a pure rename, as `Fit` adds `Cover` and a new default.

### 6. Rename the renderer's pane to backdrop

This is `fbfb89b1f` as it is, with the TODO typo fix: `BackdropKey`, `BackdropCaptures`,
`DirtyBackdrops`, `BackdropWalk`, `DrawBackdropPayload`, `PlaneBackdrop`, `BackdropReader`,
`EffectCapture`, and "fill" for "tint" in material prose.

### 7. Name a box's edge a border, and an outside rect "outside", in the renderer

The review's internal renames: `stroke_color`/`stroke_width` on `Quad`, `BackdropRow` and the
WGSL become `border_*`. The payloads' `stroke: LoweredBorder` becomes `border`.
`FillKind::WINDOW_BIT`, `FILL_FLAG_WINDOW`, `PALANTIR_WINDOW` and `composite_window` become
`OUTSIDE`. The quad tests' builders and names change with them.

### 8. Move the record's geometry into `shape::geometry`

`arc_geometry`, `bezier_geometry`, `triangle_geometry`, `point_placement`, `placed_stroke` and
`curve_basis` move out of `shape::paint`. `BoxGeometry`, `BoxOutline` and `PlacedBox` move out of
`quad_shape.rs` into `box_geometry.rs`.

### 9. Name the material and shape API by its own vocabulary

`EffectError::MaterialImageSlots`, the `backdrop` argument of the two `reduced_transparency`
setters, `const fn Stroke::is_noop`, `TriangleShape`'s method order, `InkBound::FillNotTranslucent`,
the polyline's `COLOR_PRODUCT_RULE`, and the docs that called a paint a shape. It also holds the
showcase's and tests' `stroke` variables that hold a `Border`, and the derive doc's wrap.

### 10. Rename the material and outside-coverage goldens and fixtures

`fixtures/backdrop.rs` becomes `material.rs`. The goldens become `materials`, `material_turned`,
`material_glass` and `outside_coverage_masks_corners`. The icon and image fixtures' `pane`
helpers become `cell`. Its tree is `89b0c79dd`'s, so this step also takes any hunk that steps 7
to 9 did not.

## Risks

- **An old commit that does not pass the gate.** The gate did not run on `ebf25775c`,
  `f410241e4` and `fbfb89b1f` in this session. Such a fix goes into that step and makes the
  step larger than the table says.
- **Scripted renames that differ from the old commit's hand renames.** Then the semantic step
  holds the difference. The diff shows it, and it compiles.
- **`API_SURFACE.md` loses the `EffectParams` trait** from step 1 on. The `drive` tool lists one
  item for a name that the derive macro and the trait share, and the moved `pub use` now sorts
  after the derive. This is a problem in `drive`. The palantir code is correct.
- **Local goldens.** The material goldens from before the branch fail from step 3 on. Run
  `UPDATE_GOLDEN=1 cargo visual` once after step 10.

## Status

- Step 1 is on master as `449ad9f83`. That commit also holds this file. Step 10 deletes it, as a
  proposal goes when its work lands.
- Step 2 is on master as `eb0cec486`. It also renames the overlay "backdrop" that `89b0c79dd`
  left in `modal/tests.rs`, `widget_core/configure.rs` and the overlays page. Steps 3 to 10
  keep these fixes, so the end tree is `89b0c79dd` with them.
- Step 3 is staged on master and not committed: the tree of `ebf25775c`, with the TODO edits of
  `a20e6f20f` and `2d7f4a42d`, without the proposal file, and with the step 2 fixes. The full
  gate passes. A review of it moved three names of step 9 into it: `EffectError::MaterialImageSlots`,
  the `backdrop` argument of the two `reduced_transparency` setters, and "`reduced_transparency`"
  for "the reduced paint" in the material docs. One of these, in `material/tests.rs`, is also
  missing from `89b0c79dd`, so the end tree keeps it as a fix too.
