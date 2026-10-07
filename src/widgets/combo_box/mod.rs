//! The drop-down selector: a trigger that opens a popup list, and the
//! open/closed flag one trigger site keeps between frames.

use crate::input::interaction::button_phase::ButtonPhase;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::sense::Sense;
use crate::input::shortcut::{Shortcut, ShortcutMods};
use crate::primitives::layout::align::{Align, VAlign};
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::justify::Justify;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain;
use crate::primitives::paint::stroke::Stroke;
use crate::shape::Shape;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::configure::ThemeDefaults;
use crate::widget_core::response::{Response, ResponseSnapshot};
use crate::widget_core::value_response::ValueResponse;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::theme_slot::ThemeSlot;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::popup::popup_trigger::PopupTrigger;
use crate::widgets::text::Text;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::combo_box::ComboBoxTheme;
use std::rc::Rc;

/// A dropdown selector: a button-styled trigger showing the current choice,
/// opening a [`crate::widgets::popup::Popup`] list on click. Picking a row sets
/// the `&mut usize` selection and closes; clicking outside or Esc dismisses.
/// Open state lives in the response map keyed off the trigger id.
///
/// `*selected` is an *index* coerced for display: past the end of `options`
/// shows the last option, and an empty list shows an empty trigger. The bound
/// index is rewritten only when the user picks.
///
/// The trigger reuses [`crate::Theme::button`]; the list reuses the context-menu
/// panel and [`MenuItem`] rows ([`crate::Theme::context_menu`]).
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
            // A Tab stop: Space, Enter and Alt+Down open it, and arrows step the pick
            // while closed; `TEXT` and `CARET` are the key classes involved.
            widget: Widget::hstack()
                .sense(Sense::CLICK)
                .focusable(true)
                .input_scope(KeyFilter::TEXT.union(KeyFilter::CARET)),
            selected,
            options,
            label,
            style: None,
            button_style: None,
        }
    }

    /// Per-instance override of [`crate::Theme`]'s `combo_box`, restyling the
    /// widget's own geometry (label/chevron gutter and chevron). Takes an `Option`
    /// as readily as a reference. [`Self::button_style`] restyles the trigger chrome
    /// and the dropdown reads [`crate::Theme::context_menu`].
    pub fn style(mut self, s: impl Into<Option<&'a ComboBoxTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `button`, which the trigger
    /// paints as. Separate from [`Self::style`] because the combo is a button plus a
    /// context menu.
    pub fn button_style(mut self, s: impl Into<Option<&'a ButtonTheme>>) -> Self {
        self.button_style = s.into();
        self
    }

    /// Record the trigger, and the dropdown when open.
    ///
    /// The [`ValueResponse`]'s own `response` is the trigger's; read `changed` for
    /// the pick, which commits at once. Focused, it takes the keys of WAI-ARIA's
    /// select-only combobox: Space, Enter and Alt+Down open, and while closed the
    /// Up and Down arrows step the pick, stopping at the ends.
    pub fn show(mut self, ui: &mut Ui) -> ValueResponse<'_> {
        let mut response = self.widget.response(ui);
        let id = self.widget.resolve(ui);
        let mut stepped = false;
        if !response.disabled && ui.is_focus_within(id) {
            // Every chord is sampled: `key_pressed` also keeps it subscribed for the wake gate.
            let mut key = |shortcut| self.widget.key_pressed(ui, shortcut);
            let space = key(Shortcut::key(Key::Char(' ')));
            let enter = key(Shortcut::key(Key::Enter));
            let alt_down = key(Shortcut::new(ShortcutMods::ALT, Key::ArrowDown));
            let up = key(Shortcut::key(Key::ArrowUp));
            let down = key(Shortcut::key(Key::ArrowDown));
            if space || enter || alt_down {
                response.left.phase = ButtonPhase::Up { click: Some(1) };
            } else if !PopupTrigger::is_open(ui, id)
                && let Some(shown) = domain::index(*self.selected, self.options.len())
            {
                let last = self.options.len() - 1;
                let next = if down {
                    (shown + 1).min(last)
                } else if up {
                    shown.saturating_sub(1)
                } else {
                    shown
                };
                if (up || down) && next != *self.selected {
                    *self.selected = next;
                    stepped = true;
                }
            }
        }

        // Trigger chrome from the button theme, as in `Button`. One handle covers both
        // reads; the `record` closure below owns `ui` mutably.
        let theme = Rc::clone(ui.theme());
        let slot = self.button_style.unwrap_or(&theme.button);
        let look = slot
            .plan(&response, (), theme.text)
            .apply(ui, &mut self.widget);

        let geom = self.style.unwrap_or(&theme.combo_box);
        self.widget
            .configure()
            .justify(Justify::SpaceBetween)
            .child_align(Align::v(VAlign::Center))
            .gap(geom.gap);

        let arrow_color = look.text.color;
        let text_style = look.text;
        let chosen = domain::index(*self.selected, self.options.len())
            .map_or("", |shown| (self.label)(&self.options[shown]));
        // Intern the selected label: options borrow from the caller's collection, not
        // `'static`.
        let label = ui.intern(chosen);

        self.widget.record(ui, Some(&look.background), |ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);

            let arrow = Widget::leaf().id(id.with("arrow")).size((
                Sizing::fixed(geom.arrow_size.x),
                Sizing::fixed(geom.arrow_size.y),
            ));
            arrow.record(ui, None, |ui| {
                let pts = geom.chevron_pts();
                ui.add_shape(
                    Shape::polyline(&pts, Stroke::new(arrow_color, geom.arrow_width))
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            });
        });

        let ctx = &theme.context_menu;
        let options = self.options;
        let label = self.label;
        let selected = self.selected;
        let trigger = ResponseSnapshot {
            id,
            state: response,
        };
        // The same menu theme as `ContextMenu`, except the minimum: a dropdown is at
        // least as wide as its trigger, an explicit set that outranks
        // `ContextMenuTheme::min_width`. Esc closes through the popup.
        let resp = PopupTrigger::on(&trigger)
            .id(id.with("list"))
            .arrow_focus(Axis::Y)
            .min_size((response.rect.map_or(0.0, |rect| rect.size.w), 0.0))
            .default_background(ctx.panel.clone())
            .default_padding(ctx.padding)
            .default_gap(ctx.gap)
            .show(ui, |ui, popup| {
                let mut picked = false;
                for (i, opt) in options.iter().enumerate() {
                    let lbl = ui.intern(label(opt));
                    if MenuItem::new(lbl).show(ui, popup).clicked() && *selected != i {
                        *selected = i;
                        picked = true;
                    }
                }
                picked
            });
        let changed = resp.inner.unwrap_or(false) || stepped;

        ValueResponse {
            response: Response::new(id, ui, response),
            changed,
            committed: changed,
        }
    }
}

impl<S, L> Configure for ComboBox<'_, S, L> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
