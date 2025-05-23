

pub struct Colors {}

impl Colors {
    pub const TRANSPARENT: rgb::RGBA8 = rgb::RGBA8::new(0, 0, 0, 0);
    pub const WHITE: rgb::RGBA8 = rgb::RGBA8::new(255, 255, 255, 255);
    pub const BLACK: rgb::RGBA8 = rgb::RGBA8::new(0, 0, 0, 255);

    pub const RED: rgb::RGBA8 = rgb::RGBA8::new(255, 0, 0, 255);
    pub const GREEN: rgb::RGBA8 = rgb::RGBA8::new(0, 255, 0, 255);
    pub const BLUE: rgb::RGBA8 = rgb::RGBA8::new(0, 0, 255, 255);
}


pub(crate) fn nan_aware_eq(a: f32, b: f32) -> bool {
    if a.is_nan() && b.is_nan() {
        true
    } else if a.is_nan() || b.is_nan() {
        false
    } else {
        (a - b).abs() < f32::EPSILON
    }
}

