use crate::*;

#[derive(Debug, Default, Clone, Copy)]
pub enum VAlign {
    #[default]
    Stretch,
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum HAlign {
    #[default]
    Stretch,
    Left,
    Center,
    Right,
}

#[derive(Debug)]
pub struct Style {
    pub padding: f32,
    pub margin: f32,
    pub font_size: u32,
    pub color: rgb::RGBA8,
    pub v_align: VAlign,
    pub h_align: HAlign,
    pub border_width: f32,
    pub border_color: rgb::RGBA8,
    pub border_radius: f32,
    pub background_color: rgb::RGBA8,
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
    fn background_color(self, color: rgb::RGBA8) -> Self
    where
        Self: Sized;
    fn border_width(self, width: f32) -> Self
    where
        Self: Sized;
    fn border_color(self, color: rgb::RGBA8) -> Self
    where
        Self: Sized;
    fn border_radius(self, radius: f32) -> Self
    where
        Self: Sized;
    fn v_align(self, align: VAlign) -> Self
    where
        Self: Sized;
    fn h_align(self, align: HAlign) -> Self
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
    fn background_color(mut self, color: rgb::RGBA8) -> Self
    where
        Self: Sized,
    {
        self.style_mut().background_color = color;
        self
    }
    fn border_width(mut self, width: f32) -> Self
    where
        Self: Sized,
    {
        self.style_mut().border_width = width;
        self
    }
    fn border_color(mut self, color: rgb::RGBA8) -> Self
    where
        Self: Sized,
    {
        self.style_mut().border_color = color;
        self
    }
    fn border_radius(mut self, radius: f32) -> Self
    where
        Self: Sized,
    {
        self.style_mut().border_radius = radius;
        self
    }
    fn v_align(mut self, align: VAlign) -> Self
    where
        Self: Sized,
    {
        self.style_mut().v_align = align;
        self
    }
    fn h_align(mut self, align: HAlign) -> Self
    where
        Self: Sized,
    {
        self.style_mut().h_align = align;
        self
    }

}

impl Default for Style {
    fn default() -> Self {
        Self {
            padding: 2.0,
            margin: 2.0,
            font_size: 12,
            color: Colors::WHITE,
            background_color: Colors::TRANSPARENT,
            v_align: VAlign::Stretch,
            h_align: HAlign::Stretch,
            border_width: 0.0,
            border_color: Colors::WHITE,
            border_radius: 0.0,
        }
    }
}

impl Style {}
