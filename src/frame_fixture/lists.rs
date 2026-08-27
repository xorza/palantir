//! The repeating-content cards. Between them they cover both `Scroll`
//! axes and both `WrapStack` orientations, and they are what `scale`
//! actually grows — the bulky tail of the card column.

use crate::frame_fixture::tokens;
use crate::layout::types::align::Align;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::brush::Brush;
use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
use crate::primitives::brush::gradient::radial_geometry::RadialGradient;
use crate::primitives::color::ColorU8;
use crate::primitives::corners::Corners;
use crate::scene::node::Configure;
use crate::text::wrap::TextWrap;
use crate::ui::Ui;
use crate::widgets::button::Button;
use crate::widgets::frame::Frame;
use crate::widgets::panel::Panel;
use crate::widgets::scroll::Scroll;
use crate::widgets::separator::Separator;
use crate::widgets::text::Text;
use crate::widgets::theme::text_style::TextStyle;

/// Horizontally-scrolled strip — the fixture's only `Scroll::horizontal`,
/// so both scroll axes are exercised in one tree.
pub(super) fn filmstrip(ui: &mut Ui, cells: usize) {
    tokens::card(ui, "recent", "RECENT CAPTURES", Sizing::HUG, |ui| {
        Scroll::horizontal()
            .id_salt("film-scroll")
            .gap(8.0)
            .padding(6.0)
            .size((Sizing::FILL, Sizing::fixed(74.0)))
            .background(tokens::well_bg())
            .show(ui, |ui| {
                for i in 0..cells {
                    Panel::vstack()
                        .id_salt(("film", i))
                        .gap(3.0)
                        .size((Sizing::fixed(84.0), Sizing::FILL))
                        .show(ui, |ui| {
                            Frame::new()
                                .id_salt(("film-thumb", i))
                                .size((Sizing::FILL, Sizing::FILL))
                                .background(Background {
                                    fill: Brush::Linear(LinearGradient::two_stop(
                                        0.9,
                                        ColorU8::hex(0x24304d),
                                        ColorU8::hex(0x3d2a52),
                                    )),
                                    corners: Corners::all(5.0),
                                    ..Default::default()
                                })
                                .show(ui);
                            Text::new(ui.fmt(format_args!("shot_{i:03}")))
                                .id_salt(("film-cap", i))
                                .style(&tokens::caption_style())
                                .show(ui);
                        });
                }
            });
    });
}

/// Fixed height, not `Fill`: this card lives inside the page scroll, whose
/// main axis is unbounded, so a `Fill` height would resolve against nothing
/// and the inner scroll would grow to its full content instead of paging.
pub(super) fn activity_card(ui: &mut Ui, messages: usize) {
    tokens::card(ui, "activity", "ACTIVITY", Sizing::fixed(268.0), |ui| {
        Scroll::vertical()
            .id_salt("chat-scroll")
            .gap(8.0)
            .padding(4.0)
            .size((Sizing::FILL, Sizing::FILL))
            .show(ui, |ui| {
                for i in 0..messages {
                    Panel::hstack()
                        .id_salt(("chat-row", i))
                        .gap(8.0)
                        .size((Sizing::FILL, Sizing::HUG))
                        .show(ui, |ui| {
                            Frame::new()
                                .id_salt(("avatar", i))
                                .size((Sizing::fixed(34.0), Sizing::fixed(34.0)))
                                .background(Background {
                                    fill: Brush::Radial(RadialGradient::two_stop_centered(
                                        ColorU8::hex(0xfacc15),
                                        ColorU8::hex(0x4c5cdb),
                                    )),
                                    corners: Corners::all(17.0),
                                    ..Default::default()
                                })
                                .show(ui);
                            Panel::vstack()
                                .id_salt(("chat-text", i))
                                .gap(2.0)
                                .size((Sizing::FILL, Sizing::HUG))
                                .show(ui, |ui| {
                                    Panel::hstack()
                                        .id_salt(("chat-meta", i))
                                        .gap(6.0)
                                        .child_align(Align::CENTER)
                                        .size((Sizing::FILL, Sizing::HUG))
                                        .show(ui, |ui| {
                                            let name = ui.fmt(format_args!("user_{i}"));
                                            Text::new(name)
                                                .id_salt(("from", i))
                                                .style(
                                                    &TextStyle::default()
                                                        .with_font_size(12.0)
                                                        .bold(),
                                                )
                                                .show(ui);
                                            let at = ui.fmt(format_args!("{:02}:{:02}", i % 24, i));
                                            Text::new(at)
                                                .id_salt(("at", i))
                                                .style(&tokens::caption_style())
                                                .show(ui);
                                        });
                                    Text::new(
                                        "Longer body that should wrap inside the Fill \
                                         column without breaking words inside any single \
                                         token.",
                                    )
                                    .id_salt(("msg", i))
                                    .style(&tokens::body_style())
                                    .text_wrap(TextWrap::Wrap)
                                    .size((Sizing::FILL, Sizing::HUG))
                                    .show(ui);
                                });
                        });
                }
            });
    });
}

/// Tag chips beside a badge column — the two `WrapStack` orientations
/// side by side, each wrapping against a different bounded axis.
pub(super) fn tags_card(ui: &mut Ui, tags: usize, badges: usize) {
    tokens::card(ui, "tags", "LABELS", Sizing::HUG, |ui| {
        Panel::hstack()
            .id_salt("tag-row")
            .gap(10.0)
            .size((Sizing::FILL, Sizing::HUG))
            .show(ui, |ui| {
                Panel::wrap_hstack()
                    .id_salt("tag-wrap")
                    .gap(4.0)
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui, |ui| {
                        for i in 0..tags {
                            let label = ui.fmt(format_args!("#tag{i}"));
                            Button::new().id_salt(("tag", i)).label(label).show(ui);
                        }
                    });
                Separator::vertical().id_salt("tag-vsep").show(ui);
                // Wraps against its bounded *height*, so it fills column by
                // column. Each badge is a fixed-width chip: without one the
                // columns pack to their own text width and adjacent labels
                // read as a single run.
                Panel::wrap_vstack()
                    .id_salt("badge-wrap")
                    .gap(6.0)
                    .padding(6.0)
                    .size((Sizing::HUG, Sizing::fixed(96.0)))
                    .background(tokens::well_bg())
                    .show(ui, |ui| {
                        for i in 0..badges {
                            Text::new(ui.fmt(format_args!("badge {i}")))
                                .id_salt(("badge", i))
                                .style(&tokens::caption_style())
                                .size((Sizing::fixed(64.0), Sizing::HUG))
                                .show(ui);
                        }
                    });
            });
    });
}
