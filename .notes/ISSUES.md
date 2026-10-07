# Open issues

- A partial repaint and a full repaint of the same frame can draw a shadow differently: `CutoutPlan` picks tables, and so the shaded cutout or `fs_shadow_tables`, from the drawn area, which the damage changes. The two forms round apart by one 8-bit level, against the damage oracle's bit-for-bit contract (`tests/visual/fixtures/damage_oracle.rs`), whose scenes hold no shadow.
- On the Pi 5's V3D 7.1 (`V3DV Mesa 26.2.2`), `fixtures::image::bilinear_both_nearest_and_tiled_sampling_paths_are_pinned` fails on every run: the both-nearest second seam-right pixel reads `[230, 60, 62, 255]` against `[230, 60, 60, 255]`, two steps off where one is allowed.
