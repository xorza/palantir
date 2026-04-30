/// Cross-axis alignment of a child inside its parent stack.
///
/// `Auto` defers to the child's cross-axis `Sizing`: `Fill` stretches, anything
/// else pins to the start. The other variants override that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Auto,
    Start,
    Center,
    End,
    Stretch,
}
