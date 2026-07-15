use glam::BVec2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayoutMode {
    Leaf = 0,
    HStack = 1,
    VStack = 2,
    WrapHStack = 3,
    WrapVStack = 4,
    ZStack = 5,
    Canvas = 6,
    Grid = 7,
    Scroll = 8,
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GridDefId(u16);

impl GridDefId {
    pub(crate) const PENDING: Self = Self(u16::MAX);

    pub(crate) fn from_index(index: usize) -> Self {
        debug_assert!(
            index < u16::MAX as usize,
            "more than 65 535 Grid panels in a single frame",
        );
        Self(index as u16)
    }
}

impl From<GridDefId> for usize {
    fn from(value: GridDefId) -> Self {
        value.0 as usize
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ScrollSpec(u16);

impl ScrollSpec {
    const PAN_X: u16 = 0b0001;
    const PAN_Y: u16 = 0b0010;
    const FIT_X: u16 = 0b0100;
    const FIT_Y: u16 = 0b1000;

    pub(crate) const HORIZONTAL: Self = Self(Self::PAN_X);
    pub(crate) const VERTICAL: Self = Self(Self::PAN_Y);
    pub(crate) const BOTH: Self = Self(Self::PAN_X | Self::PAN_Y);

    #[inline]
    pub(crate) fn pan_mask(self) -> BVec2 {
        BVec2::new(self.0 & Self::PAN_X != 0, self.0 & Self::PAN_Y != 0)
    }

    #[inline]
    pub(crate) fn fit_mask(self) -> BVec2 {
        BVec2::new(self.0 & Self::FIT_X != 0, self.0 & Self::FIT_Y != 0)
    }

    pub(crate) fn with_fit(mut self, fit: BVec2) -> Self {
        let pan = self.pan_mask();
        debug_assert!(
            (!fit.x || pan.x) && (!fit.y || pan.y),
            "Scroll fit axes must be a subset of its pan axes",
        );
        self.0 &= !(Self::FIT_X | Self::FIT_Y);
        self.0 |= u16::from(fit.x) * Self::FIT_X;
        self.0 |= u16::from(fit.y) * Self::FIT_Y;
        self
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ModePayload(u16);

impl ModePayload {
    pub(crate) const NONE: Self = Self(0);

    pub(crate) const fn grid(id: GridDefId) -> Self {
        Self(id.0)
    }

    pub(crate) const fn scroll(spec: ScrollSpec) -> Self {
        Self(spec.0)
    }

    pub(crate) fn grid_def_id(self, mode: LayoutMode) -> GridDefId {
        debug_assert_eq!(
            mode,
            LayoutMode::Grid,
            "grid payload read from {mode:?} node",
        );
        GridDefId(self.0)
    }

    pub(crate) fn scroll_spec(self, mode: LayoutMode) -> ScrollSpec {
        debug_assert_eq!(
            mode,
            LayoutMode::Scroll,
            "scroll payload read from {mode:?} node",
        );
        ScrollSpec(self.0)
    }
}
