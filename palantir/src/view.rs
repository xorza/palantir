

use std::fmt::Debug;

use crate::*;

pub trait View {
    fn style_mut(&mut self) -> &mut Style;
}

impl Debug for dyn View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View").finish()
    }
}

pub trait ItemsView : View {
    fn items(&self) -> impl Iterator<Item = &dyn View>;
}
pub trait ItemView : View {
    fn item(&self) -> &dyn View;
}
