# Open issues
- `src/layout/engine.rs` container-text pass and `src/layout/pass.rs` measure both read a node's own `Visibility`, not the cascaded one, so every container under a `Hidden` ancestor still shapes its paint-only run.
