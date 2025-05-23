use crate::*;


#[derive(Debug, Default)]
pub struct VStack {
    style: Style,
}

impl VStack {
  
    pub fn add<T: View>(self, item: T) -> Self {
        self
    }
}

impl View for VStack {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl ItemsView for VStack {}

