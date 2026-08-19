use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::scene::node::{Configure, Node};
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::popup::{ClickOutside, Popup};
use crate::widgets::response::Response;
use crate::widgets::text::Text;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::widget_look::look_plan::LookPlan;

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
#[derive(Debug)]
pub struct ComboBox<'a> {
    node: Node,
    selected: &'a mut usize,
    options: &'a [&'a str],
    style: Option<&'a ButtonTheme>,
}

impl<'a> ComboBox<'a> {
    #[track_caller]
    pub fn new(selected: &'a mut usize, options: &'a [&'a str]) -> Self {
        let mut node = Node::hstack();
        node.flags.set_sense(Sense::CLICK);
        Self {
            node,
            selected,
            options,
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

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut widget = ui.widget(self.node);
        let response = widget.response(ui);
        let id = widget.id();

        // Trigger chrome from the button theme (same flow as `Button`).
        let theme = ui.theme();
        let slot = self.slot(theme);
        let look = LookPlan {
            target: slot.pick(&response).to_animated(&theme.text),
            padding: slot.padding,
            margin: slot.margin,
            anim: slot.anim,
        }
        .apply(ui, &mut widget);

        // Handle: the geometry is read again inside the `record` closure
        // below, which owns `ui` mutably.
        let ui_theme = ui.theme().clone();
        let geom = &ui_theme.combo_box;
        let node = &mut widget.node;
        node.justify = Justify::SpaceBetween;
        node.child_align = Align::v(VAlign::Center);
        node.gaps.set_gap(geom.row_gap);

        let arrow_color = look.text.color;
        let text_style = look.text;
        let chosen = self
            .options
            .get(*self.selected)
            .copied()
            .unwrap_or_else(|| {
                panic!(
                    "ComboBox selection {} is out of range for {} option(s)",
                    self.selected,
                    self.options.len(),
                )
            });
        // Intern the selected label into the frame buffer — `&'a str`
        // options aren't `'static`, so they route through `Ui::intern`.
        let label = ui.intern(chosen);

        widget.record(ui, Some(&look.background), |ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);

            let arrow = Node::leaf().id(id.with("arrow")).size((
                Sizing::fixed(geom.arrow_size.x),
                Sizing::fixed(geom.arrow_size.y),
            ));
            ui.widget(arrow).record(ui, None, |ui| {
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
        if !response.disabled && response.left.clicked() {
            open = !open;
        }
        // Esc closes via the `Dismiss` popup's `resp.closed()` below — no
        // separate `escape_pressed` here.

        if open && let Some(rect) = trigger_rect {
            let panel = ui_theme.context_menu.panel.clone();
            let options = self.options;
            let selected = self.selected;
            let popup = Popup::below(rect)
                .click_outside(ClickOutside::Dismiss)
                .background(panel)
                .id(id.with("list"))
                .min_size((rect.size.w, 0.0));
            let resp = popup.show(ui, |ui, popup| {
                for (i, opt) in options.iter().enumerate() {
                    let lbl = ui.intern(opt);
                    if MenuItem::new(lbl).show(ui, popup).left.clicked() {
                        *selected = i;
                    }
                }
            });
            if resp.closed() {
                open = false;
            }
        }
        if open != was_open {
            ui.state_mut::<ComboState>(id).open = open;
        }

        Response::eager(id, ui, response)
    }
}

impl_configure!(ComboBox<'_>);

#[cfg(test)]
mod tests;
