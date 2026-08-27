use crate::layout::types::layout_mode::LayoutMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeMode {
    Resolved(LayoutMode),
    PendingGrid,
}

impl NodeMode {
    #[inline(always)]
    pub(super) fn resolved(self) -> LayoutMode {
        match self {
            Self::Resolved(mode) => mode,
            Self::PendingGrid => {
                panic!("grid node recorded before its definition was installed")
            }
        }
    }
}
