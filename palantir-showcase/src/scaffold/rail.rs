//! The rail the tour picks its pages from, on one inset grid: the band keeps every block
//! [`INSET`] from its edges, and an item's text sits [`INSET`] inside its pill, so every text of
//! the rail starts at twice the inset. The list's scrollbar paints in a gutter outside the pills
//! ([Apple HIG, Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars)).

use palantir::{
    Align, Axis, Button, ButtonTheme, Chrome, ColorScheme, Configure, Corners, FontWeight, Grid,
    GridCell, IconHandle, IconSet, IconTable, Panel, Preference, RadioGroup, RadioLook, Reduction,
    Scroll, SegmentedTheme, SizeSpec, Sizing, SlotDefaults, Spacing, StatefulLook, Text,
    TextStyleOverrides, Theme, Tooltip, Track, Ui, UiPreferences, VAlign, WidgetId, WidgetLook,
};

use crate::scaffold::tokens::M;
use crate::scaffold::tokens::Tokens;

/// The rail's width: room for a preference row's name beside its [`Segmented`] track, as the
/// longest name, "Transparency", takes 78 px at the caption size. The track takes the rest,
/// 312 − 3 × [`INSET`] − 78 − [`NAME_GAP`] = 190 px: three segments of 62 px inside its
/// 2 px padding, which leave the widest choice, the Theme row's "Light" at 32 px after its
/// 14.4 px icon and the 4 px gap, 5.8 px each side.
pub(crate) const WIDTH: f32 = 312.0;
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
        .chrome(Chrome::new(t.surround))
        .show(ui, body);
}

/// A block of the rail outside the list, held off the right edge as the list's pills are. Its
/// controls span the pills' column; a label in it takes [`label`] to sit on the items' text.
#[track_caller]
pub(crate) fn block(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    Panel::vstack()
        .auto_id()
        .size(SizeSpec::FILL_W)
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
        .size(SizeSpec::FILL_W)
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
        .size(SizeSpec::FILL)
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
        let unselected = theme.menu_bar.title.unselected.clone();
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
                    ..theme.menu_bar.title.defaults
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

/// The tour's five preferences, each a grid row of its name beside a [`Segmented`] track: the
/// scheme, motion, transparency, power and frame rate. The names' column hugs the longest, so the tracks share
/// both edges. A press sets one for every window. Here alone, apart from the pages, so no demo
/// reads as one of them.
pub(crate) fn preferences(ui: &mut Ui, t: Tokens) {
    Grid::new()
        .id_salt("preferences")
        .cols([Track::HUG, Track::FILL])
        .rows([Track::HUG; 5])
        .gap(NAME_GAP)
        .line_gap(M.space.sm)
        .size(SizeSpec::FILL_W)
        .show(ui, |ui| preference_rows(ui, &Segmented::new(ui.theme(), t)));
}

/// The rows of [`preferences`], each set from the segment pressed this frame.
fn preference_rows(ui: &mut Ui, track: &Segmented) {
    let icons = SchemeIcons::load(ui);
    let held = ui.preferences();
    let follows_scheme = match ui.system_color_scheme() {
        Some(ColorScheme::Light) => "Follows the system, light now",
        Some(ColorScheme::Dark) => "Follows the system, dark now",
        None => "Follows the system, which reports no scheme: dark until it does",
    };
    let scheme = track.row(
        ui,
        PreferenceRow {
            row: 0,
            label: "Theme",
            choices: [
                (Preference::System, "Auto"),
                (Preference::Fixed(ColorScheme::Light), "Light"),
                (Preference::Fixed(ColorScheme::Dark), "Dark"),
            ],
            icons: Some([icons.auto, icons.light, icons.dark]),
            current: held.scheme,
            follows: follows_scheme,
        },
    );
    let motion = track.row(
        ui,
        PreferenceRow {
            row: 1,
            label: "Motion",
            choices: REDUCE,
            icons: None,
            current: held.motion,
            follows: if ui.reductions().power.is_reduced() {
                "Follows the system, transitions snap under reduced power"
            } else {
                follows_reduction(ui.system_reductions().motion.is_reduced())
            },
        },
    );
    let transparency = track.row(
        ui,
        PreferenceRow {
            row: 2,
            label: "Transparency",
            choices: REDUCE,
            icons: None,
            current: held.transparency,
            follows: if ui.reductions().power.is_reduced() {
                "Follows the system, less now under reduced power"
            } else {
                follows_reduction(ui.system_reductions().transparency.is_reduced())
            },
        },
    );
    let power = track.row(
        ui,
        PreferenceRow {
            row: 3,
            label: "Power",
            choices: REDUCE,
            icons: None,
            current: held.power,
            follows: follows_reduction(ui.system_reductions().power.is_reduced()),
        },
    );
    let frame_rate = track.row(
        ui,
        PreferenceRow {
            row: 4,
            label: "Frame rate",
            choices: REDUCE,
            icons: None,
            current: held.frame_rate,
            follows: if ui.reductions().power.is_reduced() {
                "Follows the power, 30 Hz now"
            } else {
                "Follows the power, full now"
            },
        },
    );
    ui.set_preferences(UiPreferences {
        scheme,
        motion,
        transparency,
        power,
        frame_rate,
    });
}

/// The choices of a [`Preference<Reduction>`] under the name of what it reduces: "Less", not "On"
/// under a "Reduce …" name, so the segment says what it does without the name.
const REDUCE: [(Preference<Reduction>, &str); 3] = [
    (Preference::System, "Auto"),
    (Preference::Fixed(Reduction::Unreduced), "Full"),
    (Preference::Fixed(Reduction::Reduced), "Less"),
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
/// A segment's height, inside the track's padding.
const SEGMENT: f32 = 20.0;

/// One preference's row on the rail's track: its grid row and label, the three choices with the
/// text of each and an icon before it where the row has them, the one held now, and the tooltip
/// on the first, which follows the system.
#[derive(Clone, Copy, Debug)]
struct PreferenceRow<T> {
    row: u16,
    label: &'static str,
    choices: [(T, &'static str); 3],
    icons: Option<[IconHandle; 3]>,
    current: T,
    follows: &'static str,
}

/// The preference rows' track: the stock segmented look at the caption size, with no padding in a
/// segment, so the widest choice fits its third of the track.
#[derive(Debug)]
struct Segmented {
    style: SegmentedTheme,
    t: Tokens,
}

impl Segmented {
    fn new(theme: &Theme, t: Tokens) -> Self {
        let mut style = theme.segmented.clone();
        let segment = &mut style.segment;
        for pack in [&mut segment.selected, &mut segment.unselected] {
            for look in pack.each_mut() {
                look.text = look.text.with_font_size(M.text.caption);
            }
        }
        segment.defaults.padding = Spacing::ZERO;
        Self { style, t }
    }

    /// `spec`'s row: its label beside a segment per choice, its current one checked; the choice
    /// picked this frame, else the current one. The track is a radio group, so one Tab lands in
    /// it and an arrow picks (WAI-ARIA APG, Radio Group). The first segment follows the system,
    /// and its tooltip says what that shows now, as the segment alone cannot.
    fn row<T: Copy + PartialEq>(&self, ui: &mut Ui, spec: PreferenceRow<T>) -> T {
        let PreferenceRow {
            row,
            label,
            choices,
            icons,
            current,
            follows,
        } = spec;
        let mut next = current;
        Text::new(label)
            .id_salt(label)
            .style(&self.t.caption())
            .margin((INSET, 0.0, 0.0, 0.0))
            .grid_cell(GridCell::at(row, 0))
            .align(VAlign::Center)
            .show(ui);
        RadioGroup::new(&mut next)
            .look(RadioLook::Segment(Some(&self.style)))
            .axis(Axis::X)
            .id_salt((label, "track"))
            .grid_cell(GridCell::at(row, 1))
            .size(SizeSpec::FILL_W)
            .show(ui, |ui, options| {
                for (idx, (choice, text)) in choices.into_iter().enumerate() {
                    let mut option = options.option(choice).label(text);
                    if let Some(icons) = icons {
                        option = option.icon(icons[idx]);
                    }
                    let segment = option
                        .size((Sizing::FILL, Sizing::fixed(SEGMENT)))
                        .show(ui)
                        .response;
                    if idx == 0 {
                        Tooltip::on(&segment, follows).show(ui);
                    }
                }
            });
        next
    }
}

/// The Theme row's icons, a monitor for the system's scheme, a sun for light and a moon for
/// dark, as most theme switchers mark the three: single-color outlines, so each takes its
/// label's ink.
#[derive(Clone, Debug)]
struct SchemeIcons {
    #[expect(dead_code, reason = "held so its icons stay loaded")]
    set: IconSet,
    auto: IconHandle,
    light: IconHandle,
    dark: IconHandle,
}

impl SchemeIcons {
    /// The set, parked in a state row across frames, as an `IconSet` dropped each frame would
    /// unload and re-rasterize its icons each frame.
    fn load(ui: &mut Ui) -> Self {
        ui.with_state_or_insert_with(
            WidgetId::from_hash("showcase::rail::scheme_icons"),
            |ui| {
                let set = ui.load_icons(
                    IconTable::from_svgs([
                        ("auto", MONITOR_SVG),
                        ("light", SUN_SVG),
                        ("dark", MOON_SVG),
                    ])
                    .expect("the bundled icons are readable SVGs with distinct names"),
                );
                let handle = |name| set.by_name(name).expect("bundled icon");
                Self {
                    auto: handle("auto"),
                    light: handle("light"),
                    dark: handle("dark"),
                    set,
                }
            },
            |_, held: &mut Self| held.clone(),
        )
    }
}

/// A screen on a stand.
const MONITOR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<rect x="3" y="4" width="18" height="12" rx="2" fill="none" stroke="#fff" stroke-width="2"/>
<path d="M8 20h8M12 16v4" fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round"/>
</svg>"##;

/// A disc with eight rays.
const SUN_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<circle cx="12" cy="12" r="4" fill="none" stroke="#fff" stroke-width="2"/>
<path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round"/>
</svg>"##;

/// A crescent.
const MOON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z" fill="none" stroke="#fff" stroke-width="2" stroke-linejoin="round"/>
</svg>"##;
