# Caches

## Later — workload-gated

- **Cross-frame intrinsic-query cache.** Key on
  `subtree_hash + axis + req`.
- **Real-workload validation (measure cache).** Bench numbers are
  synthetic; showcase doesn't push the 400 µs ceiling.
- **Subtree-granularity encode cache.** Replay contiguous range when
  no descendant dirty; pairs with Vello-style flat stream.
- **Hit-hint propagation between caches.** Measure-cache hit implies
  encode-cache hit (same key, eviction-locked); skip encoder's
  `FxHashMap::get`.
