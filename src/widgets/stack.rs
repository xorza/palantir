use crate::primitives::{Sizes, Sizing, Spacing, Style, WidgetId};
use crate::tree::LayoutKind;
use crate::ui::Ui;
use crate::widgets::Response;
use std::hash::Hash;

pub struct Stack {
    id: WidgetId,
    kind: LayoutKind,
    size: Sizes,
    padding: Spacing,
    margin: Spacing,
}

impl Stack {
    fn new(id: impl Hash, kind: LayoutKind) -> Self {
        Self {
            id: WidgetId::from_hash(id),
            kind,
            size: Sizes::HUG,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
        }
    }

    pub fn width(mut self, v: impl Into<Sizing>) -> Self { self.size.w = v.into(); self }
    pub fn height(mut self, v: impl Into<Sizing>) -> Self { self.size.h = v.into(); self }
    pub fn padding(mut self, p: Spacing) -> Self { self.padding = p; self }
    pub fn margin(mut self, m: Spacing) -> Self { self.margin = m; self }

    pub fn show(self, ui: &mut Ui, f: impl FnOnce(&mut Ui)) -> Response {
        let style = Style {
            size: self.size,
            padding: self.padding,
            margin: self.margin,
        };
        let node = ui.node(self.id, style, self.kind, f);
        Response { node }
    }
}

pub struct HStack;
pub struct VStack;

impl HStack {
    pub fn new(id: impl Hash) -> Stack { Stack::new(id, LayoutKind::HStack) }
}

impl VStack {
    pub fn new(id: impl Hash) -> Stack { Stack::new(id, LayoutKind::VStack) }
}
