# Open issues

- The Windows CI gate's `tests.1` binary (`--features golden,gpu-debug-markers --lib`) exits
  with `STATUS_HEAP_CORRUPTION` (0xc0000374) before any test reports, now and then: runs
  41255799750, 41268734940 and 41336748808, each passing on the next push. The log names no test.
