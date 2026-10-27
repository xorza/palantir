# Open issues

- **The showcase cube frames its projection to the shown part.**
  `examples/showcase/pages/gpu_view.rs` `Cube::paint` takes the aspect from
  `physical_size` and ignores `physical_full_size` and `physical_offset`, so
  the cube squashes and appears to zoom while a scroll or the window clips
  the view.
