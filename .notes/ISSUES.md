# Open issues

- The `alloc` test binary prints `ERROR: SymInitialize FAILED with code 87
  (0x57) | The parameter is incorrect.` on Windows before the first test runs.
- `cargo test --all-features` makes the test binaries open a listening socket,
  so Windows raises a firewall prompt for each one.
