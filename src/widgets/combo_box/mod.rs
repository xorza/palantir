use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::shape::Shape;
use crate::shape::polyline::PolylineColors;
use crate::shape::style::{LineCap, LineJoin};
use crate::ui::Ui;
use crate::widgets::context_menu::MenuItem;
use crate::widgets::popup::{ClickOutside, Popup};
use crate::widgets::text::Text;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::resolve_look;
use crate::widgets::{Response, enter_widget};
use glam::Vec2;

/// Down-chevron arrow box (logical px). Drawn as a polyline so it's
/// font-independent.
const ARROW_W: f32 = 10.0;
const ARROW_H: f32 = 6.0;

/// Open/closed flag for one combo site, keyed off the trigger id.
#[derive(Default, Clone, Copy, Debug)]
struct ComboState {
    open: bool,
}

/// A dropdown selector: a button-styled trigger showing the current
/// choice, which opens a [`crate::widgets::popup::Popup`] list of the
/// options on click. Picking a row sets the `&mut usize` selection and
/// closes; clicking outside or pressing Esc dismisses. Open/closed state
/// lives in the state map keyed off the trigger id, so the caller only
/// threads the selected index.
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

    /// Borrow a trigger chrome theme override. The default inherits
    /// [`crate::Theme::button`].
    pub fn style(mut self, s: &'a ButtonTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut entry = enter_widget(ui, self.node);
        let id = entry.widget.id();

        // Trigger chrome from the button theme (same flow as `Button`).
        let look = resolve_look(
            ui,
            id,
            &mut entry.widget.node,
            &entry.state,
            self.style,
            |t| &t.button,
        );

        let node = &mut entry.widget.node;
        node.justify = Justify::SpaceBetween;
        node.child_align = Align::v(VAlign::Center);
        node.gaps.set_gap(12.0);

        let arrow_color = look.text.color;
        let text_style = look.text;
        // Intern the selected label into the frame buffer — `&'a str`
        // options aren't `'static`, so they route through `Ui::intern`.
        let label = ui.intern(self.options.get(*self.selected).copied().unwrap_or(""));

        entry.widget.record(ui, Some(&look.background), |ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(&text_style)
                .show(ui);

            let arrow = Node::leaf()
                .id(id.with("arrow"))
                .size((Sizing::fixed(ARROW_W), Sizing::fixed(ARROW_H)));
            ui.widget(arrow).record(ui, None, |ui| {
                let pts = chevron_pts();
                ui.add_shape(
                    Shape::polyline(&pts, PolylineColors::Single(arrow_color), 1.5)
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            });
        });

        let trigger_rect = entry.state.rect;
        let mut open = ui.state_mut::<ComboState>(id).open;
        if !entry.state.disabled && entry.state.left.clicked() {
            open = !open;
        }
        // Esc closes via the `Dismiss` popup's `resp.closed()` below — no
        // separate `escape_pressed` here.

        if open && let Some(rect) = trigger_rect {
            let panel = ui.theme.context_menu.panel.clone();
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
        ui.state_mut::<ComboState>(id).open = open;

        entry.into_response(ui)
    }
}

impl Configure for ComboBox<'_> {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

/// Down-pointing chevron (`v`) in the `ARROW_W × ARROW_H` box, in
/// node-local coords.
fn chevron_pts() -> [Vec2; 3] {
    [
        Vec2::new(0.0, 0.0),
        Vec2::new(ARROW_W * 0.5, ARROW_H),
        Vec2::new(ARROW_W, 0.0),
    ]
}

#[cfg(test)]
mod tests;
