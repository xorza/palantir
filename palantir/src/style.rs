use crate::*;



#[derive(Debug)]
pub struct Style {
    pub width: Size,
    pub height: Size,
    pub min_width: Size,
    pub min_height: Size,
    pub max_width: Size,
    pub max_height: Size,

    pub v_align: Align,
    pub h_align: Align,

    pub padding: f32,
    pub margin: f32,

    pub font_size: u32,

    pub color: rgb::RGBA8,
    pub background_color: rgb::RGBA8,
    
    pub border_width: f32,
    pub border_color: rgb::RGBA8,
    pub border_radius: f32,
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
    fn v_align(self, align: Align) -> Self
    where
        Self: Sized;
    fn h_align(self, align: Align) -> Self
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
    fn v_align(mut self, align: Align) -> Self
    where
        Self: Sized,
    {
        self.style_mut().v_align = align;
        self
    }
    fn h_align(mut self, align: Align) -> Self
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
            width:  Size::Auto,
            height: Size::Auto,
            min_width: Size::Auto,
            min_height: Size::Auto,
            max_width: Size::Auto,
            max_height: Size::Auto,
            
            v_align: Align::Stretch,
            h_align: Align::Stretch,

            padding: 2.0,
            margin: 2.0,

            font_size: 12,

            color: Colors::WHITE,
            background_color: Colors::TRANSPARENT,

            border_width: 0.0,
            border_color: Colors::WHITE,
            border_radius: 0.0,
        }
    }
}

impl Style {}
