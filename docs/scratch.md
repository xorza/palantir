image

local animation that dont need relayout

checkbox

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

pub struct Shadow {
pub color: Color,
pub offset: Vec2,
pub blur: f32,
pub spread: f32, #[animate(snap)]
pub inset: bool,
}

    #[inline]
    pub(crate) fn is_noop(self) -> bool {
        // ColorF16 lanes are RGBA. Alpha == 0 (any sign) ⇒ noop.
        const ABS_MASK: u16 = 0x7FFF;
        (self.color.0[3] & ABS_MASK) == 0
    } approx?
