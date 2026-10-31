//! [`Session`]: the instrument settings a night's imaging runs under.

/// The cameras the form offers, by index.
pub(crate) const CAMERAS: [&str; 3] = ["Mono 26 MP", "Color 24 MP", "Planetary 2 MP"];
/// The filter wheel's slots, by index.
pub(crate) const FILTERS: [&str; 7] = ["L", "R", "G", "B", "H\u{3b1}", "OIII", "SII"];

/// How many pixels the sensor sums into one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Binning {
    One,
    Two,
    Three,
}

impl Binning {
    pub(crate) const ALL: [Self; 3] = [Self::One, Self::Two, Self::Three];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::One => "1\u{d7}1",
            Self::Two => "2\u{d7}2",
            Self::Three => "3\u{d7}3",
        }
    }
}

/// The settings every exposure of the night takes.
#[derive(Debug, PartialEq)]
pub(crate) struct Session {
    /// An index into [`CAMERAS`].
    pub(crate) camera: usize,
    /// An index into [`FILTERS`], the filter a new plan entry takes.
    pub(crate) filter: usize,
    /// One exposure, in seconds.
    pub(crate) exposure_s: f64,
    pub(crate) gain: f64,
    pub(crate) cooling: bool,
    /// The sensor's target temperature while cooling, in °C.
    pub(crate) target_c: f64,
    pub(crate) binning: Binning,
}

impl Session {
    /// The settings a night starts with.
    pub(crate) const fn new() -> Self {
        Self {
            camera: 0,
            filter: 0,
            exposure_s: 120.0,
            gain: 100.0,
            cooling: true,
            target_c: -10.0,
            binning: Binning::One,
        }
    }
}
