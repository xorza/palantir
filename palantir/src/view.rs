 use crate::*;

pub trait View {
    fn style_mut(&mut self) -> &mut Style;
}

pub trait ItemsView {
    // fn add<T: View>(self, item: T) -> Self;
}
pub trait ItemView {
    // fn item<T: View>(self, item: T) -> Self;
}
