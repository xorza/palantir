//! The metric strip under the app bar — the one node group that carries
//! all four `Brush` variants as chrome fills at once.

use crate::frame_fixture::tokens;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::brush::gradient::conic_geometry::ConicGradient;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::color::{RgbaF32, RgbaU8};
use crate::primitives::corners::Corners;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::scene::visibility::Visibility;
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

/// Four metric tiles. Each is a ZStack — gradient plate under a text
/// column — so the strip carries all four `Brush` variants at once.
pub(super) fn show(ui: &mut Ui) {
    const STATS: [(&str, &str, &str); 4] = [
        ("FRAMES", "148 920", "+12.4%"),
        ("NODES", "8 412", "+1.8%"),
        ("DRAW CALLS", "36", "-4"),
        ("GPU", "2.81 ms", "steady"),
    ];
    const DELTA: [RgbaF32; 4] = [tokens::OK, tokens::ACCENT, tokens::OK, tokens::TEXT_DIM];

    Panel::hstack()
        .id_salt("stats")
        .gap(10.0)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| {
            for (i, (label, value, delta)) in STATS.iter().enumerate() {
                let fill = match i {
                    0 => Brush::Linear(LinearGradient::two_stop(
                        0.6,
                        RgbaU8::hex(0x1d2440),
                        RgbaU8::hex(0x2b3a63),
                    )),
                    1 => Brush::Radial(RadialGradient::two_stop_centered(
                        RgbaU8::hex(0x2a2350),
                        RgbaU8::hex(0x171a2b),
                    )),
                    2 => Brush::Conic(ConicGradient::new(
                        glam::Vec2::new(0.15, 0.9),
                        0.0,
                        [
                            Stop::new(0.0, RgbaU8::hex(0x1b2b2e)),
                            Stop::new(0.55, RgbaU8::hex(0x24404a)),
                            Stop::new(1.0, RgbaU8::hex(0x1b2b2e)),
                        ],
                    )),
                    _ => Brush::Solid(RgbaF32::hex(0x232734)),
                };
                Panel::zstack()
                    .id_salt(("stat", i))
                    .size((Sizing::fill(1.0), Sizing::fixed(74.0)))
                    .show(ui, |ui| {
                        Frame::new()
                            .id_salt(("stat-plate", i))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                fill,
                                stroke: Stroke::solid(tokens::BORDER, 1.0),
                                corners: Corners::all(8.0),
                                shadow: Shadow::drop(
                                    RgbaF32::srgba(0.0, 0.0, 0.0, 0.45),
                                    glam::Vec2::new(0.0, 2.0),
                                    8.0,
                                ),
                            })
                            .show(ui);
                        // Cascade `Hidden` flattening — the alert ring this
                        // tile would show on a threshold breach. A ZStack
                        // sibling, so reserving its box costs no layout.
                        Frame::new()
                            .id_salt(("stat-alert", i))
                            .size((Sizing::FILL, Sizing::FILL))
                            .background(Background {
                                stroke: Stroke::solid(tokens::WARN, 2.0),
                                corners: Corners::all(8.0),
                                ..Default::default()
                            })
                            .visibility(Visibility::Hidden)
                            .show(ui);
                        Panel::vstack()
                            .id_salt(("stat-text", i))
                            .gap(2.0)
                            .padding(10.0)
                            .size((Sizing::FILL, Sizing::FILL))
                            .show(ui, |ui| {
                                Text::new(*label)
                                    .id_salt(("stat-l", i))
                                    .style(&tokens::caption_style())
                                    .show(ui);
                                Text::new(*value)
                                    .id_salt(("stat-v", i))
                                    .style(&TextStyle::default().with_font_size(22.0).bold())
                                    .show(ui);
                                Text::new(*delta)
                                    .id_salt(("stat-d", i))
                                    .style(
                                        &TextStyle::default()
                                            .with_font_size(11.0)
                                            .with_color(DELTA[i]),
                                    )
                                    .show(ui);
                            });
                    });
            }
        });
}
