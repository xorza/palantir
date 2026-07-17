use aperture::{
    App, Button, Configure, HostHandle, Panel, Sizing, Text, Ui, WindowToken, WinitHost,
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
            .auto_id()
            .gap(8.0)
            .size((Sizing::Hug, Sizing::Hug))
            .show(ui, |ui| {
                Text::new(format!("clicks: {}", self.clicks))
                    .auto_id()
                    .show(ui);
                if Button::new().label("click me").show(ui).left.clicked() {
                    self.clicks += 1;
                }
            });
    }
}

fn main() {
    WinitHost::new(WindowToken(0), Counter::new)
        .title("counter")
        .run();
}
