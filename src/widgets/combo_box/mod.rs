//! The drop-down selector: a trigger that opens a popup list, and the
//! open/closed flag one trigger site keeps between frames.

use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::key_class::KeyFilter;
use crate::primitives::input::keyboard::key::Key;
use crate::primitives::input::sense::Sense;
use crate::primitives::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::layout::align::VAlign;
use crate::primitives::layout::justify::Justify;
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::theme::button::ButtonTheme;
use crate::theme::combo_box::ComboBoxTheme;
use crate::theme::look::theme_slot::ThemeSlot;
use crate::theme::menu::MenuTheme;
use crate::theme::selection::Selection;
use crate::ui::Ui;
use crate::widget_core::activation::Activation;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widgets::menu::Menu;
use crate::widgets::menu::menu_item::MenuItem;
use crate::widgets::menu::menu_landing::MenuLanding;
use crate::widgets::popup::trigger_state::{Opening, TriggerRow};
use crate::widgets::text::Text;
use std::rc::Rc;

/// A dropdown selector: a button-styled trigger showing the current choice,
/// opening a [`Menu`] list on click. Picking a row moves that row's value into the bound
/// `selected` and closes; clicking outside or Esc dismisses. A pick binds a value, as egui's
/// `selectable_value` hands each row its value, so the caller keeps its own type rather than an
/// index; `T` may be a reference, so a caller holding a `&S` into a table binds it and clones
/// nothing. Open state lives in the state map keyed off the combo's id; [`ComboBoxList::open`],
/// [`ComboBoxList::close`] and [`ComboBoxList::is_open`] drive it from outside.
///
/// A `*selected` no option equals shows its own label and marks no row; it is rewritten only
/// when the user picks an option it does not equal.
///
/// The trigger reuses [`crate::Theme::button`]; the list is a [`Menu`] of [`MenuItem`] rows
/// ([`crate::Theme::menu`]).
///
/// `options` is any collection that yields the values again when cloned: an array of `Copy`
/// values, `slice.iter()` for references, `slice.iter().copied()`. A closed combo walks it once to
/// find the pick and reads exactly one label; an open one walks it once more for the rows.
/// [`new`](Self::new) takes values that *are* text (`&str`, `&String`), [`labeled`](Self::labeled)
/// values that carry it.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct ComboBox<'a, T, I, L> {
    widget: Widget,
    selected: &'a mut T,
    options: I,
    label: L,
    style: Option<&'a ComboBoxTheme>,
    button_style: Option<&'a ButtonTheme>,
    menu_style: Option<&'a MenuTheme>,
}

impl<'a, T, I> ComboBox<'a, T, I, fn(&T) -> &str>
where
    T: PartialEq + AsRef<str>,
    I: IntoIterator<Item = T> + Clone,
{
    /// A dropdown over options that are themselves text, `selected` holding one of them.
    #[track_caller]
    pub fn new(selected: &'a mut T, options: I) -> Self {
        Self::labeled(selected, options, T::as_ref)
    }
}

impl<'a, T, I, L> ComboBox<'a, T, I, L>
where
    T: PartialEq,
    I: IntoIterator<Item = T> + Clone,
    L: Fn(&T) -> &str,
{
    /// A dropdown over options that *carry* a label: `label` reads each one's text.
    /// For option types no `AsRef<str>` could serve, e.g. an enum or a record with an id beside
    /// a display name. `label` is any `Fn`, so it may capture a table.
    #[track_caller]
    pub fn labeled(selected: &'a mut T, options: I, label: L) -> Self {
        Self {
            // Alt+Down, the arrows, Home and End are `CARET`; the activation keys add `SPACE` and
            // `ENTER` in `show`.
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
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse {
        let mut response = self.widget.activation_response(ui, Activation::Button);
        let id = response.id;
        let list = ComboBoxList::menu_id(id);
        let shown = self
            .options
            .clone()
            .into_iter()
            .position(|option| option == *self.selected);
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
                && !TriggerRow::<ComboBoxList>::is_open(ui, id)
            {
                response.activate();
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
            .default_child_align(VAlign::Center)
            .default_gap(geometry.gap);

        let arrow_color = look.text.color;
        let text_style = look.text;
        // Interned: the label borrows from the caller's value, not `'static`.
        let label = ui.intern((self.label)(&*self.selected));

        self.widget.record(ui, Some(&look.chrome), |ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);

            let arrow = Widget::leaf()
                .id(id.with("arrow"))
                .size(geometry.arrow_size);
            arrow.record(ui, None, |ui| {
                let pts = geometry.chevron_points();
                ui.add_shape(
                    Shape::polyline(&pts),
                    Stroke::new(arrow_color, geometry.arrow_width)
                        .with_cap(LineCap::Round)
                        .with_join(LineJoin::Round),
                );
            });
        });

        let options = self.options;
        let label = self.label;
        let selected = self.selected;
        // A dropdown is at least as wide as its trigger, an explicit set that outranks
        // `MenuTheme::min_width`.
        let resp = Menu::below(&response)
            .id(list)
            .style(self.menu_style)
            .landing(landing)
            .min_size((response.rect.map_or(0.0, |rect| rect.size.w), 0.0))
            .show_as::<ComboBoxList, _>(ui, |ui| {
                let mut picked = false;
                for (idx, opt) in options.into_iter().enumerate() {
                    let lbl = ui.intern(label(&opt));
                    let row = MenuItem::new(lbl)
                        .id(option_id(list, idx))
                        .selected(shown == Some(idx));
                    if row.show(ui).clicked() && *selected != opt {
                        *selected = opt;
                        picked = true;
                    }
                }
                picked
            });
        let changed = resp.is_some_and(|resp| resp.inner);

        ValueResponse::pick(response, changed)
    }
}

/// The list of a [`ComboBox`], driven from outside by the combo's id: the row its open state
/// keeps, and the functions that open and close it. A type of its own, not generic over the
/// combo's options, so a call names no option type. The list records under
/// [`Self::menu_id`] of the combo's.
#[derive(Debug)]
pub struct ComboBoxList;

impl ComboBoxList {
    /// The list of the combo box `combo`. Its own derivation, not [`Menu::menu_id`] of the combo's,
    /// which a context menu an app puts on the combo takes: the two overlays open together and
    /// record under two ids.
    pub fn menu_id(combo: WidgetId) -> WidgetId {
        combo.with("combo-list")
    }

    /// Open the list of the combo box `id`, e.g. from a keyboard shortcut; it records on the
    /// combo's next `show`.
    pub fn open(ui: &mut Ui, id: WidgetId) {
        TriggerRow::<Self>::open(ui, id, Opening::Below);
    }

    /// Close the list of the combo box `id`, giving back the input it held at once. No-op if
    /// closed.
    pub fn close(ui: &mut Ui, id: WidgetId) {
        TriggerRow::<Self>::close(ui, id);
    }

    /// `true` while the list of the combo box `id` is open.
    pub fn is_open(ui: &Ui, id: WidgetId) -> bool {
        TriggerRow::<Self>::is_open(ui, id)
    }
}

/// The row of option `idx` in the list `list`. Keyed by index, so a keyboard open can land on
/// the pick before the row records.
fn option_id(list: WidgetId, idx: usize) -> WidgetId {
    list.with(("option", idx))
}

impl<T, I, L> Configure for ComboBox<'_, T, I, L> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
