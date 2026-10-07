//! How a stack distributes the space its children did not use.

/// Main-axis distribution of leftover space, as CSS `justify-content`; a `Sizing::fill` child along the main axis consumes the leftover first.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Justify {
    /// Pack to the start. Default.
    #[default]
    Start,
    /// Pack to the center.
    Center,
    /// Pack to the end.
    End,
    /// Equal gaps between siblings; with fewer than 2 visible children, `Start`.
    SpaceBetween,
    /// Equal padding around each child: half at each edge, full between siblings.
    SpaceAround,
}
