use crate::*;

#[derive(Debug)]
pub struct Label {
    frag: Fragment,
}

impl Label {}

impl From<&str> for Label {
    fn from(_: &str) -> Self {
        Self {
            frag: Fragment::default(),
        }
    }
}

impl View for Label {
    fn frag_mut(&mut self) -> &mut Fragment {
        &mut self.frag
    }
}
