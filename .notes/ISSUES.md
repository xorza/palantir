# Open issues

- `src/common/expiry_wheel/mod.rs:113` says the shaped-buffer cache ring has 128 buckets. `with_keep(120 + 15)` gives 256.
- `src/layout/cache/bench.rs` `build_broad_variant`: changing one leaf's height from 1 px to 2 px leaves zero measure-cache hits in the whole tree on the next frame, where its 21 unchanged sibling subtrees each have an unchanged subtree hash.
