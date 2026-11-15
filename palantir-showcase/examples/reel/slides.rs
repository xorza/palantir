//! Each slide's choreography, in storyboard order.

use std::time::Duration;

use palantir::glam::Vec2;
use palantir::{AnimationSpec, ColorScheme, Easing, Theme, Ui};

use crate::deck::{self, Deck, id};
use crate::script::{Step, Target};

/// A camera glide from one slide to the next: slow, mostly a pan.
const GLIDE: Duration = Duration::from_millis(2400);

/// The theme morph's transition: slower than an app's, so a viewer follows each value move.
const MORPH: AnimationSpec =
    AnimationSpec::duration(Duration::from_millis(1400), Easing::InOutCubic);

const fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

const fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

/// Slide `k`'s steps, the camera's glide onto it first.
pub(crate) fn steps(k: usize) -> Vec<Step> {
    let mut steps = Vec::new();
    if k > 0 && k != 11 {
        steps.push(Step::Glide {
            to: deck::slide_view(k),
            length: GLIDE,
        });
    }
    let w = |name| Target::Widget(id(k, name));
    let at = |name, x: f32, y: f32| Target::WidgetAt(id(k, name), Vec2::new(x, y));
    match k {
        0 => steps.extend([Step::Hold(secs(4))]),
        1 => steps.extend([
            Step::Hold(ms(600)),
            Step::Move(w("button")),
            Step::Click,
            Step::Hold(ms(900)),
            Step::Move(at("button", 0.6, 0.55)),
            Step::Click,
            Step::Hold(secs(1)),
        ]),
        2 => steps.extend([Step::Hold(secs(7))]),
        3 => steps.extend([
            Step::Move(at("switch", 0.06, 0.5)),
            Step::Click,
            Step::Move(at("slider", 0.25, 0.5)),
            Step::Drag(at("slider", 0.8, 0.5)),
            Step::Move(w("combo")),
            Step::Click,
            Step::Move(at("combo", 0.5, 3.2)),
            Step::Click,
            Step::Move(w("edit")),
            Step::Click,
            Step::Type("Hello, palantir"),
            Step::Hold(secs(1)),
        ]),
        4 => steps.extend([
            Step::Hold(ms(800)),
            Step::Act(|_, ui| morph(ui, deck::roomy_theme())),
            Step::Hold(ms(2600)),
            Step::Act(|_, ui| morph(ui, Theme::stock(ColorScheme::Dark))),
            Step::Hold(ms(2000)),
        ]),
        5 => steps.extend([Step::Hold(secs(13))]),
        6 => steps.extend([Step::Hold(secs(9))]),
        7 => steps.extend([
            Step::Move(at("host", 0.105, 0.035)),
            Step::Drag(at("host", 0.93, 0.55)),
            Step::Hold(secs(2)),
        ]),
        8 => steps.extend([Step::Hold(secs(6))]),
        9 => steps.extend([Step::Hold(secs(5))]),
        10 => steps.extend([Step::Hold(secs(4))]),
        11 => steps.extend([
            Step::Glide {
                to: deck::canvas_view(),
                length: secs(4),
            },
            Step::Hold(secs(3)),
            Step::Glide {
                to: deck::slide_view(12),
                length: secs(3),
            },
        ]),
        _ => steps.extend([Step::Hold(secs(5))]),
    }
    steps
}

/// Blend the window's theme to `theme` under [`MORPH`].
fn morph(ui: &mut Ui, theme: Theme) {
    ui.set_scheme_theme(
        ColorScheme::Dark,
        Theme {
            transition: MORPH,
            ..theme
        },
    )
    .unwrap();
}

/// Mark `deck`'s slide as started at its time; a slide's own pictures run from here.
pub(crate) const fn start(deck: &mut Deck) {
    deck.slide_start = deck.now;
}
