//! The rail the tour picks its pages from, on one inset grid: the band keeps every block
//! [`INSET`] from its edges, and an item's text sits [`INSET`] inside its pill, so every text of
//! the rail starts at twice the inset. The list's scrollbar paints in a gutter outside the pills
//! ([Apple HIG, Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars)).

use palantir::{
    Align, Axis, Button, ButtonTheme, Chrome, ColorScheme, Configure, Corners, FontWeight, Metrics,
    Panel, ReducePreference, SchemePreference, Scroll, Sizing, SlotDefaults, Spacing, StatefulLook,
    Text, TextStyleOverrides, Theme, Tooltip, Ui, WidgetId, WidgetLook,
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

/// The tour's three preferences, each a captioned row of segments: the scheme, motion and
/// transparency. A press sets one for every window. Here alone, apart from the pages, so no demo
/// reads as one of them.
pub(crate) fn preferences(ui: &mut Ui, t: Tokens) {
    let scheme = ui.scheme_preference();
    let follows_scheme = match ui.system_color_scheme() {
        Some(ColorScheme::Light) => "Follows the system, light now",
        Some(ColorScheme::Dark) => "Follows the system, dark now",
        None => "Follows the system, which reports no scheme: dark until it does",
    };
    let picked = segments(
        ui,
        t,
        "Theme",
        [
            (SchemePreference::System, "System"),
            (SchemePreference::Light, "Light"),
            (SchemePreference::Dark, "Dark"),
        ],
        scheme,
        follows_scheme,
    );
    if picked != scheme {
        ui.set_scheme_preference(picked);
    }

    let motion = ui.motion_preference();
    let picked = segments(
        ui,
        t,
        "Reduce motion",
        REDUCE,
        motion,
        follows_reduction(ui.system_motion_reduced()),
    );
    if picked != motion {
        ui.set_motion_preference(picked);
    }

    let transparency = ui.transparency_preference();
    let picked = segments(
        ui,
        t,
        "Reduce transparency",
        REDUCE,
        transparency,
        follows_reduction(ui.system_transparency_reduced()),
    );
    if picked != transparency {
        ui.set_transparency_preference(picked);
    }
}

/// The choices of a [`ReducePreference`] under a "Reduce …" caption, as macOS names the
/// switches: off is full, on reduced. Short, as "Reduced" overflows a third of the rail.
const REDUCE: [(ReducePreference, &str); 3] = [
    (ReducePreference::System, "System"),
    (ReducePreference::Full, "Off"),
    (ReducePreference::Reduced, "On"),
];

/// What following the system shows now, where the system asks for a reduction or not.
const fn follows_reduction(reduced: bool) -> &'static str {
    if reduced {
        "Follows the system, reduced now"
    } else {
        "Follows the system, full now"
    }
}

/// `label` over a segment per choice, `current`'s selected; the choice pressed this frame, else
/// `current`. The first segment follows the system, and its tooltip, `follows`, says what that
/// shows now, as the segment alone cannot.
fn segments<T: Copy + PartialEq>(
    ui: &mut Ui,
    t: Tokens,
    label: &'static str,
    choices: [(T, &'static str); 3],
    current: T,
    follows: &'static str,
) -> T {
    let mut next = current;
    Panel::vstack()
        .id_salt(label)
        .size((Sizing::FILL, Sizing::HUG))
        .gap(M.space.xxs)
        .show(ui, |ui| {
            Text::new(label)
                .style(&t.caption())
                .margin((INSET, 0.0, 0.0, 0.0))
                .show(ui);
            Panel::hstack()
                .auto_id()
                .size((Sizing::FILL, Sizing::HUG))
                .gap(M.space.xxs)
                .show(ui, |ui| {
                    for (idx, (choice, text)) in choices.into_iter().enumerate() {
                        // A narrower inset than a button's, so three labels share the rail's width.
                        let segment = Button::new()
                            .id_salt(text)
                            .label(text)
                            .selected(current == choice)
                            .padding(Spacing::xy(M.space.xs, M.space.xs))
                            .size((Sizing::FILL, Sizing::HUG))
                            .show(ui);
                        if segment.clicked() {
                            next = choice;
                        }
                        if idx == 0 {
                            let segment = segment.snapshot();
                            Tooltip::on(&segment, follows).show(ui);
                        }
                    }
                });
        });
    next
}
