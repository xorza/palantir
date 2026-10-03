# Open issues

- `cargo doc --document-private-items` fails: the doc on `src/text/cosmic/glyph_ink.rs:19` links `Self::outsets`, which does not resolve.
- `cargo doc --document-private-items` fails: `src/layout/layout_scratch.rs:113` gives `Measured` an explicit link target that is already in scope (`redundant_explicit_links`).
