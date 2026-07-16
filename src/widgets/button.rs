use crate::forest::element::{Configure, Element};
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::layout::types::layout_mode::LayoutMode;
use crate::primitives::interned_str::InternedStr;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::theme::button::ButtonTheme;
use crate::widgets::theme::resolve_look;
use crate::widgets::{Response, WidgetEntry, enter_widget};

#[derive(Debug)]
pub struct Button {
    element: Element,
    style: Option<ButtonTheme>,
    label: InternedStr,
    label_align: Align,
    label_wrap: TextWrap,
}

impl Button {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.flags.set_sense(Sense::CLICK);
        Self {
            element,
            style: None,
            label: InternedStr::default(),
            // Buttons center their labels by convention. Override with
            // `.text_align(...)` for left/right-aligned labels.
            label_align: Align::CENTER,
            // Single-line by default — a button hugs its label, so truncation
            // only bites when the caller commits a narrower width than the
            // label's natural line (Fixed/Fill button); then the label is cut
            // to fit instead of spilling outside the chrome. Override the mode
            // via `.text_wrap(...)`.
            label_wrap: TextWrap::Truncate,
        }
    }

    pub fn style(mut self, s: ButtonTheme) -> Self {
        self.style = Some(s);
        self
    }
    pub fn label(mut self, s: impl Into<InternedStr>) -> Self {
        self.label = s.into();
        self
    }

    /// Set how the label handles a width narrower than its natural line.
    /// Default [`TextWrap::Truncate`] (hard-cut to one line, no marker); pass
    /// [`TextWrap::Ellipsis`] to mark the cut with `…`, [`TextWrap::WrapWithOverflow`] to
    /// reflow onto multiple lines, or [`TextWrap::SingleLine`] to let it run
    /// past the chrome. Only bites on a `Fixed`/`Fill`-width button — a `Hug`
    /// button commits its natural width, so the label always fits.
    pub fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.label_wrap = wrap;
        self
    }

    /// Position of the label glyphs inside the button's arranged rect.
    /// Distinct from [`Configure::align`], which positions the *button*
    /// inside its parent's slot. Default: [`Align::CENTER`].
    pub fn text_align(mut self, a: Align) -> Self {
        self.label_align = a;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response<'_> {
        let mut element = self.element;
        // `picked_state` (self-disabled merged) drives theme picking;
        // `raw_state` feeds the eager `Response` — the button's
        // `ui.node` body doesn't mutate input, so the pre-`node` probe
        // stays valid for the caller. See `enter_widget`.
        let WidgetEntry {
            id,
            raw: raw_state,
            merged: picked_state,
        } = enter_widget(ui, &element);
        let look = resolve_look(
            ui,
            id,
            &mut element,
            picked_state,
            self.style.as_ref(),
            |t| &t.button,
        );
        let label = self.label;
        let label_align = self.label_align;
        let label_wrap = self.label_wrap;

        ui.node(id, element, Some(&look.background), |ui| {
            if !label.is_empty() {
                ui.add_shape(Shape::Text {
                    local_origin: None,
                    text: label,
                    color: look.text.color,
                    font_size_px: look.text.font_size_px,
                    line_height_px: look.line_height_px(),
                    // `Truncate` by default so an over-wide label is cut to
                    // one line instead of spilling outside the chrome; see the
                    // `.text_wrap(TextWrap::Ellipsis)` / `.text_wrap(TextWrap::WrapWithOverflow)` / `.text_wrap(TextWrap::SingleLine)` builders.
                    wrap: label_wrap,
                    align: label_align,
                    family: look.text.family,
                    weight: look.text.weight,
                });
            }
        });
        // Eager: theme picking already paid for `response_for`, so
        // hand the cached state to the caller.
        Response::eager(id, ui, raw_state)
    }
}

impl Configure for Button {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
