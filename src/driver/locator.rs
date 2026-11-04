//! [`Locator`]: where an input lands, by what the window shows rather than by pixels.

use crate::driver::error::LocateError;
use crate::input::sense::Sense;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use crate::ui::inspected_widget::InspectedWidget;
use glam::Vec2;
use std::convert::Infallible;
use std::fmt::Debug;

/// How many candidates a [`LocateError::NotFound`] lists.
const NEAREST: usize = 10;

/// A place in a window, read off its last frame: a widget by the text it shows or by its id, or
/// a point. An app adds places only it can name as `App`, through [`Locate`].
#[derive(Clone, Debug, PartialEq)]
pub enum Locator<X = Infallible> {
    /// The one visible interactive widget whose text, trimmed, is exactly this, as
    /// [`Ui::inspect`] lists it.
    Text(String),
    /// The interactive widget with this id.
    Id(WidgetId),
    /// A point of the window, in logical pixels.
    Point(Vec2),
    /// Another locator's place, moved by this many logical pixels.
    Offset(Box<Self>, Vec2),
    /// The match at this index, from 0, of a locator that matches several: widgets in paint
    /// order, an app's places in the order its [`Locate`] lists them.
    Nth(Box<Self>, usize),
    /// A place the app names.
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

/// A place an app names, which [`Locator::App`] carries: a graph node, a canvas point.
pub trait Locate<A>: Debug {
    /// Every place this names in `app`'s last frame, as `ui` laid it out, in order, into `out`.
    /// Naming none is the impl's own [`LocateError::NotFound`], with the nearest it knows.
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
    /// This place moved by `by` logical pixels.
    #[must_use]
    pub fn offset(self, by: Vec2) -> Self {
        Self::Offset(Box::new(self), by)
    }

    /// The `n`th place, from 0, of the several this names.
    #[must_use]
    pub fn nth(self, n: usize) -> Self {
        Self::Nth(Box::new(self), n)
    }

    /// The one place this names in the last frame `ui` ran of `app`. A widget must be visible
    /// and the topmost interactive widget at its center, so a press there reaches it.
    ///
    /// # Errors
    ///
    /// [`LocateError`] when it names no place, or several.
    pub fn find<A>(&self, app: &A, ui: &Ui) -> Result<Found, LocateError>
    where
        X: Locate<A>,
    {
        match self {
            Self::Point(point) => Ok(Found {
                rect: Rect::new(point.x, point.y, 0.0, 0.0),
                point: *point,
            }),
            Self::Offset(inner, by) => {
                let found = inner.find(app, ui)?;
                Ok(Found {
                    rect: Rect::new(
                        found.rect.min.x + by.x,
                        found.rect.min.y + by.y,
                        found.rect.size.w,
                        found.rect.size.h,
                    ),
                    point: found.point + *by,
                })
            }
            Self::Nth(inner, n) => {
                let mut found = Vec::new();
                inner.find_each(app, ui, &mut found)?;
                if !matches!(**inner, Self::Text(_) | Self::App(_)) || *n >= found.len() {
                    return Err(LocateError::NoNth {
                        locator: format!("{self:?}"),
                        count: found.len(),
                    });
                }
                Ok(found[*n])
            }
            Self::Text(_) | Self::Id(_) | Self::App(_) => {
                let mut found = Vec::new();
                self.find_each(app, ui, &mut found)?;
                match found.as_slice() {
                    [one] => Ok(*one),
                    many => Err(LocateError::Ambiguous {
                        locator: format!("{self:?}"),
                        matches: many.to_vec(),
                    }),
                }
            }
        }
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
            let mut widgets = Vec::new();
            ui.inspect(&mut widgets);
            if topmost(&widgets, point, Sense::clicks).is_some_and(|widget| widget.disabled) {
                return Err(LocateError::Disabled {
                    locator: format!("{self:?}"),
                });
            }
        }
        Ok(point)
    }

    /// Every place this names, at least one, in order.
    fn find_each<A>(&self, app: &A, ui: &Ui, out: &mut Vec<Found>) -> Result<(), LocateError>
    where
        X: Locate<A>,
    {
        let locator = || format!("{self:?}");
        match self {
            Self::Text(text) => {
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
                        match reached(&widgets, widget, locator) {
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
            Self::Id(id) => {
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
                out.push(reached(&widgets, widget, locator)?);
            }
            Self::App(place) => {
                place.find_all(app, ui, out)?;
                debug_assert!(!out.is_empty(), "{place:?} found nothing and said nothing");
            }
            Self::Point(_) | Self::Offset(..) | Self::Nth(..) => out.push(self.find(app, ui)?),
        }
        Ok(())
    }
}

/// `widget` pressed at its center, when the input it senses reaches it there by its
/// [routing](Sense::routing), as the hit test reads the paint-ordered table `widgets`.
fn reached(
    widgets: &[InspectedWidget],
    widget: &InspectedWidget,
    locator: impl Fn() -> String,
) -> Result<Found, LocateError> {
    let found = Found::center_of(widget.rect);
    let Some(routes) = widget.sense.routing() else {
        return Ok(found);
    };
    match topmost(widgets, found.point, routes) {
        Some(top) if top.id == widget.id => Ok(found),
        _ => Err(LocateError::Covered {
            locator: locator(),
            point: found.point,
        }),
    }
}

/// The last widget of the paint-ordered table `widgets` at `point` that `routes` accepts: the one
/// the hit test hands that input to.
fn topmost(
    widgets: &[InspectedWidget],
    point: Vec2,
    routes: fn(Sense) -> bool,
) -> Option<&InspectedWidget> {
    widgets
        .iter()
        .rev()
        .find(|widget| routes(widget.sense) && widget.rect.contains(point))
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

impl<X> From<&str> for Locator<X> {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl<X> From<String> for Locator<X> {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl<X> From<WidgetId> for Locator<X> {
    fn from(id: WidgetId) -> Self {
        Self::Id(id)
    }
}

impl<X> From<Vec2> for Locator<X> {
    fn from(point: Vec2) -> Self {
        Self::Point(point)
    }
}
