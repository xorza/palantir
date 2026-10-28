# Open issues

- A `Shadow` `blur` or `spread` above 65504 logical px packs into an infinite f16 lane in
  `LoweredShadow`, so `ShadowGeometry::halo` and the drop quad's rect are infinite. `Shadow`
  validates them as a length and an offset, which allow such values.
