use std::mem::swap;

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

    pub padding: Edges,
    pub margin: Edges,

    pub font_size: u32,

    pub color: rgb::RGBA8,
    pub background_color: rgb::RGBA8,

    pub border_width: Edges,
    pub border_color: rgb::RGBA8,
    pub border_radius: f32,
}

pub trait Stylable: View {
    fn set_style(mut self, style: Style) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style = style;
        self
    }

    fn set_width<S: Into<Size>>(mut self, width: S) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.width = width.into();
        self
    }

    fn set_height<S: Into<Size>>(mut self, height: S) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.height = height.into();
        self
    }
    fn set_min_width<S: Into<Size>>(mut self, width: S) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.min_width = width.into();
        self
    }
    fn set_min_height<S: Into<Size>>(mut self, height: S) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.min_height = height.into();
        self
    }

    fn set_max_width<S: Into<Size>>(mut self, width: S) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.max_width = width.into();
        self
    }

    fn set_max_height<S: Into<Size>>(mut self, height: S) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.max_height = height.into();
        self
    }

    fn set_padding<E: Into<Edges>>(mut self, padding: E) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.padding = padding.into();
        self
    }
   
    fn set_margin<E: Into<Edges>>(mut self, margin: E) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.margin = margin.into();
        self
    }
 
    fn set_font_size(mut self, size: u32) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.font_size = size;
        self
    }
  
    fn set_font_color(mut self, color: rgb::RGBA8) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.color = color;
        self
    }
  
    fn set_background_color(mut self, color: rgb::RGBA8) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.background_color = color;
        self
    }
  
    fn set_border_width<E: Into<Edges>>(mut self, width: E) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.border_width = width.into();
        self
    }
   
    fn set_border_color(mut self, color: rgb::RGBA8) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.border_color = color;
        self
    }
 
    fn set_border_radius(mut self, radius: f32) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.border_radius = radius;
        self
    }
   
    fn set_v_align(mut self, align: Align) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.v_align = align;
        self
    }
   
    fn set_h_align(mut self, align: Align) -> Self
    where
        Self: Sized,
    {
        self.frag_mut().style.h_align = align;
        self
    }
}

impl<T: View> Stylable for T {}

impl Default for Style {
    fn default() -> Self {
        Self {
            width: Size::Auto,
            height: Size::Auto,
            min_width: Size::Auto,
            min_height: Size::Auto,
            max_width: Size::Auto,
            max_height: Size::Auto,

            v_align: Align::Stretch,
            h_align: Align::Stretch,

            padding: Edges::all(2.0),
            margin: Edges::all(2.0),

            font_size: 12,

            color: Colors::WHITE,
            background_color: Colors::TRANSPARENT,

            border_width: Edges::all(0.0),
            border_color: Colors::WHITE,
            border_radius: 0.0,
        }
    }
}

impl Style {}

impl PartialEq for Style {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.min_width == other.min_width
            && self.min_height == other.min_height
            && self.max_width == other.max_width
            && self.max_height == other.max_height
            && self.v_align == other.v_align
            && self.h_align == other.h_align
            && self.padding == other.padding
            && self.margin == other.margin
            && self.font_size == other.font_size
            && self.color == other.color
            && self.background_color == other.background_color
            && self.border_width == other.border_width
            && self.border_color == other.border_color
            && nan_aware_eq(self.border_radius, other.border_radius)
    }
}
