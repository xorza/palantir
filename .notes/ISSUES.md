# Open issues

- **A `turn` paint animation rotates only polylines and curves.** Every other
  shape kind registered through `Ui::add_shape_animated` with a turn paints
  unrotated, and nothing reports it.

- **The encoded-run cache trusts the text hash.** `EncodedKey` in
  `gpu/raster/text_backend/encode/` embeds `TextShapeKey`, so two texts whose
  hashes collide share one cached glyph run, and the second draws the first's
  glyphs.
- **The per-widget text cache trusts the text hash.** `TextSystem` reuses a
  slot's measured root while `entry.key == request.key` (`text/system.rs`), so
  a widget whose new text collides with its old text keeps the old measure.
