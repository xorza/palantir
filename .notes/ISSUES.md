# Open issues

- The lib test binary with `--features golden,gpu-debug-markers` aborted on Linux once in six runs
  (2026-11-02, after `c61f2265f`) with glibc's `corrupted size vs. prev_size while consolidating`
  (SIGABRT), after `test result: ok` for all 2261 tests: the heap is corrupt at process exit.
