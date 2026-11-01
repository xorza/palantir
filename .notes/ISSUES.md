# Open issues

- `Harness::capture` returns `PlayError::DeviceLost` when `Gpu::wait` times out on a device that
  is not lost: the copy does not map, and the error names a loss.
