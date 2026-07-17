// Visual harness for the `frame` bench workload. Runs the same
// `build_ui` the bench measures, but inside a real window via
// `WinitHost` — so the bench scene can be eyeballed for layout /
// painting regressions that a pure timing number wouldn't catch.
//
// Run with `cargo run --example frame_visual --release`.

use aperture::{
    App, HostHandle, UVec2, Ui, WindowConfig, WindowToken, WinitHost, bench::FrameFixture,
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

fn main() {
    let window = WindowConfig::new("aperture — frame bench (visual)")
        .inner_size(UVec2::new(1280, 800))
        .min_inner_size(UVec2::new(640, 480));
    WinitHost::builder(WindowToken(0))
        .window(window)
        .build(FrameVisual::new)
        .run();
}
