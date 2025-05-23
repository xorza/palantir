use crate::*;

#[derive(Debug, Default)]
pub struct Button {
    style: Style,
}

impl Button {
    pub fn onclick<F: FnOnce() -> ()>(self, _f: F) -> Self {
        self
    }
    pub fn item<T: View>(self, item: T) -> Self {
        self
    }
}

impl View for Button {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}
impl ItemView for Button {}
