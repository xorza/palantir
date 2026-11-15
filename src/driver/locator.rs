//! [`Locator`]: where an input lands, by what the window shows rather than by pixels.

use crate::driver::error::LocateError;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::sense::Sense;
use crate::ui::Ui;
use crate::ui::inspected_widget::InspectedWidget;
use glam::Vec2;
use std::convert::Infallible;
use std::fmt::{self, Debug, Formatter};

/// How many candidates a [`LocateError::NotFound`] lists.
const NEAREST: usize = 10;

/// A place in a window, read off its last frame: a widget by the text it shows or by its id, a
/// point, or a place an app names through [`Locate`]. A target names one or several matches;
/// [`Self::nth`] picks one, and [`Self::offset`] moves it. Its `Debug` writes it as it is built,
/// `Text("Save").nth(1)`, which each [`LocateError`] quotes.
#[derive(Clone, PartialEq)]
pub struct Locator<X = Infallible> {
    target: Target<X>,
    /// The match picked, from 0, of the several the target names; `None` takes the one match.
    nth: Option<usize>,
    /// How far the place moves past the match, in logical pixels.
    offset: Vec2,
}

/// What a [`Locator`] matches before it picks and moves.
#[derive(Clone, Debug, PartialEq)]
enum Target<X> {
    /// Each visible interactive widget whose text, trimmed, is exactly this, as [`Ui::inspect`]
    /// lists it, in paint order.
    Text(String),
    /// The interactive widget with this id.
    Id(WidgetId),
    /// A point of the window, in logical pixels.
    Point(Vec2),
    /// The places the app names, in the order its [`Locate`] lists them.
    App(X),
}

/// What a located point is for, which decides what may stand there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    /// A pointer that moves or scrolls there, which a disabled widget takes as it is.
    Hover,
    /// A press, which a disabled widget takes and does nothing with.
    Press,
}

/// A place an app names, which [`Locator::app`] carries: a graph node, a canvas point. A locator
/// crosses to a live window's thread, hence the owned, sendable copy.
pub trait Locate<A>: Clone + Debug + Send + 'static {
    /// Every place this names in `app`'s last frame, as `ui` laid it out, in order, into `out`.
    /// Naming none is the impl's own [`LocateError::NotFound`], with the nearest it knows; an `Ok`
    /// with none reads as that error with no nearest.
    ///
    /// # Errors
    ///
    /// The [`LocateError`] that says why it names no place.
    fn find_all(&self, app: &A, ui: &Ui, out: &mut Vec<Found>) -> Result<(), LocateError>;
}

impl<A> Locate<A> for Infallible {
    fn find_all(&self, _app: &A, _ui: &Ui, _out: &mut Vec<Found>) -> Result<(), LocateError> {
        match *self {}
    }
}

/// Where a [`Locator`] landed: the rect of what it names, and the point a press there takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    /// The visible rect, in logical pixels; empty for a point.
    pub rect: Rect,
    /// Where a pointer goes to act on it.
    pub point: Vec2,
}

impl Found {
    /// A widget's place: its visible rect, pressed at the center.
    pub const fn center_of(rect: Rect) -> Self {
        Self {
            rect,
            point: rect.center(),
        }
    }
}

impl<X> Locator<X> {
    /// The visible interactive widget whose text, trimmed, is exactly `text`.
    pub fn text(text: impl Into<String>) -> Self {
        Self::of(Target::Text(text.into()))
    }

    /// The interactive widget with this id.
    pub const fn id(id: WidgetId) -> Self {
        Self::of(Target::Id(id))
    }

    /// A point of the window, in logical pixels.
    pub const fn point(point: Vec2) -> Self {
        Self::of(Target::Point(point))
    }

    /// A place the app names.
    pub const fn app(place: X) -> Self {
        Self::of(Target::App(place))
    }

    const fn of(target: Target<X>) -> Self {
        Self {
            target,
            nth: None,
            offset: Vec2::ZERO,
        }
    }

    /// The match at `n`, from 0, of the several the target names: widgets in paint order, an
    /// app's places in the order its [`Locate`] lists them. An id or a point names one, at 0. It
    /// replaces a pick made before.
    #[must_use]
    pub const fn nth(mut self, n: usize) -> Self {
        self.nth = Some(n);
        self
    }

    /// This place moved by `by` logical pixels, after the pick; offsets add.
    #[must_use]
    pub fn offset(mut self, by: Vec2) -> Self {
        self.offset += by;
        self
    }

    /// The one place this names in the last frame `ui` ran of `app`. A widget must be visible
    /// and the topmost interactive widget at its center, so a press there reaches it.
    ///
    /// # Errors
    ///
    /// [`LocateError`] when the target names no place, several and no pick, or fewer than the
    /// pick.
    pub fn find<A>(&self, app: &A, ui: &Ui) -> Result<Found, LocateError>
    where
        X: Locate<A>,
    {
        let mut matches = Vec::new();
        self.find_each(app, ui, &mut matches)?;
        let found = match (self.nth, matches.as_slice()) {
            (None, [one]) => *one,
            (None, _) => {
                return Err(LocateError::Ambiguous {
                    locator: format!("{self:?}"),
                    matches,
                });
            }
            (Some(n), _) => *matches.get(n).ok_or_else(|| LocateError::NoNth {
                locator: format!("{self:?}"),
                count: matches.len(),
            })?,
        };
        Ok(Found {
            rect: Rect {
                min: found.rect.min + self.offset,
                size: found.rect.size,
            },
            point: found.point + self.offset,
        })
    }

    /// The point this names, for `reach`. A press refuses a point where the topmost widget that
    /// clicks is disabled: it would take the press and do nothing, which Playwright's
    /// actionability check refuses too (<https://playwright.dev/docs/actionability>).
    ///
    /// # Errors
    ///
    /// As [`Self::find`], and [`LocateError::Disabled`] for a press on a disabled widget.
    pub(crate) fn point_for<A>(&self, app: &A, ui: &Ui, reach: Reach) -> Result<Vec2, LocateError>
    where
        X: Locate<A>,
    {
        let point = self.find(app, ui)?.point;
        if reach == Reach::Press {
            let top = ui.cascade().hit_row(point, Sense::clicks);
            if top.is_some_and(|row| row.disabled) {
                return Err(LocateError::Disabled {
                    locator: format!("{self:?}"),
                });
            }
        }
        Ok(point)
    }

    /// Every place the target names, at least one, in order.
    fn find_each<A>(&self, app: &A, ui: &Ui, out: &mut Vec<Found>) -> Result<(), LocateError>
    where
        X: Locate<A>,
    {
        let locator = || format!("{self:?}");
        match &self.target {
            Target::Text(text) => {
                let mut widgets = Vec::new();
                ui.inspect(&mut widgets);
                let shows = |widget: &&InspectedWidget| widget.text.trim() == text;
                // A covered or clipped match is skipped, as the input cannot reach it; the reason
                // is the error only when no match is reachable.
                let mut covered = None;
                let mut hidden = false;
                for widget in widgets.iter().filter(shows) {
                    if widget.clipped {
                        hidden = true;
                    } else {
                        match reached(ui, widget, locator) {
                            Ok(found) => out.push(found),
                            Err(error) => covered = covered.or(Some(error)),
                        }
                    }
                }
                if out.is_empty() {
                    return Err(if let Some(covered) = covered {
                        covered
                    } else if hidden {
                        LocateError::OffScreen { locator: locator() }
                    } else {
                        LocateError::NotFound {
                            locator: locator(),
                            nearest: nearest(&widgets, text),
                        }
                    });
                }
            }
            Target::Id(id) => {
                let mut widgets = Vec::new();
                ui.inspect(&mut widgets);
                let widget = widgets
                    .iter()
                    .find(|widget| widget.id == *id)
                    .ok_or_else(|| LocateError::NotFound {
                        locator: locator(),
                        nearest: Vec::new(),
                    })?;
                if widget.clipped {
                    return Err(LocateError::OffScreen { locator: locator() });
                }
                out.push(reached(ui, widget, locator)?);
            }
            Target::App(place) => {
                place.find_all(app, ui, out)?;
                if out.is_empty() {
                    return Err(LocateError::NotFound {
                        locator: locator(),
                        nearest: Vec::new(),
                    });
                }
            }
            Target::Point(point) => out.push(Found {
                rect: Rect::new(point.x, point.y, 0.0, 0.0),
                point: *point,
            }),
        }
        Ok(())
    }
}

/// `widget` pressed at its center, when the input it senses reaches it there by its
/// [routing](Sense::routing).
fn reached(
    ui: &Ui,
    widget: &InspectedWidget,
    locator: impl Fn() -> String,
) -> Result<Found, LocateError> {
    let found = Found::center_of(widget.rect);
    let Some(routes) = widget.sense.routing() else {
        return Ok(found);
    };
    match ui.cascade().hit_row(found.point, routes) {
        Some(top) if top.widget_id == widget.id => Ok(found),
        _ => Err(LocateError::Covered {
            locator: locator(),
            point: found.point,
        }),
    }
}

/// The visible texts that contain `wanted`, ignoring case, or the first few when none does.
fn nearest(widgets: &[InspectedWidget], wanted: &str) -> Vec<String> {
    let texts = || {
        widgets
            .iter()
            .filter(|widget| !widget.clipped)
            .map(|widget| widget.text.trim())
            .filter(|text| !text.is_empty())
    };
    let wanted = wanted.to_lowercase();
    let close: Vec<String> = texts()
        .filter(|text| text.to_lowercase().contains(&wanted))
        .take(NEAREST)
        .map(str::to_owned)
        .collect();
    if close.is_empty() {
        texts().take(NEAREST).map(str::to_owned).collect()
    } else {
        close
    }
}

impl<X: Debug> Debug for Locator<X> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.target {
            Target::Text(text) => write!(f, "Text({text:?})")?,
            Target::Id(id) => write!(f, "Id({id:?})")?,
            Target::Point(point) => write!(f, "Point({}, {})", point.x, point.y)?,
            Target::App(place) => write!(f, "App({place:?})")?,
        }
        if let Some(n) = self.nth {
            write!(f, ".nth({n})")?;
        }
        if self.offset != Vec2::ZERO {
            write!(f, ".offset({}, {})", self.offset.x, self.offset.y)?;
        }
        Ok(())
    }
}

impl<X> From<&str> for Locator<X> {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl<X> From<String> for Locator<X> {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

impl<X> From<WidgetId> for Locator<X> {
    fn from(id: WidgetId) -> Self {
        Self::id(id)
    }
}

impl<X> From<Vec2> for Locator<X> {
    fn from(point: Vec2) -> Self {
        Self::point(point)
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::driver::locator::{Locator, Target};
    use glam::Vec2;

    impl<X> Locator<X> {
        /// The point a point target names, moved by the offset; `None` for a target found in a
        /// frame.
        pub(crate) fn as_point(&self) -> Option<Vec2> {
            match self.target {
                Target::Point(point) => Some(point + self.offset),
                _ => None,
            }
        }
    }
}
