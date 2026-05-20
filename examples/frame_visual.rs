// Visual harness for the `frame` bench workload. Runs the same
// `build_ui` the bench measures, but inside a real window via
// `WinitHost` — so the bench scene can be eyeballed for layout /
// painting regressions that a pure timing number wouldn't catch.
//
// Run with `cargo run --example frame_visual --release`.

use palantir::{App, Ui, WinitHost, WinitHostConfig};
use winit::dpi::LogicalSize;

#[path = "../benches/support/frame_fixture.rs"]
mod fixture;

struct FrameVisual {
    state: fixture::FormState,
}

impl App for FrameVisual {
    fn frame(&mut self, ui: &mut Ui) {
        fixture::build_ui(&mut self.state, ui);
    }
}

fn main() {
    let config = WinitHostConfig {
        title: String::from("palantir — frame bench (visual)"),
        inner_size: Some(LogicalSize::new(1280, 800)),
        min_inner_size: Some(LogicalSize::new(640, 480)),
        ..WinitHostConfig::default()
    };
    WinitHost::new(
        config,
        FrameVisual {
            state: fixture::FormState::default(),
        },
    )
    .run();
}
