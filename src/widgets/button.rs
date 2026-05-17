use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::sense::Sense;
use crate::layout::types::align::Align;
use crate::primitives::interned_str::InternedStr;
use crate::primitives::spacing::Spacing;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::button::ButtonTheme;

pub struct Button {
    element: Element,
    style: Option<ButtonTheme>,
    label: InternedStr<'static>,
    label_align: Align,
}

impl Button {
    #[allow(clippy::new_without_default)]
    #[track_caller]
    pub fn new() -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.set_sense(Sense::CLICK);
        Self {
            element,
            style: None,
            label: InternedStr::default(),
            // Buttons center their labels by convention. Override with
            // `.text_align(...)` for left/right-aligned labels.
            label_align: Align::CENTER,
        }
    }

    pub fn style(mut self, s: ButtonTheme) -> Self {
        self.style = Some(s);
        self
    }
    pub fn label(mut self, s: impl Into<InternedStr<'static>>) -> Self {
        self.label = s.into();
        self
    }

    /// Position of the label glyphs inside the button's arranged rect.
    /// Distinct from [`Configure::align`], which positions the *button*
    /// inside its parent's slot. Default: [`Align::CENTER`].
    pub fn text_align(mut self, a: Align) -> Self {
        self.label_align = a;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let mut element = self.element;
        // Resolve `.id_salt(...)`'s parent-scoping now so per-id
        // state lookups (response_for, animate) below see the same
        // `WidgetId` `Forest::open_node` will record.
        let id = ui.make_persistent_id(element.salt);
        // One `response_for` call covers both theme-picking (with
        // self-disabled merged in) and the returned `Response.state`
        // (without the merge). The button's `ui.node` body doesn't
        // mutate input state, so a re-read after `node` would return
        // the same `ResponseState` minus the merge.
        let raw_state = ui.response_for(id);
        let mut picked_state = raw_state;
        // Cascade lags by a frame; OR self-disabled in so a freshly
        // toggled `.disabled(true)` lands disabled visuals immediately.
        picked_state.disabled |= element.is_disabled();
        let fallback_text = ui.theme.text;
        // Borrow either the user override or the default theme without
        // cloning the ~540-byte `ButtonTheme`. Copy out the four
        // scalars we need (padding/margin/anim + picked `WidgetLook`)
        // so the borrow on `ui.theme` ends before `animate(ui, ..)`
        // reborrows `ui` mutably.
        let style: &ButtonTheme = self.style.as_ref().unwrap_or(&ui.theme.button);
        let style_padding = style.padding;
        let style_margin = style.margin;
        let style_anim = style.anim;
        let look_target = *style.pick(picked_state);
        // Apply theme padding/margin when the builder hasn't set
        // anything (sentinel: `Spacing::ZERO` == "use theme"). User
        // overrides — anything non-zero set via `.padding(...)` /
        // `.margin(...)` — pass through unchanged.
        if element.padding == Spacing::ZERO {
            element.padding = style_padding;
        }
        if element.margin == Spacing::ZERO {
            element.margin = style_margin;
        }
        let look = look_target.animate(ui, id, fallback_text, style_anim);
        let chrome = look.background;
        let label = self.label;
        let label_align = self.label_align;

        ui.node_with_chrome(id, element, chrome, |ui| {
            if !label.is_empty() {
                ui.add_shape(Shape::Text {
                    local_origin: None,
                    text: label,
                    brush: look.text.color.into(),
                    font_size_px: look.text.font_size_px,
                    line_height_px: look.line_height_px(),
                    wrap: TextWrap::Single,
                    align: label_align,
                    family: look.text.family,
                });
            }
        });
        Response {
            id,
            state: raw_state,
        }
    }
}

impl Configure for Button {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
