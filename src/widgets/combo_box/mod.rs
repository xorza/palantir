//! The drop-down selector: a trigger that opens a popup list, and the
//! open/closed flag one trigger site keeps between frames.

use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::configure::Configure;
use crate::widgets::configure::ThemeDefaults;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::popup::Popup;
use crate::widgets::response::Response;
use crate::widgets::select_response::SelectResponse;
use crate::widgets::text::Text;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::widget_look::theme_slot::ThemeSlot;
use crate::widgets::widget::Widget;
use std::rc::Rc;

/// Open/closed flag for one combo site, keyed off the trigger id.
#[derive(Default, Clone, Copy, Debug)]
struct ComboState {
    open: bool,
}

/// A dropdown selector: a button-styled trigger showing the current
/// choice, which opens a [`crate::widgets::popup::Popup`] list of the
/// options on click. Picking a row sets the `&mut usize` selection and
/// closes; clicking outside or pressing Esc dismisses. Open/closed response
/// lives in the response map keyed off the trigger id, so the caller only
/// threads the selected index.
///
/// **`*selected` must index `options`.** Showing the current choice is
/// the trigger's whole contract and there is no placeholder response, so an
/// out-of-range index — including any index into an empty list — is a
/// caller bug and panics. A caller whose option list can shrink or be
/// replaced between frames owns re-deriving the index alongside it;
/// swallowing it here would render as an ordinary blank control.
///
/// The trigger chrome reuses [`crate::Theme::button`]; the list reuses
/// the context-menu panel + [`MenuItem`] rows
/// ([`crate::Theme::context_menu`]).
///
/// `options` is the caller's own collection, handed over rather than
/// projected: [`new`](Self::new) takes a slice whose elements *are* text
/// (`&[&str]`, `&[String]`, `&[Cow<'_, str>]`) and
/// [`labeled`](Self::labeled) one whose elements merely carry it. Either
/// way nothing is copied to open a combo, and a closed one — nearly every
/// frame — reads exactly one label.
#[derive(Debug)]
pub struct ComboBox<'a, S> {
    widget: Widget,
    selected: &'a mut usize,
    options: &'a [S],
    /// Reads one option's label. `new` fills this with `S::as_ref`.
    label: fn(&S) -> &str,
    style: Option<&'a ButtonTheme>,
}

impl<'a, S: AsRef<str>> ComboBox<'a, S> {
    /// A dropdown over options that are themselves text.
    #[track_caller]
    pub fn new(selected: &'a mut usize, options: &'a [S]) -> Self {
        Self::labeled(selected, options, S::as_ref)
    }
}

impl<'a, S> ComboBox<'a, S> {
    /// A dropdown over rows that *carry* a label rather than being one:
    /// `label` reads each row's text.
    ///
    /// For an option type no `AsRef<str>` impl could serve — a record with
    /// an id beside a display name, where picking between the two is the
    /// call site's business, not the type's.
    ///
    /// A plain `fn` pointer rather than a closure keeps `ComboBox`
    /// non-generic over the projection; every real label is a field read.
    #[track_caller]
    pub fn labeled(selected: &'a mut usize, options: &'a [S], label: fn(&S) -> &str) -> Self {
        Self {
            widget: Widget::hstack().sense(Sense::CLICK),
            selected,
            options,
            label,
            style: None,
        }
    }

    style_setter!(
        'a,
        ButtonTheme,
        button,
        "Restyles the trigger chrome. The dropdown reads \
         [`crate::Theme::context_menu`], and the arrow geometry \
         [`crate::Theme::combo_box`].",
    );

    pub fn show(mut self, ui: &mut Ui) -> SelectResponse<'_> {
        let response = self.widget.response(ui);
        let id = self.widget.resolve(ui);

        // Trigger chrome from the button theme (same flow as `Button`).
        // One handle covers both reads: the geometry is read again inside
        // the `record` closure below, which owns `ui` mutably.
        let theme = Rc::clone(ui.theme());
        let slot = self.slot(&theme);
        let look = slot
            .plan(&response, (), theme.text)
            .apply(ui, &mut self.widget);

        let geom = &theme.combo_box;
        self.widget
            .configure()
            .justify(Justify::SpaceBetween)
            .child_align(Align::v(VAlign::Center))
            .gap(geom.gap);

        let arrow_color = look.text.color;
        let text_style = look.text;
        let Some(option) = self.options.get(*self.selected) else {
            panic!(
                "ComboBox selection {} is out of range for {} option(s)",
                self.selected,
                self.options.len(),
            )
        };
        let chosen = (self.label)(option);
        // Intern the selected label into the frame buffer — an option
        // borrows from the caller's collection rather than from `'static`,
        // so it routes through `Ui::intern`.
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
                    Shape::polyline(&pts, PolylineColors::Single(arrow_color), geom.arrow_stroke)
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            });
        });

        let trigger_rect = response.rect;
        // Probed, not inserted: a combo box spends nearly every frame closed,
        // and a closed one is the default — so an unopened trigger keeps no
        // row at all, and the write-back below happens only on a real flip.
        let was_open = ui
            .try_state::<ComboState>(id)
            .is_some_and(|state| state.open);
        let mut open = was_open;
        let mut changed = false;
        if !response.disabled && response.left.clicked() {
            open = !open;
        }
        // Esc closes via the `Dismiss` popup's `resp.closed()` below — no
        // separate `escape_pressed` here.

        if open && let Some(rect) = trigger_rect {
            let ctx = &theme.context_menu;
            let options = self.options;
            let label = self.label;
            let selected = self.selected;
            // The same menu theme `ContextMenu` fills its popup in from,
            // so the two read as one control with two triggers. The one
            // deliberate difference is the minimum: a dropdown is at
            // least as wide as the trigger it drops from, which is an
            // explicit set and so outranks `ContextMenuTheme::min_width`.
            let popup = Popup::below(rect)
                .id(id.with("list"))
                .min_size((rect.size.w, 0.0))
                .default_background(&ctx.panel)
                .default_padding(ctx.padding)
                .default_gap(ctx.gap);
            let resp = popup.show(ui, |ui, popup| {
                let mut picked = false;
                for (i, opt) in options.iter().enumerate() {
                    let lbl = ui.intern(label(opt));
                    if MenuItem::new(lbl).show(ui, popup).left.clicked() && *selected != i {
                        *selected = i;
                        picked = true;
                    }
                }
                picked
            });
            changed = resp.inner;
            if resp.closed() {
                open = false;
            }
        }
        if open != was_open {
            ui.state_mut::<ComboState>(id).open = open;
        }

        SelectResponse {
            response: Response::eager(id, ui, response),
            changed,
        }
    }
}

impl_configure!(<S> ComboBox<'_, S>);

#[cfg(test)]
mod tests;
