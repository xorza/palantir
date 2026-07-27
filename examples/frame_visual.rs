// Visual harness for the `frame` bench workload. Runs the same
// `build_ui` the bench measures, but inside a real window via
// `WinitHost` — so the bench scene can be eyeballed for layout /
// painting regressions that a pure timing number wouldn't catch.
//
// Run with `cargo run --example frame_visual --release`.

use palantir::{
    App, HostHandle, UVec2, Ui, Vsync, WindowConfig, WindowToken, WinitHost, WinitHostError,
    bench::FrameFixture,
};

#[derive(Debug)]
struct FrameVisual {
    state: FrameFixture,
}

impl FrameVisual {
    fn new(_ui: &mut Ui, _handle: HostHandle<Self>) -> Self {
        FrameVisual {
            state: FrameFixture::default(),
        }
    }
}

impl App for FrameVisual {
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        self.state.render(6, ui);
    }
}

fn main() -> Result<(), WinitHostError> {
    let window = WindowConfig::new("palantir — frame bench (visual)")
        .inner_size(UVec2::new(1280, 800))
        .min_inner_size(UVec2::new(640, 480));
    WinitHost::builder(WindowToken(0))
        .window(window)
        // Unthrottled, like the bench this mirrors: a frame presents as soon
        // as it is ready rather than waiting for the next vblank.
        .vsync(Vsync::Off)
        .build(FrameVisual::new)?
        .run()
}
