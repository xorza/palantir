# Profiling

Hot-path spans use the [`profiling`](https://crates.io/crates/profiling) abstraction
crate. Off by default — `profiling::scope!` / `#[profiling::function]` cost
~1 ns when no backend feature is selected.

## Tracy (recommended)

1. Install the Tracy client/server: `brew install tracy` (or build from
   [github.com/wolfpld/tracy](https://github.com/wolfpld/tracy); match the
   version the `tracy-client` crate transitively expects — check
   `cargo tree -p tracy-client`).
2. Launch the viewer (`tracy` / `Tracy.app`) — it waits for a client.
3. Run with the feature flag:

   ```sh
   cargo run --release --features profile-with-tracy --example showcase
   ```

   The client auto-connects on startup. wgpu's GPU zones light up
   automatically because `profiling` is a singleton in the dep graph.

## Puffin (no external viewer)

```sh
cargo run --release --features profile-with-puffin --example showcase
```

Then `cargo install puffin_viewer && puffin_viewer --url 127.0.0.1:8585`
— requires wiring `puffin_http::Server` in the example, not done yet.

## Instrumented passes

One scope per pass — `Ui::frame`, `Ui::post_record`, `Ui::finalize_frame`,
`LayoutEngine::run`, `Cascades::run`, `encoder::encode`, `Composer::compose`,
`WgpuBackend::submit`, `CosmicMeasure::measure`. Add finer scopes only when
a flame graph asks for one — blanket `#[profiling::function]` clutter
drowns out signal.

`Host::render` calls `profiling::finish_frame!()` on exit (after GPU
submit) so the viewer's frame markers bracket the whole record → submit
cycle, not just the recorder. If you drive `Ui::frame` directly without
a `Host` (tests, headless harnesses), call `profiling::finish_frame!()`
yourself at the equivalent boundary.
