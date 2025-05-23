use std::{fmt::Debug, mem::swap};

use crate::*;

pub trait View {
    fn frag_mut(&mut self) -> &mut Fragment;
}

pub trait ItemsView: View {
    fn items_mut(&mut self) -> &mut Vec<Box<dyn View>> {
        &mut self.frag_mut().items
    }

    fn add_item<T>(mut self, item: T) -> Self
    where
        Self: Sized,
        T: View + 'static,
    {
        self.frag_mut().items.push(Box::new(item));
        self
    }
}
pub trait ItemView: View {
    fn item_mut(&mut self) -> &mut dyn View {
        self.frag_mut().items[0].as_mut()
    }

    fn set_item<T>(mut self, item: T) -> Self
    where
        Self: Sized,
        T: View + 'static,
    {
        self.frag_mut().items.clear();
        self.frag_mut().items.push(Box::new(item));
        self
    }
}

impl Debug for dyn View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("View").finish()
    }
}
