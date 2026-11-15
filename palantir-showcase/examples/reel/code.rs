//! Slide 1's listing: the counter's code, one `Text` per token in a row, colored as an editor
//! colors it. Refused for now: a rich-text widget with colored spans, a library feature for one
//! slide.

use palantir::{
    Chrome, Configure, Corners, FontFamily, Panel, RgbaF32, SizeSpec, Sizing, Text, Ui,
};

use crate::deck::{id, id_at};

/// What a token is, by the color it takes.
#[derive(Clone, Copy, Debug)]
enum Kind {
    Keyword,
    Type,
    Call,
    Text,
    Plain,
}

impl Kind {
    const fn color(self) -> RgbaF32 {
        match self {
            Self::Keyword => RgbaF32::hex(0xc792ea),
            Self::Type => RgbaF32::hex(0xffcb6b),
            Self::Call => RgbaF32::hex(0x82aaff),
            Self::Text => RgbaF32::hex(0xc3e88d),
            Self::Plain => RgbaF32::hex(0xd6deeb),
        }
    }
}

use Kind::{Call, Keyword, Plain, Text as Str, Type};

/// The lines, each a run of tokens; the fourth is the one a click runs.
const LINES: [&[(&str, Kind)]; 6] = [
    &[
        ("fn ", Keyword),
        ("record", Call),
        ("(&", Plain),
        ("mut ", Keyword),
        ("self, ui: &", Plain),
        ("mut ", Keyword),
        ("Ui", Type),
        (") {", Plain),
    ],
    &[
        ("    Text", Type),
        ("::", Plain),
        ("new", Call),
        ("(", Plain),
        ("fmt!", Call),
        ("(ui, ", Plain),
        ("\"clicks: {}\"", Str),
        (", self.clicks)).", Plain),
        ("show", Call),
        ("(ui);", Plain),
    ],
    &[
        ("    if ", Keyword),
        ("Button", Type),
        ("::", Plain),
        ("new", Call),
        ("().", Plain),
        ("label", Call),
        ("(", Plain),
        ("\"click me\"", Str),
        (").", Plain),
        ("show", Call),
        ("(ui).", Plain),
        ("clicked", Call),
        ("() {", Plain),
    ],
    &[("        self.clicks += 1;", Plain)],
    &[("    }", Plain)],
    &[("}", Plain)],
];

/// The line a click runs.
const CLICK_LINE: usize = 3;

/// The listing, its click line lit by `flash`, 1 at a click fading to 0.
pub(crate) fn listing(ui: &mut Ui, flash: f32) {
    Panel::vstack()
        .id(id(1, "code"))
        .size((Sizing::fixed(720.0), Sizing::HUG))
        .padding(24.0)
        .gap(6.0)
        .chrome(Chrome::rounded(RgbaF32::hex(0x0b1020), Corners::all(14.0)))
        .show(ui, |ui| {
            for (n, line) in LINES.iter().enumerate() {
                let lit = if n == CLICK_LINE { flash } else { 0.0 };
                Panel::hstack()
                    .id(id_at(1, "line", n))
                    .size(SizeSpec::FILL_W)
                    .chrome(Chrome::rounded(
                        RgbaF32::hex(0x6d8cff).with_alpha(0.35 * lit),
                        Corners::all(6.0),
                    ))
                    .show(ui, |ui| {
                        for (token, kind) in *line {
                            Text::new(*token)
                                .family(FontFamily::MONO)
                                .font_size(17.0)
                                .color(kind.color())
                                .show(ui);
                        }
                    });
            }
        });
}
