use crate::primitives::{Sense, Size, Sizes, Spacing, Style, WidgetId};
use crate::tree::LayoutKind;
use crate::ui::Ui;
use crate::widgets::Response;
use std::hash::Hash;

pub struct Stack {
    id: WidgetId,
    kind: LayoutKind,
    size: Sizes,
    min_size: Size,
    max_size: Size,
    padding: Spacing,
    margin: Spacing,
    sense: Sense,
}

impl Stack {
    fn from_id(id: WidgetId, kind: LayoutKind) -> Self {
        Self {
            id,
            kind,
            size: Sizes::HUG,
            min_size: Size::ZERO,
            max_size: Size::INF,
            padding: Spacing::ZERO,
            margin: Spacing::ZERO,
            sense: Sense::NONE,
        }
    }

    pub fn size(mut self, s: impl Into<Sizes>) -> Self {
        self.size = s.into();
        self
    }
    pub fn min_size(mut self, s: impl Into<Size>) -> Self {
        self.min_size = s.into();
        self
    }
    pub fn max_size(mut self, s: impl Into<Size>) -> Self {
        self.max_size = s.into();
        self
    }
    pub fn padding(mut self, p: impl Into<Spacing>) -> Self {
        self.padding = p.into();
        self
    }
    pub fn margin(mut self, m: impl Into<Spacing>) -> Self {
        self.margin = m.into();
        self
    }
    /// Make the stack itself an interaction target (clickable card, drag handle, etc).
    /// Default is `Sense::NONE` so containers don't intercept clicks meant for children.
    pub fn sense(mut self, s: Sense) -> Self {
        self.sense = s;
        self
    }

    pub fn show(&self, ui: &mut Ui, f: impl FnOnce(&mut Ui)) -> Response {
        let style = Style {
            size: self.size,
            min_size: self.min_size,
            max_size: self.max_size,
            padding: self.padding,
            margin: self.margin,
        };
        let node = ui.node(self.id, style, self.kind, self.sense, f);
        let state = ui.response_for(self.id);
        Response { node, state }
    }
}

pub struct HStack;
pub struct VStack;

#[allow(clippy::new_ret_no_self)]
impl HStack {
    #[track_caller]
    pub fn new() -> Stack {
        Stack::from_id(WidgetId::auto_stable(), LayoutKind::HStack)
    }
    pub fn with_id(id: impl Hash) -> Stack {
        Stack::from_id(WidgetId::from_hash(id), LayoutKind::HStack)
    }
}

#[allow(clippy::new_ret_no_self)]
impl VStack {
    #[track_caller]
    pub fn new() -> Stack {
        Stack::from_id(WidgetId::auto_stable(), LayoutKind::VStack)
    }
    pub fn with_id(id: impl Hash) -> Stack {
        Stack::from_id(WidgetId::from_hash(id), LayoutKind::VStack)
    }
}
