# Open issues

- A partial repaint and a full repaint of the same frame can draw a shadow differently: `CutoutPlan` picks tables, and so the shaded cutout or `fs_shadow_tables`, from the drawn area, which the damage changes. The two forms round apart by one 8-bit level, against the damage oracle's bit-for-bit contract (`tests/visual/fixtures/damage_oracle.rs`), whose scenes hold no shadow.
