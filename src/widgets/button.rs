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
            label_wrap: TextWrap::SingleLine,
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
    /// Default [`TextWrap::SingleLine`] (hard-cut to one line); pass
    /// [`TextWrap::Ellipsis`] to mark the cut with `…`, [`TextWrap::WrapWithOverflow`] to
    /// reflow onto multiple lines, or [`TextWrap::Overflow`] to let it run
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
        picked_state.disabled |= element.flags.is_disabled();
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
        let look_target = style.pick(picked_state).clone();
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
        let label = self.label;
        let label_align = self.label_align;
        let label_wrap = self.label_wrap;

        ui.node_with_chrome(id, element, &look.background, |ui| {
            if !label.is_empty() {
                ui.add_shape(Shape::Text {
                    local_origin: None,
                    text: label,
                    brush: look.text.color.into(),
                    font_size_px: look.text.font_size_px,
                    line_height_px: look.line_height_px(),
                    // `SingleLine` by default so an over-wide label is cut to
                    // one line instead of spilling outside the chrome; see the
                    // `.text_wrap(TextWrap::Ellipsis)` / `.text_wrap(TextWrap::WrapWithOverflow)` / `.text_wrap(TextWrap::Overflow)` builders.
                    wrap: label_wrap,
                    align: label_align,
                    family: look.text.family,
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
