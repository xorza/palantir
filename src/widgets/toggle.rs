use crate::animation::AnimSpec;
use crate::input::response::ResponseState;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::sizing::Sizing;
use crate::primitives::corners::Corners;
use crate::primitives::interned_str::TextInput;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;
use crate::widgets::text::Text;
use crate::widgets::theme::toggle::ToggleTheme;
use crate::widgets::theme::widget_look::WidgetLook;
use crate::widgets::{Response, WidgetEntry};

/// Chrome inputs for [`toggle_row`], built by the caller from its
/// resolved [`crate::ToggleTheme`]. `look_target` is the picked
/// per-state look **before** animation — `toggle_row` runs
/// [`WidgetLook::animate`] itself, so the caller's borrow on the source
/// theme (which may point into `ui.theme`) is released before the
/// `&mut Ui` animate reborrow.
pub(crate) struct ToggleChrome {
    pub(crate) look_target: WidgetLook,
    pub(crate) anim: Option<AnimSpec>,
    pub(crate) box_size: f32,
    pub(crate) row_gap: f32,
    /// RadioButton forces the box chrome to a pill (`box_size * 0.5`
    /// radius) regardless of the theme's stored corner radius.
    pub(crate) pill: bool,
}

impl ToggleChrome {
    /// Build the chrome from a resolved [`ToggleTheme`] and the toggle's
    /// `(state, on)` — the shape Checkbox and RadioButton both need; they
    /// differ only in `pill`. `look_target` is the *picked* look (cloned out),
    /// so the borrow on `theme` (which may point into `ui.theme`) is released
    /// before `toggle_row`'s `&mut Ui` animate reborrow.
    pub(crate) fn new(theme: &ToggleTheme, state: &ResponseState, on: bool, pill: bool) -> Self {
        Self {
            look_target: theme.pick(state, on).clone(),
            anim: theme.anim,
            box_size: theme.box_size,
            row_gap: theme.row_gap,
            pill,
        }
    }
}

/// Shared `HStack [box, label]` scaffolding behind [`crate::Checkbox`]
/// and [`crate::RadioButton`] — the two differ only in the toggle
/// semantics (resolved by the caller before this runs), the indicator
/// glyph (`paint_indicator`), and whether the box is a pill
/// (`chrome.pill`). Everything structural — the look animation, the row
/// gap / cross-centering, the `Fixed×Fixed` box leaf with its chrome,
/// the label leaf — lives here.
///
/// The row `HStack` node (sense + salt already set) rides in
/// `entry.widget`. `paint_indicator` runs inside the box leaf — it
/// receives the box side length and is responsible for its own
/// checked/selected gate.
pub(crate) fn toggle_row<'ui, 'text>(
    ui: &'ui mut Ui,
    mut entry: WidgetEntry,
    chrome: ToggleChrome,
    label: TextInput<'text>,
    paint_indicator: impl FnOnce(&mut Ui, f32),
) -> Response<'ui> {
    let id = entry.widget.id();
    let box_size = chrome.box_size;
    let fallback_text = ui.theme.text.clone();
    let mut look = chrome
        .look_target
        .animate(ui, id, &fallback_text, chrome.anim);
    if chrome.pill {
        look.background.corners = Corners::all(box_size * 0.5);
    }

    entry.widget.node.gaps.set_gap(chrome.row_gap);
    entry.widget.node.child_align = Align::v(VAlign::Center);

    entry.widget.record(ui, None, |ui| {
        let box_elem = Node::leaf()
            .id(id.with("box"))
            .size((Sizing::fixed(box_size), Sizing::fixed(box_size)));
        ui.widget(box_elem)
            .record(ui, Some(&look.background), |ui| {
                paint_indicator(ui, box_size)
            });

        if !label.is_empty() {
            Text::new(label)
                .id(id.with("label"))
                .style(&look.text)
                .text_align(Align::v(VAlign::Center))
                .show(ui);
        }
    });

    entry.into_response(ui)
}
