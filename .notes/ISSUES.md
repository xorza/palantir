# Open issues

- `user_frames_keeps_palantir_src_and_excludes_harness_internals` (`tests/alloc/harness/tests.rs`) failed once on the Pi 5 with "at least one kept frame": no frame of the rendered backtrace, the full-stack fallback included, carried a file name. About 40 later runs of the same suite passed, among them runs straight after a rebuild of the library and of the test binary.
