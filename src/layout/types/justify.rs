/// Main-axis distribution of leftover space in a stack panel. Mirrors CSS
/// `justify-content`. Has no effect when any child is `Sizing::Fill` along
/// the main axis — Fill consumes the leftover first.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Justify {
    /// Pack to the start (left for HStack, top for VStack). Default.
    #[default]
    Start,
    /// Pack to the center of the main axis.
    Center,
    /// Pack to the end (right / bottom).
    End,
    /// First child at start, last at end, leftover split as equal extra gaps
    /// between siblings. With <2 visible children, behaves like `Start`.
    SpaceBetween,
    /// Equal padding around each child: leftover/(count) per slot, half at
    /// the leading edge, half at the trailing edge, full between siblings.
    SpaceAround,
}
