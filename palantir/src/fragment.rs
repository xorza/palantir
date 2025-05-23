use crate::*;

#[derive(Debug, Default)]
pub struct Fragment {
    pub(crate) style: Style,
    pub(crate) id: String,
    pub(crate) items: Vec<Box<dyn View>>,
}

impl Fragment {}
