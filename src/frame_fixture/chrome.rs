//! The three fixed frames around the scrolling card column: the top app
//! bar, the left nav rail, and the status bar that carries the counter
//! the `frame/partial_*` arms mutate.

use std::time::Duration;

use crate::frame_fixture::FrameFixture;
use crate::frame_fixture::tokens;
use crate::layout::types::align::Align;
use crate::layout::types::justify::Justify;
use crate::layout::types::overlay::OverlayPosition;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::brush::gradient::conic::ConicGradient;
use crate::primitives::brush::gradient::linear::LinearGradient;
use crate::primitives::brush::gradient::stops::Stop;
use crate::primitives::color::{Color, ColorU8};
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::stroke::Stroke;
use crate::scene::layer::Layer;
use crate::scene::node::Configure;
use crate::ui::Ui;
use crate::widgets::button::Button;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::separator::Separator;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;
use crate::widgets::tooltip::Tooltip;

pub(super) fn app_bar(ui: &mut Ui) {
    Panel::hstack()
        .id_salt("app-bar")
        .gap(10.0)
        .size((Sizing::FILL, Sizing::HUG))
        .child_align(Align::CENTER)
        .show(ui, |ui| {
            // Brand dot: the only conic-gradient chrome fill in the tree.
            Frame::new()
                .id_salt("brand")
                .size((Sizing::fixed(22.0), Sizing::fixed(22.0)))
                .background(Background {
                    fill: Brush::Conic(ConicGradient::new(
                        glam::Vec2::splat(0.5),
                        0.0,
                        [
                            Stop::new(0.0, ColorU8::hex(0x4cd3ff)),
                            Stop::new(0.5, ColorU8::hex(0xd897ff)),
                            Stop::new(1.0, ColorU8::hex(0x4cd3ff)),
                        ],
                    )),
                    corners: Corners::all(11.0),
                    ..Default::default()
                })
                .show(ui);
            Text::new("Palantir")
                .id_salt("title")
                .style(&TextStyle::default().with_font_size(19.0).bold())
                .show(ui);
            Text::new("frame bench")
                .id_salt("subtitle")
                .style(&tokens::caption_style())
                .show(ui);
            Frame::new()
                .id_salt("title-spacer")
                .size((Sizing::FILL, Sizing::fixed(1.0)))
                .show(ui);
            for i in 0..5 {
                let label = ui.fmt(format_args!("Action {i}"));
                let btn = Button::new()
                    .id_salt(("hdr", i))
                    .label(label)
                    .show(ui)
                    .snapshot();
                Tooltip::on(&btn)
                    .text("Header action")
                    .delay(Duration::ZERO)
                    .show(ui);
            }
            // Cascade `disabled` flattening — and a real UI state, not a
            // marker: the deploy action is unavailable until a run finishes.
            Button::new()
                .id_salt("deploy")
                .label("Deploy")
                .disabled(true)
                .show(ui);
        });
}

pub(super) fn sidebar(ui: &mut Ui, items: usize) {
    Panel::vstack()
        .id_salt("sidebar")
        .gap(6.0)
        .padding(8.0)
        .size((Sizing::fixed(216.0), Sizing::FILL))
        .background(tokens::card_bg())
        .clip_rounded()
        .show(ui, |ui| {
            Text::new("WORKSPACE")
                .id_salt("sb-title")
                .style(&tokens::section_style())
                .show(ui);
            Scroll::vertical()
                .id_salt("sidebar-scroll")
                .gap(3.0)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    for i in 0..items {
                        // Every 8th row is a group caption, so the long list
                        // is a grouped nav tree rather than N identical
                        // buttons — and the scroll viewport measures two
                        // different node shapes instead of one.
                        if i % 8 == 0 {
                            let head = ui.fmt(format_args!("GROUP {}", i / 8));
                            Text::new(head)
                                .id_salt(("side-group", i))
                                .style(&tokens::caption_style())
                                .show(ui);
                        } else {
                            let label = ui.fmt(format_args!("Sidebar item {i}"));
                            Button::new()
                                .id_salt(("side", i))
                                .label(label)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui);
                        }
                    }
                });
            Separator::horizontal().id_salt("sb-divider").show(ui);
            Panel::hstack()
                .id_salt("sb-foot-row")
                .gap(4.0)
                .justify(Justify::Center)
                .size((Sizing::FILL, Sizing::HUG))
                .show(ui, |ui| {
                    for i in 0..3 {
                        Button::new()
                            .id_salt(("sb-foot", i))
                            .label(ui.fmt(format_args!("F{i}")))
                            .show(ui);
                    }
                });
        });
}

pub(super) fn status_bar(state: &mut FrameFixture, ui: &mut Ui) {
    let bar = Panel::zstack()
        .id_salt("status")
        .size((Sizing::FILL, Sizing::fixed(34.0)))
        .show(ui, |ui| {
            Frame::new()
                .id_salt("footer-bg")
                .size((Sizing::FILL, Sizing::FILL))
                .background(Background {
                    fill: Brush::Linear(LinearGradient::two_stop(
                        0.0,
                        ColorU8::hex(0x1a1a2e),
                        ColorU8::hex(0x2a2a3e),
                    )),
                    corners: Corners::all(6.0),
                    ..Default::default()
                })
                .show(ui);
            Panel::hstack()
                .id_salt("footer-row")
                .padding(8.0)
                .gap(8.0)
                .child_align(Align::CENTER)
                .size((Sizing::FILL, Sizing::FILL))
                .show(ui, |ui| {
                    // Footer "live counter": the partial-damage arm mutates
                    // `state.tick` each iter. Fixed width pins layout so the
                    // changing digits can't shift siblings — damage collapses
                    // to this single Text node's arranged rect.
                    Text::new(ui.fmt(format_args!("Frame {:08}", state.tick)))
                        .id_salt("footer-status")
                        .style(&TextStyle::default().with_font_size(12.0))
                        .size((Sizing::fixed(120.0), Sizing::HUG))
                        .show(ui);
                    Frame::new()
                        .id_salt("footer-spacer")
                        .size((Sizing::FILL, Sizing::fixed(1.0)))
                        .show(ui);
                    Text::new("v1.2.3 · many nodes")
                        .id_salt("footer-meta")
                        .style(&tokens::caption_style())
                        .show(ui);
                });
        })
        .response
        .snapshot();

    // Toast on the Popup layer, parked above the *right* end of the status
    // bar — anchoring to the bar's full rect would drop it over the sidebar.
    // Reading the bar's rect (last frame's, hence the frame-0 fallback)
    // keeps it placed at any viewport size: the showcase page is window-sized,
    // the bench target is far taller.
    //
    // Recorded straight into the layer rather than through `Popup`, which is
    // a *modal* primitive: every `Popup::show` also records a full-surface
    // `Sense::ABSORB_POINTER` click-eater under its body, and this toast is
    // re-recorded unconditionally every frame. Standalone that was invisible
    // — nothing else was on screen to click — but the moment the fixture
    // shares a window, as the showcase page, the eater swallows every
    // pointer event bound for `Main` and the host goes dead. A toast is not
    // modal, so it does not want the eater.
    let bar_rect = bar.rect.unwrap_or(Rect::new(12.0, 12.0, 240.0, 34.0));
    const TOAST_W: f32 = 220.0;
    let anchor = Rect::new(
        bar_rect.min.x + (bar_rect.size.w - TOAST_W).max(0.0),
        bar_rect.min.y,
        TOAST_W.min(bar_rect.size.w),
        bar_rect.size.h,
    );
    ui.layer(Layer::Popup)
        .placement(OverlayPosition::above(anchor, 0.0))
        .show(|ui| {
            Panel::vstack()
                .id_salt("toast")
                .size((Sizing::HUG, Sizing::HUG))
                .background(Background {
                    fill: tokens::CARD_BG.into(),
                    stroke: Stroke::solid(tokens::BORDER, 1.0),
                    corners: Corners::all(6.0),
                    shadow: Shadow::drop(
                        Color::rgba(0.0, 0.0, 0.0, 0.55),
                        glam::Vec2::new(0.0, 3.0),
                        10.0,
                    ),
                })
                .show(ui, |ui| {
                    Text::new("Capture written to disk")
                        .id_salt("popup-label")
                        .style(&tokens::caption_style())
                        .show(ui);
                });
        });
}
