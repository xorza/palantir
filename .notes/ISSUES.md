# Open issues

- A Fixed-100 vstack holding a Hug vstack with a Fixed-30 block above a Hug `Scroll::vertical()` of 8 × 50 rows arranges the scroll viewport 100 tall, below the 30 px block, so the pair takes 130 in the 100 px stack (`layout/scroll/tests.rs`, `hug_scroll_viewport_follows_parent_cap`). The stack measures each non-Fill child against its whole main extent (`layout/stack/mod.rs`, pass 1), and arrange does not shrink a Hug sibling whose floor is below its size, though the scroll's floor on its panned axis is zero.
