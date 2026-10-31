//! Why a [`Locator`](crate::Locator) names no one place, and why an [`Action`](crate::Action)
//! could not be played.

use glam::Vec2;
use std::error;
use std::fmt::{self, Display, Formatter};

/// Why a [`Locator`](crate::Locator) found no one place in the last frame. Each case says what
/// corrects the locator, as an agent or a script reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum LocateError {
    /// Nothing matches; `nearest` lists the visible texts that contain the one asked for, or the
    /// first ones when none does.
    NotFound {
        /// The locator, as its `Debug` writes it.
        locator: String,
        /// Texts to try instead.
        nearest: Vec<String>,
    },
    /// Several match; `matches` lists each with its index for [`Locator::Nth`](crate::Locator::Nth).
    Ambiguous {
        /// The locator, as its `Debug` writes it.
        locator: String,
        /// Each match, with its index and the point a press takes.
        matches: Vec<String>,
    },
    /// It matches only what the ancestors' clips hide, so no pointer reaches it.
    OffScreen {
        /// The locator, as its `Debug` writes it.
        locator: String,
    },
    /// Another widget lies on top of it at `point`, where a press would land on that one.
    Covered {
        /// The locator, as its `Debug` writes it.
        locator: String,
        /// Where a press would land on the other widget.
        point: Vec2,
    },
    /// [`Locator::Nth`](crate::Locator::Nth) around a locator that names one place, or past the
    /// matches there are.
    NoNth {
        /// The locator, as its `Debug` writes it.
        locator: String,
        /// The matches the inner locator has.
        count: usize,
    },
}

impl Display for LocateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { locator, nearest } if nearest.is_empty() => {
                write!(f, "{locator} matches nothing")
            }
            Self::NotFound { locator, nearest } => write!(
                f,
                "{locator} matches nothing; the nearest are: {}",
                nearest.join(", ")
            ),
            Self::Ambiguous { locator, matches } => write!(
                f,
                "{locator} matches {} places; pick one with Nth: {}",
                matches.len(),
                matches.join(", ")
            ),
            Self::OffScreen { locator } => {
                write!(f, "{locator} is clipped out of view; scroll it into view")
            }
            Self::Covered { locator, point } => write!(
                f,
                "{locator} lies under another widget at ({}, {}); close what covers it",
                point.x, point.y
            ),
            Self::NoNth { locator, count } => {
                write!(f, "{locator} picks past the {count} matches there are")
            }
        }
    }
}

impl error::Error for LocateError {}

/// Why a player could not play a step, settle the window or take its picture. Each case says
/// what corrects the call, as an agent or a script reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayError {
    /// A locator names no one place.
    Locate(LocateError),
    /// The window still asked for frames after `frames` of them, past the step `step`: an
    /// animation that never ends, or an app that asks a repaint every frame.
    Unsettled {
        /// The step, as its `Debug` writes it.
        step: String,
        /// The frames run.
        frames: u32,
    },
    /// A pinch by a factor that is not finite and above 0, which the window would drop.
    BadPinch {
        /// The factor asked.
        factor: f32,
    },
    /// A picture of a region that holds no pixel of the window.
    EmptyRegion,
    /// A picture of a window that paints no pixels: a harness below
    /// [`Depth::Gpu`](crate::Depth::Gpu).
    NoPixels,
    /// The window closed, or its host stopped.
    Disconnected,
    /// The window ran no frame in `seconds` of real time: it is hidden or minimized, and its host
    /// runs no frames for it until it shows.
    NoFrame {
        /// The time waited.
        seconds: f32,
    },
    /// The app stayed [busy](crate::App::is_busy) past the step `step` and sent no
    /// [wake](crate::Waker) in `seconds` of real time: its work hangs, or ends with no wake.
    Busy {
        /// The step, as its `Debug` writes it.
        step: String,
        /// The time waited.
        seconds: f32,
    },
    /// A remote query panicked on the window's thread, which went on; `message` is the panic's.
    Panicked {
        /// The panic's message, or a note that it had none.
        message: String,
    },
}

impl Display for PlayError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locate(error) => error.fmt(f),
            Self::Unsettled { step, frames } => write!(
                f,
                "the window still asks for frames {frames} frames after {step}; an animation \
                 runs on, or the app asks a repaint each frame"
            ),
            Self::BadPinch { factor } => {
                write!(f, "a pinch takes a finite factor above 0, not {factor}")
            }
            Self::EmptyRegion => f.write_str("the region holds no pixel of the window"),
            Self::NoPixels => f.write_str("the window paints no pixels; build it with a GPU"),
            Self::Disconnected => f.write_str("the window closed"),
            Self::NoFrame { seconds } => write!(
                f,
                "the window ran no frame in {seconds} s; show it, as a hidden or minimized \
                 window runs none"
            ),
            Self::Busy { step, seconds } => write!(
                f,
                "the app stayed busy {seconds} s after {step} and sent no wake; its work hangs, \
                 or ends with no wake"
            ),
            Self::Panicked { message } => write!(f, "the query panicked: {message}"),
        }
    }
}

impl error::Error for PlayError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::Locate(error) => Some(error),
            _ => None,
        }
    }
}

impl From<LocateError> for PlayError {
    fn from(error: LocateError) -> Self {
        Self::Locate(error)
    }
}

/// Why a list of [`Action`](crate::Action)s stopped: the step it stopped at, after `ran` steps
/// played.
#[derive(Clone, Debug, PartialEq)]
pub struct ActError {
    /// The steps played before the one that failed.
    pub ran: usize,
    /// Why that one failed.
    pub error: PlayError,
}

impl Display for ActError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "action {} failed: {}", self.ran, self.error)
    }
}

impl error::Error for ActError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.error)
    }
}
