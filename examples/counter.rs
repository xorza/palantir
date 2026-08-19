use palantir::{
    App, Button, Configure, HostHandle, Panel, Sizing, Text, Ui, WindowToken, WinitHost,
    WinitHostError, fmt,
};

struct Counter {
    clicks: u32,
}

impl Counter {
    fn new(_ui: &mut Ui, _handle: HostHandle<Self>) -> Self {
        Counter { clicks: 0 }
    }
}

impl App for Counter {
    fn record(&mut self, _win: WindowToken, ui: &mut Ui) {
        Panel::vstack()
            .gap(8.0)
            .size((Sizing::HUG, Sizing::HUG))
            .show(ui, |ui| {
                // `fmt!` formats into the frame's text arena — no `String`.
                Text::new(fmt!(ui, "clicks: {}", self.clicks)).show(ui);
                if Button::new().label("click me").show(ui).left.clicked() {
                    self.clicks += 1;
                }
            });
    }
}

fn main() -> Result<(), WinitHostError> {
    WinitHost::builder(WindowToken(0))
        .title("counter")
        .build(Counter::new)?
        .run()
}
