image

local animation that dont need relayout

checkbox

stroke
pub(crate) width: f32, - pack to u16

#[repr(C)] #[derive(Copy, Clone, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FillAxis {
pub dir_x: f32,
pub dir_y: f32,
pub t0: f32,
pub t1: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LinearGradient {
pub angle: f32,
pub stops: ArrayVec<[Stop; MAX_STOPS]>,
pub spread: Spread,
pub interp: Interp,
}

lower chrome bg brush
