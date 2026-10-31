# Showcase refresh — proposal

The showcase stays the widget reference; the observatory shows the widgets in use. This proposal
removes what the tests or other pages already hold, puts every page on one grid and one scaffold,
makes it follow the scheme, floats its popups on frosted glass, and turns static specimens into
ones a reader drives.

## What the pages show today

Every page shot at 1280 × 800, dark, and the popups shot open.

- **The rail shows half the tour.** At 800 px tall it lists 14 of 27 pages; the debug footer takes
  the rest. Page names start at x 24, checkbox labels at 48, the UI-scale stepper is centered. The
  overlay scrollbar sits on the selection pill at x 188.
- **One look only.** `shell::pin_dark` holds the scheme and the scaffold reads 134 constants of
  `Palette::DARK`; pages add 25 `RgbaF32::WHITE` inks and 62 hex and sRGB literals, which a light ground
  would lose.
- **No spacing grid.** 57 literal gaps in 9 values and 31 literal paddings in 16 shapes, off the
  4 px grid at 5, 6, 10, 14, 18, 20 and 28; 18 literal corner radii in 8 values beside `M.radius`.
- **Four surface treatments.** A specimen sits bare on the page (button themes, tooltips), in a
  ground-colored well, on `raised_chrome` (context-menu surfaces) or in a tile. Captions sit above
  tiles at 30 px fixed height.
- **Popups are flat, one has no surface.** Menus, combo list, picker, tooltip and modal are opaque
  `element` fill. The focus page's Filters ▾ popup sets no chrome, and the stock `panel_chrome` is
  `None`, so its checkboxes draw straight over the checklist under it. The overlays page's Edit ▾
  popup holds three filled buttons, not menu rows.
- **The modal says nothing.** "Delete all the things?" with two equal buttons, no title, no body,
  no destructive look.
- **Neon specimens.** Swatches `A`–`E` (`#4cd3ff`, `#ffa63d`, `#d9ff57`, `#d897ff`, `#ff5e44`),
  108 uses, read as a test card and have no light set; scroll & split and pan & zoom paint whole
  rainbows of them.
- **Static specimens.** Five justify rows, three gap rows, three visibility rows, fixed gradient
  tiles; "resize the window" is the only knob on text and containers.
- **Things that look broken on purpose.** A 3 px teal separator inside the settings form, an empty
  progress bar under it at rest, a label spilling out of its button, a `danger disabled` that reads
  as another red.
- **Hand checklists.** 11 lists, 39 items (focus 28, text input 6, dialogs 5), between the demos.
- **Bench content in the tour.** The pan & zoom "heavy document" with auto-drive, the 2 500-vertex
  stress mesh and the fixtures page exist for measuring or regression, not for a reader.
- **Two scaffolds.** The observatory's `Tokens` already read the live palette; the showcase's
  `support` does not. Both copy `api!`, and their chips, card radius and mono size differ.
- **Never settles.** The gpu view requests a repaint every frame and clears to a fixed near-black.
- **Gaps.** `EditableLabel`, `Clipboard`, `CursorIcon` and `Waker` appear on no page.

## Remove

- **The fixtures page.** The golden suite holds blending, corners and occlusion
  (`tests/visual/fixtures/{fade,corners,occlusion}.rs`) and `src/ui/tests/ids.rs` the id
  collisions; a reader learns nothing from deliberately ugly content.
- **pan & zoom as a page.** The zoomable viewport joins scroll & split as its two-axis pane; the
  heavy document and auto-drive go, as the frame bench is the measured workload. The observatory's
  sky chart lists the page, so its list follows.
- **dialogs as a page.** Its modal joins overlays; window-close interception joins the windows
  page, where it is a window's concern.
- **The settings form's "Live state" well**, which repeats what the form shows, and its tinted
  separator.
- **The stress mesh and the 40-row rainbows**, kept only where they show a behavior.
- **Checklist items a harness test can hold**: each moves into a test or, already held, goes. What
  needs a real OS (IME, the OS close button) stays in one folded list per page.
- **The rail's debug footer**, into a Diagnostics popover.

27 pages become 24: 7 widgets, 2 input, 5 layout, 7 paint, 3 runtime and diagnostics.

## Unify

- **Spacing from `Metrics` only.** Every gap, padding and radius reads `M.space`, `M.radius` or a
  named scaffold constant on the 4 px grid; a literal stays only as a specimen's subject (the gap
  page's 0, 8, 24).
- **One page anatomy.** A section is a title row, a note held to 680 px, one stage, then the
  stage's knobs. Widgets stand on a stage, paint specimens in tiles with captions under them;
  nothing sits bare on the page but prose and knobs.
- **Sentence case** for page names and titles ("Focus & keyboard"), as Fluent writes UI text
  ([Fluent 2 writing](https://fluent2.microsoft.design/content-design)); the rail's group headings
  stay small caps.

## Look

- **Rail.** A search field heads it (Ctrl+K), filtering labels and blurbs as Storybook's sidebar
  does. 28 px rows on one 12 px inset, a rounded pill inset from both edges, the scrollbar outside
  the pill ([Apple HIG sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars)).
  The open page scrolls into view when it changes from the keyboard. The footer holds Appearance
  and a Diagnostics button.
- **Frosted floating surfaces** through `Theme::frost_floats` (**Decisions**).
- **The modal reads as a dialog.** A title, a body, buttons at the end with the confirming one
  last in the accent look, over the stock dim scrim.
- **Buttons by weight.** Apply, Clear and Delete wear `ButtonTheme::accent`; the danger theme
  stays a custom-theme specimen on the controls page.
- **The page content** keeps a 1 040 px column, so a wide window does not leave stages on the left
  and prose stretched to the right.

## Theme, live

- **Appearance in the rail's footer:** System / Light / Dark, an accent row of seed hues, and
  the UI scale. The showcase owns one appearance and applies it to both schemes, so a focus-ring or
  accent edit survives a scheme switch; today the focus page writes the shown scheme's theme only.
- **colors: "Use as accent"** sends the picked color to the appearance as a seed.
- **The gpu view clears to transparent**, so the stage shows through and the page proves its
  compositing claim in both schemes.

## Page by page

| Page | Change |
|---|---|
| controls | The form's Appearance radios drive the real scheme; Reduce motion reads `is_motion_reduced`. Apply shows the progress only while it syncs. Spinners join a "Progress" row with the bar. Label overflow moves to text. |
| colors | "Use as accent"; panel defaults from the specimen set. |
| tabs | Chips reorder by drag and close; a "+" adds one; an operations log beside it. |
| dock | Panes hold small live views (a picker, an operations log, the pane's size); Reset layout. |
| custom widget | Range and step knobs; the call-site listing follows them. |
| overlays | Gains the modal. Edit ▾ holds `MenuItem` rows. An opaque / frosted toggle. Tooltips become one row with a delay slider and a max-width slider. |
| text input | Gains `EditableLabel` and a clipboard copy / paste pair; knobs for max chars, multiline, alignment, select-all-on-focus. |
| focus & keyboard | Ring width and color edit the appearance; checklists fold. |
| sizing & spacing | One playground: three children, each with a `Sizing` combo, a weight and an align; padding, gap and justify on the parent. The fixed rows stay below as the reference. |
| containers | Gap and line-gap sliders; a splitter resizes the wrap and the grid. |
| text | A width slider replaces "resize window"; a font-size slider; label overflow. |
| clip & transform | Translate and scale sliders (a `TranslateScale` has no rotation), a clip-mode radio, a padding slider. |
| scroll & zoom | Vertical list, horizontal strip, zoomable two-axis pane; "reveal row N"; zoom readout and Reset view. |
| shapes | Corner radius and stroke width knobs on the triangle; the windowed rect over the stage. |
| strokes | One live polyline: width, join and cap. Hairlines in the ink, not white. |
| gradients | A stop editor: two color chips, an angle slider, an interpolation radio on a live tile. |
| shadows | Offset, blur and spread sliders on one card; the stage follows the scheme. |
| materials | Blur and tint sliders; "hold the ink" toggles the ceiling or floor, the ratio shown. |
| icons | A size slider and a tint chip; a cursor-icon grid that sets `CursorIcon` on hover. |
| images | Fit radios and a sampling toggle on one image. |
| motion | A curve plot per easing with a dot riding it; a duration slider; Replay; a reduced-motion line. |
| gpu view | A Spin switch, off in the shot mode; transparent clear. |
| app & windows | The shared state grows past a counter (a note and a color); window-close interception; a background job that wakes the UI through a `Waker`. |
| frame bench | Unchanged. |

## Also

- **`showcase --page NAME`** opens a page in the window mode, as the shot header does.
- **Alt+↑ / Alt+↓** step through the pages from anywhere, as Storybook binds them.
- **Shots in both schemes** in the shot tests, one per page, so a light-scheme regression fails.

## Costs and risks

- **Goldens move.** The materials page compiles into the visual suite; its golden is rewritten
  after a look.
- **Each open frosted float costs a backdrop capture**; the opaque / frosted toggle compares.
- **Knobs add code to every page**; a page past about 400 lines becomes a folder.

## Decisions

- **Frosted floats are one `Theme` method.** `Theme::frost_floats(&mut self, sigma, tint_alpha)`
  frosts every floating slot (context menu, combo list and menu bar through `context_menu.panel`,
  `color_picker.popup`, `tooltip.panel`, `modal.panel`), each holding its ink by the ceiling or
  floor its ink and tint call for, as `Theme::scale_text` visits every text slot; the stock themes
  stay opaque. Refused: a showcase helper, which each app would copy and a new floating slot would
  silently miss. WinUI and macOS likewise make it one material: MenuFlyout, ComboBox and ToolTip
  wear acrylic ([Fluent acrylic](https://learn.microsoft.com/windows/apps/design/style/acrylic)),
  NSMenu and NSPopover vibrancy.
- **The modal keeps a dim scrim.** `ModalTheme::backdrop` stays a color; the frosted panel shows
  the page through itself. Refused: a scrim as a `Chrome` and a blurred stock scrim, a web pattern
  that costs a full-window capture; Fluent's ContentDialog smoke layer, Material 3's 32 % scrim,
  Apple's alerts and libadwaita's `AdwDialog` all dim.
- **`ButtonTheme::accent(p, m)` beside `menu_button`**, filled with the accent under `on_accent`
  ink; darkroom's `palantir_bridge::primary_button` becomes a call to it. Refused: a danger button
  and a `Palette` danger role, which Fluent 2's button appearances, Material 3 and Apple's HIG do
  not ship (Apple marks a destructive role with red text; GTK alone has `.destructive-action`);
  both in the showcase, which leaves darkroom's copy.
- **An accent is a seed hue, solved per scheme.** The showcase keeps a color's hue and chroma and
  solves its lightness in each scheme to WCAG 4.5:1 as text on the ground and against `on_accent`
  and 3:1 as the ring, as Material 3 derives tones from one seed. Refused: presets alone (GNOME
  47, macOS), which drop "Use as accent"; a color as given with its failed floors shown (Windows'
  custom accent), which can leave the tour unreadable.
- **Specimens share one luminance per scheme.** Five hues at one Okhsv saturation, each value
  bisected to the scheme's relative luminance (dark 0.35, light 0.25): WCAG reads contrast in
  luminance, so every swatch meets the ground and the swatch ink alike, and Okhsv keeps the hue
  and the gamut as the value moves. Refused: one Oklab lightness, which palantir keeps private and
  which leaves the contrast per hue; hand-picked hex, which a palette change would leave behind.
- **Pages take the kit as a parameter**, as the observatory's studies take `Tokens`. Refused: the
  kit in a state row or a static, which hides what a page paints with; `TextStyle::default`,
  whose ink is the stock dark one in either scheme.
- **No density switch.** The observatory's Appearance study already edits `Metrics` live.
  Refused: compact / comfortable steps, a second demo of the same API that every shot and golden
  would depend on.

## Changed

- **Scaffold and live tokens (batch 1).** The specimen set shares one relative luminance per
  scheme rather than one perceived lightness (**Decisions**). The shared header also carries each
  page's source and its observatory studies, from the "Also" list.

## Plan

### 2. Remove and merge
- [ ] Delete fixtures; merge pan & zoom into scroll & zoom; split dialogs into overlays and
      windows; drop Live state, the tinted separator, the stress mesh, the rainbows.
- [ ] Checklist items into harness tests or deleted; the rest folded.

### 3. Rail and appearance
- [ ] Inset grid, 28 px rows, pill, scrollbar outside it; reveal on change.
- [ ] Search (Ctrl+K), Alt+↑/↓, `--page`.
- [ ] Appearance footer (scheme, seed accent solved per scheme, UI scale) and Diagnostics
      popover.

### 4. Floating surfaces
- [ ] palantir: `Theme::frost_floats` and `ButtonTheme::accent`, tested; darkroom's
      `primary_button` calls it; API surface regenerated.
- [ ] Frosted menus, combo list, picker, tooltip, modal; the toggle.
- [ ] Every popup has a surface; Edit ▾ holds menu rows; the modal as a dialog.

### 5. Grid and anatomy
- [ ] Spacing and radii from `Metrics`; sentence case; stages; captions under tiles;
      1 040 px column.

### 6. Knobs: widgets and input
- [ ] controls, colors, tabs, dock, custom widget, overlays, text input, focus.

### 7. Knobs: layout
- [ ] sizing & spacing, containers, text, clip & transform, scroll & zoom.

### 8. Knobs: paint and runtime
- [ ] shapes, strokes, gradients, shadows, materials, icons, images, motion, gpu view, app &
      windows.

### 9. Finish
- [ ] Shots in both schemes; goldens rewritten after a look; README.
