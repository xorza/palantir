- widget-authoring surface
- custom panels
- wgpu features and limits request public api
- mesh once registrations
- flexbox
- mcp
- reduced motion/backdrops theme options selection
- organize ui mod.rs
- scrim: Option<RgbaF32> - to backdrop? for blur, etc?

- table widget: sortable header, virtual rows over a fixed row height
- reorderable list: a drag handle and Alt+arrows
- hover card at a point for content that is not a widget
- subtree theme, as Flutter's `Theme` widget
- reel music track: a calm Pixabay one at 80 BPM, its license read for the use

- layout driver tests on `internals::layout_rig` rather than `Harness` and widgets, so no widget default moves a layout expectation; scrollbars has moved
- `ExpiryWheel::grow_buckets` grows every bucket at once, so no bucket allocates after warmup: one burst of N inserts keeps 256 × N × 32 B for the session (N = 4000: 32 MiB, twice, for the shaped and encoded caches); decide between that RAM and one bucket doubling in a later frame
