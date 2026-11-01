//! The rail the tour picks its pages from, on one inset grid: the band keeps every block
//! [`INSET`] from its edges, and an item's text sits [`INSET`] inside its pill, so every text of
//! the rail starts at twice the inset. The list's scrollbar paints in a gutter outside the pills
//! ([Apple HIG, Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars)).

use palantir::{
    Align, Axis, Button, ButtonTheme, Chrome, ColorScheme, Configure, Corners, FontWeight, Metrics,
    Panel, SchemePreference, Scroll, Sizing, SlotDefaults, Spacing, StatefulLook, Text,
    TextStyleOverrides, Theme, Tooltip, Ui, WidgetId, WidgetLook,
};

use crate::scaffold::tokens::Tokens;

const M: Metrics = Metrics::DEFAULT;

/// The rail's width.
pub(crate) const WIDTH: f32 = 208.0;
/// The inset of the rail's blocks from its edges, and of an item's text from its pill.
pub(crate) const INSET: f32 = M.space.md;
/// An item's height: macOS's medium sidebar row.
const ROW: f32 = 28.0;
/// How much of the accent tints the open item's pill at rest, hovered and pressed.
const PILL_TINT: [f32; 3] = [0.16, 0.22, 0.28];

/// The rail: a surround band of [`WIDTH`] holding `body` on the inset grid. Its right edge has
/// no inset, so the list's gutter reaches it; every other block takes [`block`]'s margin.
pub(crate) fn show(ui: &mut Ui, t: Tokens, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .id_salt("rail")
        .size((Sizing::fixed(WIDTH), Sizing::FILL))
        .padding((INSET, M.space.lg, 0.0, INSET))
        .gap(INSET)
        .chrome(Chrome::fill(t.surround))
        .show(ui, body);
}

/// A block of the rail outside the list, held off the right edge as the list's pills are. Its
/// controls span the pills' column; a label in it takes [`label`] to sit on the items' text.
#[track_caller]
pub(crate) fn block(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size((Sizing::FILL, Sizing::HUG))
        .margin((0.0, 0.0, INSET, 0.0))
        .gap(M.space.sm)
        .show(ui, body);
}

/// A small-caps label of a block, on the items' text column.
pub(crate) fn label(ui: &mut Ui, t: Tokens, text: &'static str) {
    Text::new(text)
        .id_salt(text)
        .style(&t.heading())
        .margin((INSET, 0.0, 0.0, 0.0))
        .show(ui);
}

/// The tour's name and what it is, its text on the items' column.
pub(crate) fn brand(ui: &mut Ui, t: Tokens, title: &'static str, subtitle: &'static str) {
    Panel::vstack()
        .id_salt("brand")
        .size((Sizing::FILL, Sizing::HUG))
        .padding((INSET, 0.0, INSET, 0.0))
        .gap(M.space.xxs)
        .show(ui, |ui| {
            Text::new(title)
                .style(
                    &t.body()
                        .with_font_size(M.text.title)
                        .with_weight(FontWeight::BOLD),
                )
                .show(ui);
            Text::new(subtitle).style(&t.caption()).show(ui);
        });
}

/// The scrolling list of items under `id`, an arrow group so one Tab lands in it, to show with
/// the items as its body. Its right padding is the gutter the overlay scrollbar paints in,
/// outside the pills.
pub(crate) fn list(id: WidgetId) -> Scroll<'static> {
    Scroll::vertical()
        .id(id)
        .size((Sizing::FILL, Sizing::FILL))
        .overlay_bars()
        .padding((0.0, 0.0, INSET, 0.0))
        .gap(M.space.xxs)
        .arrow_focus(Axis::Y)
}

/// The looks of the list's items: the menu bar's flat titles at rest, and the open one on a
/// pill the accent tints, in the ink.
#[derive(Debug)]
pub(crate) struct Nav {
    theme: ButtonTheme,
}

impl Nav {
    pub(crate) fn new(theme: &Theme, t: Tokens) -> Self {
        let pill = |tint: f32| WidgetLook {
            chrome: Chrome::rounded(t.accent.with_alpha(tint), Corners::all(M.radius.control)),
            text: TextStyleOverrides::NONE.with_color(t.ink),
        };
        let [rest, hovered, pressed] = PILL_TINT;
        let unselected = theme.menu_bar.title.clone();
        Self {
            theme: ButtonTheme {
                selected: StatefulLook {
                    normal: pill(rest),
                    hovered: pill(hovered),
                    active: pill(pressed),
                    disabled: unselected.disabled.clone(),
                },
                unselected,
                defaults: SlotDefaults {
                    padding: Spacing::xy(INSET, 0.0),
                    ..theme.menu_bar.defaults
                },
            },
        }
    }

    /// One item of `label` under `id`, on its pill while `open`; whether it was clicked.
    pub(crate) fn item(&self, ui: &mut Ui, id: WidgetId, label: &'static str, open: bool) -> bool {
        Button::new()
            .id(id)
            .label(label)
            .style(&self.theme)
            .selected(open)
            .text_align(Align::LEFT)
            .size((Sizing::FILL, Sizing::fixed(ROW)))
            .show(ui)
            .clicked()
    }
}

/// The scheme preference as three segments; a press sets it for every window. System's tooltip
/// says what the system reports, as the segment alone cannot.
pub(crate) fn scheme_switch(ui: &mut Ui) {
    let current = ui.scheme_preference();
    let system = match ui.system_color_scheme() {
        Some(ColorScheme::Light) => "Follows the system, light now",
        Some(ColorScheme::Dark) => "Follows the system, dark now",
        None => "Follows the system, which reports no scheme: dark until it does",
    };
    let mut next = current;
    Panel::hstack()
        .id_salt("scheme switch")
        .size((Sizing::FILL, Sizing::HUG))
        .gap(M.space.xxs)
        .show(ui, |ui| {
            for (preference, label) in [
                (SchemePreference::System, "System"),
                (SchemePreference::Light, "Light"),
                (SchemePreference::Dark, "Dark"),
            ] {
                // A narrower inset than a button's, so three labels share the rail's width.
                let segment = Button::new()
                    .id_salt(label)
                    .label(label)
                    .selected(current == preference)
                    .padding(Spacing::xy(M.space.xs, M.space.xs))
                    .size((Sizing::FILL, Sizing::HUG))
                    .show(ui);
                if segment.clicked() {
                    next = preference;
                }
                if preference == SchemePreference::System {
                    let segment = segment.snapshot();
                    Tooltip::on(&segment, system).show(ui);
                }
            }
        });
    if next != current {
        ui.set_scheme_preference(next);
    }
}
