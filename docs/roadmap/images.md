# Images

User-supplied raster images via [`Shape::Image`](../../src/shape.rs) +
the cross-frame [`ImageRegistry`](../../src/primitives/image.rs). Slice
1 ships the end-to-end pipeline against caller-supplied raw RGBA8;
follow-up slices fill out file/byte decoding, layout-time `fit`
modes (shipped, see slice 2), and GPU-side eviction.

## Status (2026-05)

- **Slice 1 (raw RGBA8 → on-screen):** shipped. `Image::from_rgba8(w,
  h, pixels)`, `ui.images.register(key, image)`, `Shape::Image {
  handle, local_rect, tint }`, dedicated wgpu `ImagePipeline` +
  `image.wgsl`, per-handle GPU texture cache, schedule integration via
  `RenderStep::ImageBatch`. Pinned by composer / schedule tests and a
  showcase tab (`image::build`).
- **Slice 2 (`fit` modes):** shipped — see commit log; `Shape::Image`
  carries an `ImageFit` field, layout-resolved into `local_rect` at
  paint time.
- **Slice 3 (PNG / JPEG decode):** **not started.**
- **Slice 4 (GPU LRU eviction):** shipped. `ImagePipeline` carries a
  byte budget (default `DEFAULT_IMAGE_BUDGET_BYTES` = 256 MB,
  configurable at `Host::with_text_and_image_budget`); a per-frame
  `frame_id` + `last_used_frame` stamp drives oldest-first eviction
  in `end_of_frame_evict`, called after `queue.submit`. Evicted
  handles are re-queued on the registry via `mark_pending` so the
  next sighting re-uploads from the retained `Rc<Image>`. Entries
  touched the same frame are excluded from eviction (would force
  immediate re-upload). When the touched-this-frame set alone exceeds
  budget, `tracing::error!` surfaces the overage with remediation
  hint. Policy pinned by unit tests on the pure `pick_evictions` fn.

## Slice 3 — PNG / JPEG decode

Currently users hand the framework already-decoded `Vec<u8>` of
`width * height * 4` sRGB bytes. That's enough for procedurally
generated images and tests; for real apps loading from disk or
network, decoding ought to live in the framework.

The `image` crate (with `png` feature, both already in `Cargo.toml`)
is the obvious decoder. Two new constructors on `Image`:

```rust
impl Image {
    /// Decode PNG / JPEG bytes from memory. Returns `Err` on malformed
    /// or unsupported format.
    pub fn from_encoded(bytes: &[u8]) -> Result<Self, ImageError>;

    /// Decode a file from disk. Convenience wrapper around
    /// `from_encoded` + `std::fs::read`.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ImageError>;
}
```

Decoder picks RGBA8 output regardless of source format (image crate's
`to_rgba8`). Format detection via the crate's `guess_format` so callers
don't have to pre-classify.

**Open question:** error type. `image::ImageError` is rich but pulls
the crate into the public surface. A thin `ImageError` enum we own
(`Decode { reason: &'static str }`, `Io(std::io::Error)`) keeps the
seam clean — slice 1 of the prior `Brush` migration took the same
shape (no internal crate types in the public API).

**JPEG** requires enabling the `jpeg` feature on the `image` crate.
Default-decoder JPEG is ~50% slower than PNG and pulls a non-trivial
dependency tree (zune-jpeg); deferred until a concrete use case asks
for it. PNG-only ships first.

**Cost estimate:** ~2 hrs. Single-file change to
`src/primitives/image.rs` + showcase tab update to load from
`tests/visual/fixtures/` or similar.

## Slice 4 — GPU-side LRU eviction (shipped)

See "Status" above. Open follow-up questions kept here for future
slices:

- **Per-handle pin?** A user-facing `ImageRegistry::pin(handle)` that
  excludes the handle from eviction. Useful for "always-needed" assets
  (the app icon). Default today is "no pin, all evictable."
- **Async re-upload?** Re-uploading a large texture inside a render-
  prep call hitches. A future slice could queue re-uploads onto a
  background thread (rayon? `wgpu::CommandEncoder`?) and let the
  handle paint as a transparent placeholder until ready.
- **Runtime budget setter.** A `Host::set_image_budget_bytes` was
  drafted then removed in favour of ctor-only configuration. Re-add
  if/when a host needs to retune mid-session (e.g. responding to
  device-pressure signals).

## Future work (no slice yet)

- **Mipmaps.** Large image rendered small at 1:1 minification
  produces aliasing. wgpu mip generation is straightforward (write
  level 0, then downsample levels in a compute shader or by repeated
  blit). Defer until a showcase tab visibly aliases.
- **HDR images** (linear-RGB f16 / f32 input). Would touch the
  texture format (`Rgba16Float` instead of `Rgba8UnormSrgb`),
  the `Image` payload type, and the shader's sample-then-tint math.
  Niche for UI; deferred until a concrete use case.
- **Per-image filter mode** — currently every image uses
  `Linear`/`Linear`/`Nearest` (mag / min / mipmap). Pixel-art
  rendering wants `Nearest` mag. Could surface as
  `Image { filter: ImageFilter, .. }` or as a per-handle setting at
  `register`-time.
- **Texture atlases for many small images.** UI icon sets often have
  100+ 16–32 px images. One texture per image creates N bind groups
  and N draw calls; atlasing would coalesce them. Today's
  `ImagePipeline` is the wrong abstraction for that — would warrant
  a parallel `IconAtlas` system. No demand yet.
