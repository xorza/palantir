#[derive(Debug, Default)]
pub struct Style {
    pub padding: f32,
    pub margin: f32,
    pub font_size: u32,
    pub color: rgb::RGBA8,
}

pub trait View {
    fn style_mut(&mut self) -> &mut Style;
}

pub trait Stylable {
    fn padding(self, padding: f32) -> Self
    where
        Self: Sized;
    fn margin(self, margin: f32) -> Self
    where
        Self: Sized;
    fn font_size(self, size: u32) -> Self
    where
        Self: Sized;
    fn color(self, color: rgb::RGBA8) -> Self
    where
        Self: Sized;
}

impl<T: View> Stylable for T {
    fn padding(mut self, padding: f32) -> Self
    where
        Self: Sized,
    {
        self.style_mut().padding = padding;
        self
    }

    fn margin(mut self, margin: f32) -> Self
    where
        Self: Sized,
    {
        self.style_mut().margin = margin;
        self
    }

    fn font_size(mut self, size: u32) -> Self
    where
        Self: Sized,
    {
        self.style_mut().font_size = size;
        self
    }

    fn color(mut self, color: rgb::RGBA8) -> Self
    where
        Self: Sized,
    {
        self.style_mut().color = color;
        self
    }
}

pub struct Colors {}

impl Colors {
    pub const RED: rgb::RGBA8 = rgb::RGBA8::new(255, 0, 0, 255);
    pub const GREEN: rgb::RGBA8 = rgb::RGBA8::new(0, 255, 0, 255);
    pub const BLUE: rgb::RGBA8 = rgb::RGBA8::new(0, 0, 255, 255);
}

pub trait ItemsView {
    fn add<T: View>(self, item: T) -> Self;
}
pub trait ItemView {
    fn item<T: View>(self, item: T) -> Self;
}

pub struct VStack {
    style: Style,
}

impl VStack {
    pub fn new() -> Self {
        Self {
            style: Style::default(),
        }
    }
}

impl View for VStack {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl ItemsView for VStack {
    fn add<T: View>(self, _item: T) -> Self {
        self
    }
}

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
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

pub struct Button {
    style: Style,
}

impl Button {
    pub fn new() -> Self {
        Self {
            style: Style::default(),
        }
    }
    pub fn onclick<F: FnOnce() -> ()>(self, _f: F) -> Self {
        self
    }
}

impl View for Button {
    fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}
impl ItemView for Button {
    fn item<T: View>(self, _item: T) -> Self {
        self
    }
}
