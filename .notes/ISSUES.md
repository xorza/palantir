# Open issues

- Golden fixture tests (`cargo test --tests --all-features`, the `fixtures::*` binary) failed once in bulk — about 18 tests across expander, gradient, hidpi, layout, scroll and shapes — and then passed on four later runs with no change that touches rendering.
