use crate::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    #[default]
    Stretch,
    Start,
    Center,
    End,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum Size {
    #[default]
    Auto,
    Fixed(f32),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Edges {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl<T: Into<f32>> From<T> for Size {
    fn from(value: T) -> Self {
        Self::Fixed(value.into())
    }
}

impl PartialEq for Size {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Auto, Self::Auto) => true,
            (Self::Fixed(l), Self::Fixed(r)) => nan_aware_eq(*r, *l),

            _ => false,
        }
    }
}

impl Eq for Size {}

impl Edges {
    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn horizontal_vertical(h: f32, v: f32) -> Self {
        Self {
            top: v,
            right: h,
            bottom: v,
            left: h,
        }
    }
}

impl PartialEq for Edges {
    fn eq(&self, other: &Self) -> bool {
        nan_aware_eq(self.top, other.top)
            && nan_aware_eq(self.right, other.right)
            && nan_aware_eq(self.bottom, other.bottom)
            && nan_aware_eq(self.left, other.left)
    }
}
impl Eq for Edges {}

impl From<f32> for Edges {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl From<i32> for Edges {
    fn from(value: i32) -> Self {
        Self::all(value as f32)
    }
}
impl From<u32> for Edges {
    fn from(value: u32) -> Self {
        Self::all(value as f32)
    }
}

impl From<(f32, f32)> for Edges {
    fn from(value: (f32, f32)) -> Self {
        Self::horizontal_vertical(value.0, value.1)
    }
}

impl From<(f32, f32, f32, f32)> for Edges {
    fn from(value: (f32, f32, f32, f32)) -> Self {
        Self {
            top: value.0,
            right: value.1,
            bottom: value.2,
            left: value.3,
        }
    }
}

impl From<(i32, i32)> for Edges {
    fn from(value: (i32, i32)) -> Self {
        Self::horizontal_vertical(value.0 as f32, value.1 as f32)
    }
}

impl From<(i32, i32, i32, i32)> for Edges {
    fn from(value: (i32, i32, i32, i32)) -> Self {
        Self {
            top: value.0 as f32,
            right: value.1 as f32,
            bottom: value.2 as f32,
            left: value.3 as f32,
        }
    }
}

impl From<(u32, u32)> for Edges {
    fn from(value: (u32, u32)) -> Self {
        Self::horizontal_vertical(value.0 as f32, value.1 as f32)
    }
}

impl From<(u32, u32, u32, u32)> for Edges {
    fn from(value: (u32, u32, u32, u32)) -> Self {
        Self {
            top: value.0 as f32,
            right: value.1 as f32,
            bottom: value.2 as f32,
            left: value.3 as f32,
        }
    }
}

