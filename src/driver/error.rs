//! Why a [`Locator`](crate::Locator) names no one place, and why an [`Action`](crate::Action)
//! could not be played.

use crate::driver::locator::Found;
use crate::input::capture::DRAG_THRESHOLD;
use glam::Vec2;
use std::error;
use std::fmt::{self, Display, Formatter};
use std::time::Duration;

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
        /// Each match, in the order [`Locator::Nth`](crate::Locator::Nth) counts them.
        matches: Vec<Found>,
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
    /// A press would land on a disabled widget, which takes it and does nothing.
    Disabled {
        /// The locator, as its `Debug` writes it.
        locator: String,
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
            Self::Ambiguous { locator, matches } => {
                write!(
                    f,
                    "{locator} matches {} places; pick one with Nth:",
                    matches.len()
                )?;
                for (n, found) in matches.iter().enumerate() {
                    let separator = if n == 0 { " " } else { ", " };
                    write!(
                        f,
                        "{separator}{n}: at ({}, {})",
                        found.point.x, found.point.y
                    )?;
                }
                Ok(())
            }
            Self::OffScreen { locator } => {
                write!(f, "{locator} is clipped out of view; scroll it into view")
            }
            Self::Covered { locator, point } => write!(
                f,
                "{locator} lies under another widget at ({}, {}); close what covers it",
                point.x, point.y
            ),
            Self::Disabled { locator } => {
                write!(f, "{locator} is disabled; a press there does nothing")
            }
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
    /// The window still asked for frames after `frames` of them: an animation that never ends,
    /// or an app that asks a repaint every frame. The step that failed is the caller's to name, as
    /// an [`ActError`] does by its index.
    Unsettled {
        /// The frames run.
        frames: u32,
    },
    /// A drag that moves `travel` logical pixels, under the threshold a drag starts at, so the
    /// window would read it as a click.
    ShortDrag {
        /// The distance from the press to the release.
        travel: f32,
    },
    /// A picture of a region that holds no pixel of the window.
    EmptyRegion,
    /// A picture of a window that paints no pixels: a harness below
    /// [`Depth::Gpu`](crate::Depth::Gpu).
    NoPixels,
    /// The device was lost before the window's pixels came back.
    DeviceLost,
    /// The device did not finish the frame whose pixels a player waits for within its bound, and
    /// is not lost: a hang no system has reset yet.
    GpuTimedOut,
    /// The window closed, or its host stopped.
    Disconnected,
    /// Something outside the host holds the window's thread, as a native dialog does until the
    /// person answers it, so no event and no frame runs: what
    /// [`Remote::with_blocked`](crate::Remote::with_blocked)'s probe says.
    Blocked,
    /// The window is hidden or minimized, and its host runs no frames for it until it shows.
    Hidden,
    /// The window gave no answer and ran no frame in `waited` of real time, though it is shown and
    /// its thread reads as free: its thread is busy, or hangs.
    NoFrame {
        /// The time waited.
        waited: Duration,
    },
    /// The app stayed [busy](crate::App::is_busy) and sent no [wake](crate::Waker) in `waited` of
    /// real time: its work hangs, or ends with no wake.
    Busy {
        /// The time waited.
        waited: Duration,
    },
    /// The app stayed [busy](crate::App::is_busy) through `waited` of real time while it kept
    /// waking the window, past twenty of the settle's waits for a wake, ten minutes at a remote's
    /// default: its work runs on past one step, which an agent waits for itself.
    StillBusy {
        /// The time waited.
        waited: Duration,
    },
}

impl Display for PlayError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locate(error) => error.fmt(f),
            Self::Unsettled { frames } => write!(
                f,
                "the window still asks for frames after {frames} frames; an animation runs on, \
                 or the app asks a repaint each frame"
            ),
            Self::ShortDrag { travel } => write!(
                f,
                "a drag of {travel} px is under the {DRAG_THRESHOLD} px a drag starts at, so it \
                 would play as a click; drag farther"
            ),
            Self::EmptyRegion => f.write_str("the region holds no pixel of the window"),
            Self::NoPixels => f.write_str("the window paints no pixels; build it with a GPU"),
            Self::DeviceLost => f.write_str("the device was lost before the pixels came back"),
            Self::GpuTimedOut => {
                f.write_str("the device did not finish the frame in time, and hangs unreset")
            }
            Self::Disconnected => f.write_str("the window closed"),
            Self::Blocked => f.write_str(
                "something outside the window holds its thread, as a native dialog does; answer \
                 it, then try again",
            ),
            Self::Hidden => {
                f.write_str("the window is hidden or minimized and runs no frame; show it")
            }
            Self::NoFrame { waited } => write!(
                f,
                "the window ran no frame in {} s; its thread is busy, or hangs",
                waited.as_secs_f32()
            ),
            Self::Busy { waited } => write!(
                f,
                "the app stayed busy {} s and sent no wake; its work hangs, or ends with no wake",
                waited.as_secs_f32()
            ),
            Self::StillBusy { waited } => write!(
                f,
                "the app stayed busy {} s while it woke the window; its work runs past one step, \
                 so wait for it and settle again",
                waited.as_secs_f32()
            ),
        }
    }
}

impl error::Error for PlayError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            // Skips the locate error itself, whose words the `Display` already forwards.
            Self::Locate(error) => error.source(),
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
        write!(f, "action {} failed", self.ran)
    }
}

impl error::Error for ActError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
mod tests {
    use crate::driver::error::{ActError, LocateError, PlayError};
    use std::error::Error;
    use std::time::Duration;

    /// A cause shows once in a chain: a failed action names its step and hands on why, and a
    /// locate failure, which a play error shows in its own words, is not handed on again.
    #[test]
    fn each_cause_shows_once_in_the_chain() {
        let locate = LocateError::OffScreen {
            locator: "Text(\"Save\")".to_owned(),
        };
        let play = PlayError::Locate(locate.clone());
        assert_eq!(play.to_string(), locate.to_string());
        assert!(play.source().is_none());

        let act = ActError {
            ran: 3,
            error: play.clone(),
        };
        assert_eq!(act.to_string(), "action 3 failed");
        assert_eq!(
            act.source().map(ToString::to_string),
            Some(play.to_string())
        );
    }

    /// Why a window runs no frame says what corrects it, one fix per cause.
    #[test]
    fn each_cause_of_no_frame_says_its_own_fix() {
        for (error, text) in [
            (
                PlayError::Blocked,
                "something outside the window holds its thread, as a native dialog does; answer \
                 it, then try again",
            ),
            (
                PlayError::Hidden,
                "the window is hidden or minimized and runs no frame; show it",
            ),
            (
                PlayError::NoFrame {
                    waited: Duration::from_millis(1500),
                },
                "the window ran no frame in 1.5 s; its thread is busy, or hangs",
            ),
        ] {
            assert_eq!(error.to_string(), text);
        }
    }
}
