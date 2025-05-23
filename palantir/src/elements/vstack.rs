use crate::*;

#[derive(Debug, Default)]
pub struct VStack {
    frag: Fragment,
}

impl VStack {
    pub fn add_item<T: View>(self, item: T) -> Self {
        self
    }
}

impl View for VStack {
    fn frag(&self) -> &Fragment {
        &self.frag
    }
    fn frag_mut(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}

impl ItemsView for VStack {}
