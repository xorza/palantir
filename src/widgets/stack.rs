use crate::primitives::{Sizes, Spacing, Style, WidgetId};
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
    fn from_id(id: WidgetId, kind: LayoutKind) -> Self {
        Self {
            id,
            kind,
            size: Sizes::HUG,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
        }
    }

    pub fn size(mut self, s: impl Into<Sizes>) -> Self { self.size = s.into(); self }
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
    #[track_caller]
    pub fn new() -> Stack { Stack::from_id(WidgetId::auto_stable(), LayoutKind::HStack) }
    pub fn with_id(id: impl Hash) -> Stack {
        Stack::from_id(WidgetId::from_hash(id), LayoutKind::HStack)
    }
}

impl VStack {
    #[track_caller]
    pub fn new() -> Stack { Stack::from_id(WidgetId::auto_stable(), LayoutKind::VStack) }
    pub fn with_id(id: impl Hash) -> Stack {
        Stack::from_id(WidgetId::from_hash(id), LayoutKind::VStack)
    }
}
