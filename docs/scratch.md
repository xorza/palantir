- `impl WidgetId` instead of `impl Hash`
  let id_seed: u64 = self
            .id_key
            .unwrap_or_else(|| WidgetId::from_hash(("palantir.popup", self.anchor)).0);
        let eater_key = (id_seed, "eater");
        let body_key = (id_seed, "body");
        let eater_id = WidgetId::from_hash(eater_key);

- gradients, textures
- how with autostabe id work in release
-showcase agent testing
