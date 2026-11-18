# Open issues

- **`baked_cutout_tables_match_the_shaded_cutout` fails on an unchanged tree** (`palantir-showcase/tests/visual/fixtures/shadow.rs`): on a high-end NVIDIA laptop GPU under Vulkan, `cargo visual` reports "a channel moved 2 levels" between the baked cutout tables and the shaded cutout, over the test's one-level bound.
