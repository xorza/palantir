use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, VAlign};
use crate::layout::types::justify::Justify;
use crate::layout::types::sizing::Sizing;
use crate::shape::{LineCap, LineJoin, PolylineColors, Shape};
use crate::ui::Ui;
use crate::widgets::context_menu::MenuItem;
use crate::widgets::popup::{ClickOutside, Popup};
use crate::widgets::text::Text;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::{Response, WidgetEntry, button_look, enter_widget};
use glam::Vec2;

/// Down-chevron arrow box (logical px). Drawn as a polyline so it's
/// font-independent.
const ARROW_W: f32 = 10.0;
const ARROW_H: f32 = 6.0;

/// Open/closed flag for one combo site, keyed off the trigger id.
#[derive(Default, Clone, Copy)]
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
pub struct ComboBox<'a> {
    element: Element,
    selected: &'a mut usize,
    options: &'a [&'a str],
    style: Option<ButtonTheme>,
}

impl<'a> ComboBox<'a> {
    #[track_caller]
    pub fn new(selected: &'a mut usize, options: &'a [&'a str]) -> Self {
        let mut element = Element::new(LayoutMode::HStack);
        element.flags.set_sense(Sense::CLICK);
        Self {
            element,
            selected,
            options,
            style: None,
        }
    }

    /// Override the trigger chrome theme. `None` (default) inherits
    /// [`crate::Theme::button`].
    pub fn style(mut self, s: ButtonTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut element = self.element;
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: state,
        } = enter_widget(ui, &element);

        // Trigger chrome from the button theme (same flow as `Button`).
        let look = button_look(ui, id, &mut element, state, self.style.as_ref());

        element.justify = Justify::SpaceBetween;
        element.child_align = Align::v(VAlign::Center);
        element.gaps.set_gap(12.0);

        let arrow_color = look.text.color;
        let text_style = look.text;
        // Intern the selected label into the frame buffer — `&'a str`
        // options aren't `'static`, so they route through `Ui::intern`.
        let label = ui.intern(self.options.get(*self.selected).copied().unwrap_or(""));

        ui.node(id, element, Some(&look.background), |ui| {
            Text::new(label)
                .id(id.with("label"))
                .style(text_style)
                .show(ui);

            let arrow_id = id.with("arrow");
            let mut arrow = Element::new(LayoutMode::Leaf);
            arrow.salt = Salt::Verbatim(arrow_id);
            arrow.size = (Sizing::Fixed(ARROW_W), Sizing::Fixed(ARROW_H)).into();
            ui.node(arrow_id, arrow, None, |ui| {
                let pts = chevron_pts();
                ui.add_shape(
                    Shape::polyline(&pts, PolylineColors::Single(arrow_color), 1.5)
                        .cap(LineCap::Round)
                        .join(LineJoin::Round),
                );
            });
        });

        let trigger_rect = state.rect;
        let mut open = ui.state_mut::<ComboState>(id).open;
        if !state.disabled && raw_state.left.clicked {
            open = !open;
        }
        // Esc closes via the `Dismiss` popup's `resp.closed()` below — no
        // separate `escape_pressed` here.

        if open && let Some(rect) = trigger_rect {
            let panel = ui.theme.context_menu.panel.clone();
            let anchor = Vec2::new(rect.min.x, rect.min.y + rect.size.h);
            let options = self.options;
            let selected = self.selected;
            let mut popup = Popup::anchored_to(anchor)
                .click_outside(ClickOutside::Dismiss)
                .background(panel)
                .id(id.with("list"));
            popup.element_mut().min_size.w = rect.size.w;
            let resp = popup.show(ui, |ui, popup| {
                for (i, opt) in options.iter().enumerate() {
                    let lbl = ui.intern(opt);
                    if MenuItem::new(lbl).show(ui, popup).clicked() {
                        *selected = i;
                    }
                }
            });
            if resp.closed() {
                open = false;
            }
        }
        ui.state_mut::<ComboState>(id).open = open;

        Response::eager(id, ui, raw_state)
    }
}

impl Configure for ComboBox<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
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
