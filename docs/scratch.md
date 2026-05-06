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
