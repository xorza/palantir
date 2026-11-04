//! [`AgentWindow`]: the verbs an agent drives a window with, on either host.

use crate::driver::action::Action;
use crate::driver::error::{ActError, PlayError};
use crate::driver::locator::{Found, Locate, Locator};
use crate::primitives::geometry::rect::Rect;
use crate::ui::inspected_widget::InspectedWidget;
use crate::window::screenshot::Screenshot;

/// A window of the app `A` an agent drives as a person at it would: a
/// [`Remote`](crate::Remote) on a live window, or a [`HarnessSession`](crate::HarnessSession) on a
/// [`Harness`](crate::Harness) and its app. One set of verbs, so a driver writes each step once for
/// both hosts. The two differ only in who holds the app: a harness takes it for each session, as a
/// closure is an app, and a remote's lives on the window's thread. Refused: matching inherent
/// methods on each host, which a driver still joined through an enum of its own.
pub trait AgentWindow<A> {
    /// Play `actions` in order, settling after each; the first that fails stops the rest. Returns
    /// the frames run.
    ///
    /// # Errors
    ///
    /// [`ActError`], naming the step it stopped at.
    fn play<X: Locate<A> + Clone + Send + 'static>(
        &mut self,
        actions: &[Action<X>],
    ) -> Result<u32, ActError>;

    /// Run the window until a frame asks no repaint and the app is not [busy](crate::App::is_busy);
    /// returns the frames run.
    ///
    /// # Errors
    ///
    /// The [`PlayError`] that kept it from settling.
    fn settle(&mut self) -> Result<u32, PlayError>;

    /// A picture of `region`, the whole window for none, as a later frame paints it.
    ///
    /// # Errors
    ///
    /// The [`PlayError`] that kept the picture from landing.
    fn shot(&mut self, region: Option<Rect>) -> Result<Screenshot, PlayError>;

    /// The one place `at` names in the last frame.
    ///
    /// # Errors
    ///
    /// [`PlayError::Locate`] when `at` names no one place, and what the host's query fails with.
    fn find<X: Locate<A> + Clone + Send + 'static>(
        &self,
        at: &Locator<X>,
    ) -> Result<Found, PlayError>;

    /// The visible interactive widgets of the last frame whose text holds `filter`, ignoring
    /// case, in paint order: what an agent reads to pick a locator.
    ///
    /// # Errors
    ///
    /// What the host's query fails with.
    fn widgets(&self, filter: Option<&str>) -> Result<Vec<InspectedWidget>, PlayError>;
}
