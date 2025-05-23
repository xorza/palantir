use crate::*;

#[derive(Debug, Default)]
pub struct Button {
    style: Style,
    item: Option<Box<dyn View>>,
}

impl Button {
    pub fn onclick<F: FnOnce() -> ()>(self, _f: F) -> Self {
        self
    }
    pub fn item<T>(mut self, item: T) -> Self
    where
        T: View + 'static,
    {
        self.item = Some(Box::new(item));
        self
    }
}

impl View for Button {
    fn get_style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl ItemView for Button {
    fn item(&self) -> &dyn View {
        self
    }
}
