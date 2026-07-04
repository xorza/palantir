// Visual harness for the `frame` bench workload. Runs the same
// `build_ui` the bench measures, but inside a real window via
// `WinitHost` — so the bench scene can be eyeballed for layout /
// painting regressions that a pure timing number wouldn't catch.
//
// Run with `cargo run --example frame_visual --release`.

use palantir::{App, HostHandle, UVec2, Ui, WindowConfig, WindowToken, WinitHost, WinitHostConfig};

#[path = "../benches/support/frame_fixture.rs"]
mod fixture;

struct FrameVisual {
    state: fixture::FormState,
}

impl FrameVisual {
    fn new(_ui: &mut Ui, _handle: HostHandle<Self>) -> Self {
        FrameVisual {
            state: fixture::FormState::default(),
        }
    }
}

impl App for FrameVisual {
    fn frame(&mut self, _win: WindowToken, ui: &mut Ui) {
        fixture::build_ui(&mut self.state, fixture::VISUAL_SCALE, ui);
    }
}

fn main() {
    let config = WinitHostConfig {
        window: WindowConfig {
            title: String::from("palantir — frame bench (visual)"),
            inner_size: Some(UVec2::new(1280, 800)),
            min_inner_size: Some(UVec2::new(640, 480)),
            ..WindowConfig::default()
        },
        ..WinitHostConfig::default()
    };
    WinitHost::new(WindowToken(0), config, FrameVisual::new).run();
}
