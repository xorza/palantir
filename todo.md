transform


3. `WidgetId` hashing convenience.** No real duplication, but `WidgetId::from_hash`, `auto_stable`, `with` could grow a dedicated builder doc-block — currently scattered across 3 fns.

**4. `Color::from_srgb_u8(r, g, b, a)`.** Most literal callsites (e.g. `Color::rgb(0.20, 0.40, 0.80)`) come from RGB hex thinking. A `from_srgb_u8(51, 102, 204)` helper would feel more natural at construction; not duplication, just ergonomics.

**5. `auto_stable` macro.** All widget constructors do `Self::for_*(WidgetId::auto_stable())` / `Self::for_*(WidgetId::from_hash(id))`. Could be a one-liner via macro, but it's only 6 callsites and the explicit form is clear. Skip.
