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

## Page by page

| Page | Change |
|---|---|
| controls | Spinners join a "Progress" row with the bar; label overflow moves to text. |
| dock | Panes hold small live views (a picker, the operations log). |
| text input | Knobs for max chars, multiline, alignment, select-all-on-focus. |
| containers | A splitter resizes the wrap and the grid. |
| clip & transform | A clip-mode radio and a padding slider on one card. |
| scroll & zoom | "Reveal row N". |
| materials | "Hold the ink" toggles the ceiling or the floor, the ratio shown. |
| icons | A cursor-icon grid that sets `CursorIcon` on hover. |
| images | Fit radios and a sampling toggle on one image. |
| motion | A curve plot per easing with a dot riding it. |
| app & windows | The shared state grows past a counter (a note and a color); a background job that wakes the UI through a `Waker`. |

## Also

- **Shots in both schemes** in the shot tests, one per page, so a light-scheme regression fails.

## Costs and risks

- **Goldens move.** The materials page compiles into the visual suite; its golden is rewritten
  after a look.
- **Each open frosted float costs a backdrop capture**; the opaque / frosted toggle compares.
- **Knobs add code to every page**; a page past about 400 lines becomes a folder.

## Decisions

- **Frosted floats are one `Theme` method.** `Theme::frost_floats(&mut self, p, sigma,
  tint_alpha)` frosts every floating slot (context menu, combo list and menu bar through
  `context_menu.panel`, `color_picker.popup`, `tooltip.panel`, `modal.panel`), each holding the
  weakest ink its recipe paints by the ceiling or floor that ink and its tint call for, as
  `Theme::scale_text` visits every text slot; the stock themes stay opaque. It takes the palette,
  as a theme keeps no muted ink of its own: a menu's shortcut and the picker's channel names are
  muted, a tooltip and the modal hold the full ink, and muted ink on the dark modal's
  `element_mid` misses 4.5:1 once the target stores it in 8 bits. Refused: a showcase helper, which each app would copy and a new floating slot would
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
- **The accent button moves away from its label's ink.** Hovered and pressed,
  `ButtonTheme::accent` moves 12 % and 24 % toward white under dark `on_accent` ink and toward
  black under light, which only raises the label's contrast. Refused: Material 3's state layer of
  `on_accent` (8 % hover), which drops the light palette's accent under 4.5:1; a move toward
  `p.text`, which lowers it under an accent lighter than the ink, as the dark palette's lime is;
  palantir's checked toggle's look, which shows no hover.
- **No density switch.** The observatory's Appearance study already edits `Metrics` live.
  Refused: compact / comfortable steps, a second demo of the same API that every shot and golden
  would depend on.

## Changed

- **Scaffold and live tokens (batch 1).** The specimen set shares one relative luminance per
  scheme rather than one perceived lightness (**Decisions**). The shared header also carries each
  page's source and its observatory studies, from the "Also" list.
- **Remove and merge (batch 2).** Two checklist items had no harness test, so palantir gained
  `a_backdrop_click_dismisses_and_reaches_nothing_below` (modal) and
  `a_click_focuses_a_control_out_of_the_tab_order` (tab); every other focus and modal item was
  already held, so the focus page and the modal carry no list. The nine items that need a real
  input method or OS close button fold under one "Check by hand (n)" disclosure per page.
- **Rail and appearance (batch 3).** The rail's grid, its items and the scheme switch live in the
  shared scaffold (`scaffold::rail`), so the observatory's rail is the same one, and the
  observatory gains `--page` from the shared shot mode. Ctrl+K starts a new search rather than
  selecting the old one, as the field's Escape first clears a selection; Enter hands focus to the
  opened item, as the field keeps Alt+arrows. The focus page's ring moved into the appearance
  with the accent, so either edit survives the other and a scheme switch.

- **Floats, grid and knobs (batches 4–8).** `frost_floats` takes the palette (**Decisions**).
  A bare `PopupTrigger` still wears no surface (`.notes/ISSUES.md`), so the showcase's popups pass
  the theme's menu panel, frosted or not; Edit ▾ is a `PopupTrigger` of `MenuItem` rows, as
  palantir has no menu button. Page names and study names are sentence case in both examples, and
  the shot scripts with them. Spacing moved onto `Metrics` on the pages whose spacing is chrome;
  the layout pages keep their literals, which are what they show. The custom danger and outlined
  button themes build on the live button theme, so they read in light. The rows above are left.

## Plan

### 6–8. Knobs left
- [ ] The rows of **Page by page** above.

### 9. Finish
- [ ] Shots in both schemes in the shot tests; README's recording note.
