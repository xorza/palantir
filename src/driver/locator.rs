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
    pub const fn rect(rect: Rect) -> Self {
        Self {
            rect,
            point: rect.center(),
        }
    }
}

impl<X> Locator<X> {
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
                        matches: many
                            .iter()
                            .enumerate()
                            .map(|(n, found)| format!("{n}: at {:?}", found.point))
                            .collect(),
                    }),
                }
            }
        }
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
                let mut hidden = false;
                for widget in widgets.iter().filter(shows) {
                    if widget.clipped {
                        hidden = true;
                    } else {
                        out.push(reached(&widgets, widget, locator)?);
                    }
                }
                if out.is_empty() {
                    return Err(if hidden {
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

/// `widget` pressed at its center, when the input it senses reaches it there: it is the topmost
/// widget that clicks there, for one that clicks, or that hovers, for one that only hovers, as
/// the hit test reads the paint-ordered table `widgets`. A widget that takes only focus or a
/// scroll has no press to reach it, so any center serves.
fn reached(
    widgets: &[InspectedWidget],
    widget: &InspectedWidget,
    locator: impl Fn() -> String,
) -> Result<Found, LocateError> {
    let found = Found::rect(widget.rect);
    // The routing that serves what the widget senses: a press goes to the topmost widget that
    // clicks or drags, a hover to the topmost that hovers, so a hover-only label over a button
    // takes no press from it.
    let routes: fn(Sense) -> bool = if widget.sense.clicks() {
        Sense::clicks
    } else if widget.sense.hovers() {
        Sense::hovers
    } else {
        return Ok(found);
    };
    let top = widgets
        .iter()
        .rev()
        .find(|other| routes(other.sense) && other.rect.contains(found.point));
    match top {
        Some(top) if top.id == widget.id => Ok(found),
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
