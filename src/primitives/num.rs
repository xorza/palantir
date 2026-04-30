/// Marker trait for primitive numeric types accepted by `From` impls on
/// `Sizing`, `Size`, `Corners`, `Spacing`, etc.
pub trait Num: Copy {
    fn as_f32(self) -> f32;
}

macro_rules! impl_num {
    ($($t:ty),*) => {
        $(
            impl Num for $t {
                fn as_f32(self) -> f32 { self as f32 }
            }
        )*
    };
}

impl_num!(f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
