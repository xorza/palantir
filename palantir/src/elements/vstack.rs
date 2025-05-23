use crate::*;

#[derive(Debug, Default)]
pub struct VStack {
    frag: Fragment,
}

impl VStack {
    pub fn add<T: View>(self, item: T) -> Self {
        self
    }
}

impl View for VStack {
    fn frag_mut(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}


impl ItemsView for VStack {

}
