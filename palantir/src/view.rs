use std::{fmt::Debug, mem::swap};

use crate::*;

pub trait View {
    fn get_style_mut(&mut self) -> &mut Style;
    fn style(mut self, mut style: Style) -> Self
    where
        Self: Sized,
    {
        swap(self.get_style_mut(), &mut style);
        self
    }
}

pub trait ItemsView: View {
    fn items(&self) -> impl Iterator<Item = &dyn View>;
}
pub trait ItemView: View {
    fn item(&self) -> &dyn View;
}

impl Debug for dyn View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View").finish()
    }
}
