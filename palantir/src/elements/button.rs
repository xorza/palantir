use crate::*;

#[derive(Debug, Default)]
pub struct Button {
    frag: Fragment,
}

impl Button {
    pub fn onclick<F: FnOnce() -> ()>(self, _f: F) -> Self {
        self
    }
}

impl View for Button {
    fn frag_mut(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}

impl ItemView for Button {}
