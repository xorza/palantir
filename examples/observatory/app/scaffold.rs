//! The example's look outside palantir's widgets: tokens read from the live palette, the type ramp,
//! chips, and each study's header.

use palantir::{
    Chrome, Configure, Corners, FontFamily, FontWeight, Palette, Panel, RgbaF32, Sizing, Stroke,
    Text, TextStyle, TextWrap, Ui,
};

/// The checked spelling of an API a study shows: expands to the path as text after the compiler
/// resolves it, so a rename breaks the build. `api!(Ui::animate)` for a value path,
/// `api!(Ui::f as fn(&mut Ui, ..))` for a generic function, `api!(type GpuView)` for a type.
macro_rules! api {
    ($head:ident $(:: $tail:ident)* as $sig:ty) => {{
        let _: $sig = $head $(:: $tail)*;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
    (type $head:ident $(:: $tail:ident)*) => {{
        #[expect(unused_imports, reason = "the import is the check")]
        use $head $(:: $tail)* as _;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
    ($head:ident $(:: $tail:ident)*) => {{
        let _ = $head $(:: $tail)*;
        concat!(stringify!($head) $(, "::", stringify!($tail))*)
    }};
}
pub(crate) use api;

/// The scaffold's colors this frame: roles of the palette the shown scheme wears, so a scheme
/// switch, a preset or an edit restyles the scaffold with palantir's widgets.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tokens {
    pub(crate) ground: RgbaF32,
    pub(crate) surround: RgbaF32,
    pub(crate) raised: RgbaF32,
    pub(crate) border: RgbaF32,
    pub(crate) hairline: RgbaF32,
    pub(crate) ink: RgbaF32,
    pub(crate) ink_dim: RgbaF32,
    pub(crate) ink_faint: RgbaF32,
    pub(crate) accent: RgbaF32,
}

impl Tokens {
    pub(crate) const fn of(p: &Palette) -> Self {
        Self {
            ground: p.window_bg,
            surround: p.surround,
            raised: p.element,
            border: p.border_mid(),
            hairline: p.border_soft(),
            ink: p.text,
            ink_dim: p.text_muted,
            ink_faint: p.text_disabled,
            accent: p.accent,
        }
    }

    pub(crate) fn title(self) -> TextStyle {
        TextStyle::default()
            .with_font_size(TITLE_PX)
            .with_weight(FontWeight::BOLD)
            .with_color(self.ink)
    }

    pub(crate) fn heading(self) -> TextStyle {
        TextStyle::default()
            .with_font_size(CAPTION_PX)
            .with_weight(FontWeight::BOLD)
            .with_color(self.ink_faint)
    }

    pub(crate) fn body(self) -> TextStyle {
        TextStyle::default().with_color(self.ink)
    }

    pub(crate) fn dim(self) -> TextStyle {
        TextStyle::default().with_color(self.ink_dim)
    }

    pub(crate) fn caption(self) -> TextStyle {
        TextStyle::default()
            .with_font_size(CAPTION_PX)
            .with_color(self.ink_dim)
    }

    pub(crate) fn mono(self, color: RgbaF32) -> TextStyle {
        TextStyle::default()
            .with_family(FontFamily::MONO)
            .with_font_size(CAPTION_PX + 1.0)
            .with_color(color)
    }

    /// A panel's frame: the raised surface, a hairline, the card radius.
    pub(crate) fn card(self) -> Chrome {
        Chrome::rounded(self.ground, Corners::all(CARD_RADIUS))
            .with_border(Stroke::new(self.border, 1.0))
    }
}

/// The type ramp around the stock 14 px body.
const CAPTION_PX: f32 = 12.0;
const TITLE_PX: f32 = 20.0;
const CARD_RADIUS: f32 = 6.0;

/// What a study's header says: what it is, the keys it answers to, the API it is built from, the
/// showcase pages that show each piece alone, and where its source is.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StudyHeader<'a> {
    pub(crate) title: &'static str,
    pub(crate) blurb: &'static str,
    pub(crate) keys: &'static [&'static str],
    /// `api!` spellings, which a `const` cannot hold, so the header borrows them.
    pub(crate) apis: &'a [&'static str],
    pub(crate) showcase: &'static [&'static str],
    pub(crate) source: &'static str,
}

impl StudyHeader<'_> {
    pub(crate) fn show(self, ui: &mut Ui, t: Tokens) {
        Panel::vstack()
            .id_salt(("header", self.title))
            .size((Sizing::FILL, Sizing::HUG))
            .gap(6.0)
            .show(ui, |ui| {
                Text::new(self.title).style(&t.title()).show(ui);
                Text::new(self.blurb)
                    .style(&t.dim())
                    .size((Sizing::FILL, Sizing::HUG))
                    .text_wrap(TextWrap::WrapWithOverflow)
                    .show(ui);
                chips(ui, t, "keys", self.keys, t.ink);
                chips(ui, t, "built from", self.apis, t.ink_dim);
                chips(ui, t, "showcase", self.showcase, t.ink_dim);
                Text::new(self.source)
                    .style(&t.mono(t.ink_faint))
                    .id_salt("source")
                    .show(ui);
            });
    }
}

/// A labeled row of mono chips; nothing for an empty list.
fn chips(ui: &mut Ui, t: Tokens, label: &'static str, items: &[&'static str], ink: RgbaF32) {
    if items.is_empty() {
        return;
    }
    Panel::wrap_hstack()
        .id_salt(label)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(4.0)
        .line_gap(4.0)
        .show(ui, |ui| {
            Text::new(label).style(&t.caption()).show(ui);
            for &item in items {
                Panel::hstack()
                    .id_salt(item)
                    .padding((6.0, 2.0))
                    .chrome(
                        Chrome::rounded(t.raised, Corners::all(3.0))
                            .with_border(Stroke::new(t.hairline, 1.0)),
                    )
                    .show(ui, |ui| {
                        Text::new(item).style(&t.mono(ink)).show(ui);
                    });
            }
        });
}
