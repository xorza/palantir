use crate::*;

#[derive(Debug)]
pub struct Label {
    style: Style,
}

impl Label {}

impl From<&str> for Label {
    fn from(_: &str) -> Self {
        Self {
            style: Style::default(),
        }
    }
}

impl View for Label {
    fn get_style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}
