//! The drop-down selector: a trigger that opens a popup list, and the
//! open/closed flag one trigger site keeps between frames.

use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::style::{LineCap, LineJoin};
use crate::theme::button::ButtonTheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::menu::MenuTheme;
use crate::theme::selection::Selection;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::Response;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::menu::Menu;
use crate::widgets::menu::menu_item::MenuItem;
use crate::widgets::menu::menu_landing::MenuLanding;
use crate::widgets::text::Text;
use std::rc::Rc;

/// A dropdown selector: a button-styled trigger showing the current choice,
/// opening a [`Menu`] list on click. Picking a row sets
/// the `&mut usize` selection and closes; clicking outside or Esc dismisses.
/// Open state lives in the state map keyed off the combo's id; [`Self::open`],
/// [`Self::close`] and [`Self::is_open`] drive it from outside.
///
/// `*selected` is an *index* coerced for display: past the end of `options`
/// shows the last option, and an empty list shows an empty trigger. The bound
/// index is rewritten only when the user picks.
///
/// The trigger reuses [`crate::Theme::button`]; the list is a [`Menu`] of [`MenuItem`] rows
/// ([`crate::Theme::menu`]).
///
/// `options` is the caller's collection, uncopied: [`new`](Self::new) takes a
/// slice whose elements *are* text (`&[&str]`, `&[String]`,
/// `&[Cow<'_, str>]`), [`labeled`](Self::labeled) one whose elements carry it.
/// A closed combo reads exactly one label.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ComboBox<'a, S, L> {
    widget: Widget,
    selected: &'a mut usize,
    options: &'a [S],
    label: L,
    style: Option<&'a ComboBoxTheme>,
    button_style: Option<&'a ButtonTheme>,
    menu_style: Option<&'a MenuTheme>,
}

impl<'a, S: AsRef<str>> ComboBox<'a, S, fn(&S) -> &str> {
    /// A dropdown over options that are themselves text.
    #[track_caller]
    pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self {
        Self::labeled(selected, options, S::as_ref)
    }
}

impl<'a, S, L: Fn(&S) -> &str> ComboBox<'a, S, L> {
    /// A dropdown over rows that *carry* a label: `label` reads each row's text.
    /// For option types no `AsRef<str>` could serve, e.g. a record with an id beside
    /// a display name. `label` is any `Fn`, so it may capture a table.
    #[track_caller]
    pub fn labeled(selected: &'a mut usize, options: &'a [S], label: L) -> Self {
        Self {
            // Alt+Down, the arrows, Home and End are `CARET`; the activation keys add `TEXT` in
            // `show`.
            widget: Widget::hstack()
                .sense(Sense::CLICK)
                .focusable(true)
                .input_scope(KeyFilter::CARET),
            selected,
            options,
            label,
            style: None,
            button_style: None,
            menu_style: None,
        }
    }

    /// Per-instance override of [`Theme::combo_box`](crate::Theme::combo_box), the widget's own
    /// geometry: the label and chevron gutter, and the chevron. [`Self::button_style`] restyles
    /// the trigger chrome, and [`Self::menu_style`] the list.
    pub fn style(mut self, s: impl Into<Option<&'a ComboBoxTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Per-instance override of [`Theme::button`](crate::Theme::button), which the trigger
    /// paints as. Separate from [`Self::style`] because the combo is a button plus a menu.
    pub fn button_style(mut self, s: impl Into<Option<&'a ButtonTheme>>) -> Self {
        self.button_style = s.into();
        self
    }

    /// Per-instance override of [`Theme::menu`](crate::Theme::menu), which the list is a menu
    /// in.
    pub fn menu_style(mut self, s: impl Into<Option<&'a MenuTheme>>) -> Self {
        self.menu_style = s.into();
        self
    }

    /// Record the trigger, and the dropdown when open.
    ///
    /// The [`ValueResponse`]'s own `response` is the trigger's; read `changed` for
    /// the pick, which commits at once. The list marks the pick. Focused, it takes the keys of
    /// the WAI-ARIA APG's select-only combobox: Space, Enter, Alt+Down and Down open the list on
    /// the pick, Up and Home on its first row, and End on its last. No key changes the pick
    /// while the list is closed, so the value never changes where the user cannot see the list
    /// ([APG](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/examples/combobox-select-only/)).
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let mut response = self.widget.activation_response(ui, Activation::Button);
        let id = response.id;
        let list = id.with("list");
        let shown = domain::idx(*self.selected, self.options.len());
        let mut landing = shown.map_or(MenuLanding::First, |pick| {
            MenuLanding::Row(option_id(list, pick))
        });
        if !response.disabled && ui.is_focus_within(id) {
            // Every chord is sampled: `shortcut_pressed` also keeps it subscribed for the wake gate.
            let mut key = |shortcut| self.widget.shortcut_pressed(ui, shortcut);
            let opens_pick = [
                Shortcut::new(ShortcutMods::ALT, Key::ArrowDown),
                Shortcut::key(Key::ArrowDown),
            ]
            .map(&mut key)
            .contains(&true);
            let opens_first = [Shortcut::key(Key::ArrowUp), Shortcut::key(Key::Home)]
                .map(&mut key)
                .contains(&true);
            let opens_last = key(Shortcut::key(Key::End));
            if opens_first {
                landing = MenuLanding::First;
            } else if opens_last {
                landing = MenuLanding::Last;
            }
            if (opens_pick || opens_first || opens_last)
                && !response.clicked()
                && !Menu::is_open(ui, id)
            {
                response.state.activate();
            }
        }

        // Trigger chrome from the button theme, as in `Button`. One handle covers both
        // reads; the `record` closure below owns `ui` mutably.
        let theme = Rc::clone(ui.theme());
        let slot = self.button_style.unwrap_or(&theme.button);
        let plan = slot.plan(&response, Selection::Unselected, theme.text);
        let look = self.widget.wear(ui, plan);

        let geometry = self.style.unwrap_or(&theme.combo_box);
        self.widget
            .configure()
            .default_justify(Justify::SpaceBetween)
            .default_child_align(Align::v(VAlign::Center))
            .default_gap(geometry.gap);

        let arrow_color = look.text.color;
        let text_style = look.text;
        let chosen = shown.map_or("", |shown| (self.label)(&self.options[shown]));
        // Intern the selected label: options borrow from the caller's collection, not
        // `'static`.
        let label = ui.intern(chosen);

        self.widget.record(ui, Some(&look.chrome), |ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);

            let arrow = Widget::leaf().id(id.with("arrow")).size((
                Sizing::fixed(geometry.arrow_size.x),
                Sizing::fixed(geometry.arrow_size.y),
            ));
            arrow.record(ui, None, |ui| {
                let pts = geometry.chevron_points();
                ui.add_shape(
                    Shape::polyline(&pts, Stroke::new(arrow_color, geometry.arrow_width))
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            });
        });

        let options = self.options;
        let label = self.label;
        let selected = self.selected;
        // A dropdown is at least as wide as its trigger, an explicit set that outranks
        // `MenuTheme::min_width`.
        let resp = Menu::below(&response)
            .style(self.menu_style)
            .landing(landing)
            .id(list)
            .min_size((response.rect.map_or(0.0, |rect| rect.size.w), 0.0))
            .show(ui, |ui, _| {
                let mut picked = false;
                for (i, opt) in options.iter().enumerate() {
                    let lbl = ui.intern(label(opt));
                    let row = MenuItem::new(lbl)
                        .id(option_id(list, i))
                        .selected(shown == Some(i));
                    if row.show(ui).clicked() && *selected != i {
                        *selected = i;
                        picked = true;
                    }
                }
                picked
            });
        let changed = resp.inner.unwrap_or(false);

        ValueResponse {
            response: Response::new(ui, response),
            changed,
            committed: changed,
        }
    }
}

/// Driving a combo box's list from outside, by the combo's id, as a [`Menu`] is driven. One concrete
/// instance holds them, so a call names no option type.
impl ComboBox<'static, &'static str, fn(&&'static str) -> &'static str> {
    /// Open the list of the combo box `id`, e.g. from a keyboard shortcut; it records on the
    /// combo's next `show`.
    pub fn open(ui: &mut Ui, id: WidgetId) {
        Menu::open_below(ui, id);
    }

    /// Close the list of the combo box `id`, giving back the input it held at once. No-op if
    /// closed.
    pub fn close(ui: &mut Ui, id: WidgetId) {
        Menu::close(ui, id);
    }

    /// `true` while the list of the combo box `id` is open.
    pub fn is_open(ui: &Ui, id: WidgetId) -> bool {
        Menu::is_open(ui, id)
    }
}

/// The row of option `i` in the list `list`. Keyed by index, so a keyboard open can land on the
/// pick before the row records.
fn option_id(list: WidgetId, i: usize) -> WidgetId {
    list.with(("option", i))
}

impl<S, L> Configure for ComboBox<'_, S, L> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
