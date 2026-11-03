//! The rail the tour picks its pages from, on one inset grid: the band keeps every block
//! [`INSET`] from its edges, and an item's text sits [`INSET`] inside its pill, so every text of
//! the rail starts at twice the inset. The list's scrollbar paints in a gutter outside the pills
//! ([Apple HIG, Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars)).

use palantir::{
    Align, Axis, Button, ButtonTheme, Chrome, ColorScheme, Configure, Corners, FontWeight, Grid,
    GridCell, Metrics, Panel, ReducePreference, RgbaF32, SchemePreference, Scroll, Sizing,
    SlotDefaults, Spacing, StatefulLook, Stroke, Text, TextStyleOverrides, Theme, Tooltip, Track,
    Ui, VAlign, WidgetId, WidgetLook,
};

use crate::scaffold::tokens::Tokens;

const M: Metrics = Metrics::DEFAULT;

/// The rail's width: room for a preference row's name beside its [`Segmented`] track, as the
/// longest name, "Transparency", takes 78 px at the caption size. The track takes the rest,
/// 256 − 3 × [`INSET`] − 78 − [`NAME_GAP`] = 134 px: three segments of 43.3 px inside its
/// [`TRACK_PAD`], which leave the widest choice, "Light" at 32 px, 5.7 px each side.
pub(crate) const WIDTH: f32 = 256.0;
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

/// The tour's three preferences, each a grid row of its name beside a [`Segmented`] track: the
/// scheme, motion and transparency. The names' column hugs the longest, so the tracks share both
/// edges. A press sets one for every window. Here alone, apart from the pages, so no demo reads as
/// one of them.
pub(crate) fn preferences(ui: &mut Ui, t: Tokens) {
    Grid::new()
        .id_salt("preferences")
        .cols([Track::HUG, Track::FILL])
        .rows([Track::HUG; 3])
        .gap(NAME_GAP)
        .line_gap(M.space.sm)
        .size((Sizing::FILL, Sizing::HUG))
        .show(ui, |ui| preference_rows(ui, &Segmented::new(ui.theme(), t)));
}

/// The rows of [`preferences`], each set from the segment pressed this frame.
fn preference_rows(ui: &mut Ui, track: &Segmented) {
    let scheme = ui.scheme_preference();
    let follows_scheme = match ui.system_color_scheme() {
        Some(ColorScheme::Light) => "Follows the system, light now",
        Some(ColorScheme::Dark) => "Follows the system, dark now",
        None => "Follows the system, which reports no scheme: dark until it does",
    };
    let picked = track.row(
        ui,
        0,
        "Theme",
        [
            (SchemePreference::System, "Auto"),
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
    let picked = track.row(
        ui,
        1,
        "Motion",
        REDUCE,
        motion,
        follows_reduction(ui.system_motion_reduced()),
    );
    if picked != motion {
        ui.set_motion_preference(picked);
    }

    let transparency = ui.transparency_preference();
    let picked = track.row(
        ui,
        2,
        "Transparency",
        REDUCE,
        transparency,
        follows_reduction(ui.system_transparency_reduced()),
    );
    if picked != transparency {
        ui.set_transparency_preference(picked);
    }
}

/// The choices of a [`ReducePreference`] under the name of what it reduces: "Less", not "On"
/// under a "Reduce …" name, so the segment says what it does without the name.
const REDUCE: [(ReducePreference, &str); 3] = [
    (ReducePreference::System, "Auto"),
    (ReducePreference::Full, "Full"),
    (ReducePreference::Reduced, "Less"),
];

/// What following the system shows now, where the system asks for a reduction or not.
const fn follows_reduction(reduced: bool) -> &'static str {
    if reduced {
        "Follows the system, less now"
    } else {
        "Follows the system, full now"
    }
}

/// The space between a preference's name and its track.
const NAME_GAP: f32 = M.space.sm;
/// A segment's height, inside the track's [`TRACK_PAD`].
const SEGMENT: f32 = 20.0;
/// The track's inset around its segments.
const TRACK_PAD: f32 = 2.0;

/// A segmented control: one track, its choices flat on it and the picked one on a pill of the
/// ground, as the open tab chip wears the ground and Radix's tabs put the active trigger on the
/// background over a muted list. Refused: a gapped row of buttons, whose picked one showed only as
/// a ring a focus ring also draws; a raised pill of the `element` rungs, which reads raised in a
/// dark scheme and sunk in a light one, where the rungs step darker
/// ([Apple HIG, Segmented controls](https://developer.apple.com/design/human-interface-guidelines/segmented-controls),
/// [Radix Tabs](https://www.radix-ui.com/themes/docs/components/tabs)).
#[derive(Debug)]
struct Segmented {
    theme: ButtonTheme,
    track: Chrome,
    t: Tokens,
}

impl Segmented {
    fn new(theme: &Theme, t: Tokens) -> Self {
        let corners = Corners::all(M.radius.control - TRACK_PAD);
        let look = |fill: RgbaF32, ink| WidgetLook {
            chrome: Chrome::rounded(fill, corners),
            text: TextStyleOverrides::NONE
                .with_font_size(M.text.caption)
                .with_color(ink),
        };
        let pill = WidgetLook {
            chrome: Chrome::rounded(t.ground, corners)
                .with_border(Stroke::new(t.hairline, M.stroke.hairline)),
            ..look(t.ground, t.ink)
        };
        let rest = look(RgbaF32::TRANSPARENT, t.ink_dim);
        Self {
            theme: ButtonTheme {
                selected: StatefulLook {
                    normal: pill.clone(),
                    hovered: pill.clone(),
                    active: pill.clone(),
                    disabled: pill,
                },
                unselected: StatefulLook {
                    normal: rest.clone(),
                    hovered: look(t.control_strong, t.ink),
                    // A press shows the pill's ground before the release moves the pill there.
                    active: look(t.ground, t.ink),
                    disabled: rest,
                },
                defaults: SlotDefaults {
                    padding: Spacing::ZERO,
                    ..theme.button.defaults
                },
            },
            track: Chrome::rounded(t.control, Corners::all(M.radius.control)),
            t,
        }
    }

    /// Grid row `row` of `label` beside a segment per choice, `current`'s on the pill; the choice
    /// pressed this frame, else `current`. The track is one arrow group, so one Tab lands in it,
    /// as a radio group takes one (WAI-ARIA APG, Radio Group). The first segment follows the
    /// system, and its tooltip, `follows`, says what that shows now, as the segment alone cannot.
    fn row<T: Copy + PartialEq>(
        &self,
        ui: &mut Ui,
        row: u16,
        label: &'static str,
        choices: [(T, &'static str); 3],
        current: T,
        follows: &'static str,
    ) -> T {
        let mut next = current;
        Text::new(label)
            .id_salt(label)
            .style(&self.t.caption())
            .margin((INSET, 0.0, 0.0, 0.0))
            .grid_cell(GridCell::at(row, 0))
            .align(Align::v(VAlign::Center))
            .show(ui);
        Panel::hstack()
            .id_salt((label, "track"))
            .grid_cell(GridCell::at(row, 1))
            .size((Sizing::FILL, Sizing::HUG))
            .padding(TRACK_PAD)
            .chrome(self.track.clone())
            .arrow_focus(Axis::X)
            .show(ui, |ui| {
                for (idx, (choice, text)) in choices.into_iter().enumerate() {
                    let segment = Button::new()
                        .id_salt(text)
                        .label(text)
                        .style(&self.theme)
                        .selected(current == choice)
                        .size((Sizing::FILL, Sizing::fixed(SEGMENT)))
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
        next
    }
}
