# Themes

- **`Metrics` carries the type ramp**, Fluent 2 by default. Refused: a size a recipe sets by hand,
  which an app's ramp cannot reach
  ([Fluent 2 typography](https://fluent2.microsoft.design/typography)).
- **The stock themes frost their floats**: one `Material` under each float's fill, held in the
  float's slot beside its panel (`material`, the modal's `panel_material` and `scrim_material`),
  which the float draws as a shape under the panel. Refused: opaque defaults, which make an app
  build its themes again to frost them; a token per look, three spellings of a choice one material
  holds; a material on a look, which every themed widget would carry for the floats alone.
- **Frosted glass holds its ink at WCAG 4.5:1**, and palantir owns the math, as it depends on its
  dither ([WCAG 1.4.3](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum)). Refused: a
  `Chrome` setter per stock material.
- **A color chip wears a button's slot**, the color filling it inside the frame's border.
