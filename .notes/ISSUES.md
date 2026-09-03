# Open issues

- `gpu_gradient_atlas.rs` states that `write_texture`'s `bytes_per_row`
  must be a multiple of `COPY_BYTES_PER_ROW_ALIGNMENT`, and asserts its
  row pitch against it. That requirement belongs to
  `copy_buffer_to_texture`, not to a queue write.
