# Scratch ideas

Unsorted; not yet triaged into a category file.

- `impl WidgetId` instead of `impl Hash`
- `Spacing` serializable nicely
- gradients, textures
- frame to accept surface
- add shapes after children?
SubRect - whaat
Multi-`Shape::Text` per leaf is unsupported


 fix  pub(crate) fn compute(&mut self, tree: &Tree) {
        self.compute_per_node(tree);
        self.compute_subtree_rollup(tree);
    }

soa on tree

## Considered, deferred

- **Tailwind-style chained styling DSL** (à la GPUI's
  `div().flex().gap_2().bg(...)`). Palantir already gets terse call
  sites from `Configure` + `Styled` + immediate-mode authoring; a
  separate styling DSL would duplicate surface without saving
  characters. Park unless a real authoring complaint shows up.
