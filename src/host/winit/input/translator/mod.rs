//! The state that turns a stream of winit events into Palantir's input.

use std::mem;

use glam::Vec2;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent};
use winit::keyboard::{ModifiersState, PhysicalKey};

use crate::common::platform::Platform;
use crate::display;
use crate::host::winit::input::{
    KeyDownFacts, ime_event, key_down, normalize_modifiers, shift_wheel,
};
use crate::input::input_event::InputEvent;
use crate::input::keyboard::key::Key;
use crate::input::pointer::PointerButton;

/// The pointer's **physical** position, retained across events because a
/// scale change invalidates the logical one and re-deriving needs it before
/// the division.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PointerTrace {
    Unchanged,
    At(Vec2),
    Gone,
}

impl PointerTrace {
    /// The trace of `event`, in logical px at `scale`, as the translator would report the OS
    /// event it came from. A position the input screen refuses moves nothing.
    pub(crate) fn of_input(event: InputEvent<'_>, scale: f32) -> Self {
        match event {
            InputEvent::PointerMoved(logical) if event.is_valid() => Self::At(logical * scale),
            InputEvent::PointerLeft => Self::Gone,
            _ => Self::Unchanged,
        }
    }
}

/// What translation carries from one event to the next.
///
/// A touchscreen reports no mouse events. Windows promotes a touch to mouse
/// messages only in `DefWindowProc`, which winit does not call for pointer
/// messages, and Wayland keeps touch apart from the pointer. So one finger
/// drives the pointer as the left button. While it is down, other fingers and
/// the mouse's position and left button are ignored, as either would move or
/// end the press the finger holds.
#[derive(Debug)]
pub(crate) struct Translator {
    /// Whose conventions apply: `PLATFORM` in a build, any in a test.
    platform: Platform,
    /// The modifiers winit last reported; key events carry none, and text and
    /// wheel translation read them.
    modifiers: ModifiersState,
    /// The finger standing in for the pointer, while one is down.
    finger: Option<u64>,
    /// The keys down, by the physical key each press came from, with the logical key it
    /// reported: a release's own logical key can differ, as Shift pressed in between turns `a`
    /// into `A`, and `KeyUp` names its press's. A few at a time, so a scan.
    held: Vec<(PhysicalKey, Key)>,
}

impl Translator {
    pub(crate) const fn new(platform: Platform) -> Self {
        Self {
            platform,
            modifiers: ModifiersState::empty(),
            finger: None,
            held: Vec::new(),
        }
    }

    /// Note that `physical` went down as `key`. A key already down lost its release, which ends
    /// here as the `KeyUp` returned, so no press is held twice.
    fn hold(&mut self, physical: PhysicalKey, key: Key) -> Option<InputEvent<'static>> {
        if let Some(entry) = self.held.iter_mut().find(|(held, _)| *held == physical) {
            return Some(InputEvent::KeyUp {
                key: mem::replace(&mut entry.1, key),
            });
        }
        self.held.push((physical, key));
        None
    }

    /// The `KeyUp` of `physical`'s press, or `None` for a key this surface never saw go down.
    fn release(&mut self, physical: PhysicalKey) -> Option<InputEvent<'static>> {
        let idx = self.held.iter().position(|(held, _)| *held == physical)?;
        Some(InputEvent::KeyUp {
            key: self.held.swap_remove(idx).1,
        })
    }

    /// `scale_factor` is physical pixels per logical pixel in the current frame's
    /// layout space. Returns the pointer's physical position the event reported,
    /// for `Window::resync_pointer`.
    pub(crate) fn translate<'e>(
        &mut self,
        event: &'e WindowEvent,
        scale_factor: f32,
        mut emit: impl FnMut(InputEvent<'e>),
    ) -> PointerTrace {
        debug_assert!(
            display::scale_factor_is_valid(scale_factor),
            "the host screens the platform's half through \
             display::sanitize_system_scale and the app's half carries its range \
             in UserScale; got {scale_factor}",
        );
        match event {
            WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput {
                button: MouseButton::Left,
                ..
            } if self.finger.is_some() => {}
            WindowEvent::Touch(touch) => return self.touch(touch, scale_factor, emit),
            WindowEvent::CursorMoved { position, .. } => {
                let physical = Vec2::new(position.x as f32, position.y as f32);
                emit(InputEvent::PointerMoved(physical / scale_factor));
                return PointerTrace::At(physical);
            }
            WindowEvent::CursorLeft { .. } => {
                emit(InputEvent::PointerLeft);
                return PointerTrace::Gone;
            }
            WindowEvent::MouseInput { state, button, .. } => {
                // Three buttons on purpose: [`PointerButton`] indexes per-widget
                // `ButtonState` and an `InputState` capture slot, so a fourth costs every app.
                // Named, not wildcarded, so adding one is a decision.
                let button = match button {
                    MouseButton::Left => PointerButton::Left,
                    MouseButton::Right => PointerButton::Right,
                    MouseButton::Middle => PointerButton::Middle,
                    MouseButton::Back | MouseButton::Forward | MouseButton::Other(_) => {
                        return PointerTrace::Unchanged;
                    }
                };
                emit(match state {
                    ElementState::Pressed => InputEvent::PointerPressed(button),
                    ElementState::Released => InputEvent::PointerReleased(button),
                });
            }
            // A pinch delta is a displacement, so the factor is `1 + delta`. Emitted
            // unscreened; `InputEvent::is_valid` decides what a factor must satisfy.
            WindowEvent::PinchGesture { delta, .. } => {
                emit(InputEvent::Zoom(1.0 + *delta as f32));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (event, delta) = match *delta {
                    MouseScrollDelta::LineDelta(x, y) => (WheelUnit::Lines, Vec2::new(-x, -y)),
                    MouseScrollDelta::PixelDelta(pos) => (
                        WheelUnit::Pixels,
                        Vec2::new(-pos.x as f32, -pos.y as f32) / scale_factor,
                    ),
                };
                let delta = shift_wheel(delta, self.modifiers, self.platform);
                emit(match event {
                    WheelUnit::Lines => InputEvent::ScrollLines(delta),
                    WheelUnit::Pixels => InputEvent::ScrollPixels(delta),
                });
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } if event.state == ElementState::Pressed => {
                if let Some(press) = key_down(
                    KeyDownFacts {
                        logical: &event.logical_key,
                        physical: &event.physical_key,
                        text: event.text.as_deref(),
                        repeat: event.repeat,
                        is_synthetic: *is_synthetic,
                    },
                    self.modifiers,
                ) {
                    if let InputEvent::KeyDown {
                        key, repeat: false, ..
                    } = press
                        && let Some(lost) = self.hold(event.physical_key, key)
                    {
                        emit(lost);
                    }
                    emit(press);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let Some(release) = self.release(event.physical_key) {
                    emit(release);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
                emit(InputEvent::ModifiersChanged(normalize_modifiers(
                    self.modifiers,
                    self.platform,
                )));
            }
            // Only the loss is forwarded; regaining focus is learned from the next event.
            WindowEvent::Focused(false) => {
                emit(InputEvent::SurfaceFocusLost);
                // The recorder releases every held key with the focus.
                self.held.clear();
                // The recorder drops the press with the focus, and an unfocused
                // surface may never see this finger lift: forget it, so the next
                // touch is not ignored.
                if self.finger.take().is_some() {
                    emit(InputEvent::PointerLeft);
                    return PointerTrace::Gone;
                }
            }
            WindowEvent::Ime(ime) => {
                if let Some(event) = ime_event(ime) {
                    emit(event);
                }
            }
            _ => {}
        }
        PointerTrace::Unchanged
    }

    fn touch<'e>(
        &mut self,
        touch: &Touch,
        scale_factor: f32,
        mut emit: impl FnMut(InputEvent<'e>),
    ) -> PointerTrace {
        let physical = Vec2::new(touch.location.x as f32, touch.location.y as f32);
        let logical = physical / scale_factor;
        let held = self.finger == Some(touch.id);
        match touch.phase {
            // Moved before pressed, as the press hit-tests where the pointer is and
            // a finger has no hover to have put it there.
            TouchPhase::Started if self.finger.is_none() || held => {
                // A start from the held finger means its end was lost: nothing says
                // where it lifted, so its press ends as a cancel does.
                if held {
                    emit(InputEvent::PointerLeft);
                    emit(InputEvent::PointerReleased(PointerButton::Left));
                }
                self.finger = Some(touch.id);
                emit(InputEvent::PointerMoved(logical));
                emit(InputEvent::PointerPressed(PointerButton::Left));
                PointerTrace::At(physical)
            }
            TouchPhase::Moved if held => {
                emit(InputEvent::PointerMoved(logical));
                PointerTrace::At(physical)
            }
            // Released where the finger lifted, so a tap is a click; then left, as a
            // lifted finger hovers nothing.
            TouchPhase::Ended if held => {
                self.finger = None;
                emit(InputEvent::PointerMoved(logical));
                emit(InputEvent::PointerReleased(PointerButton::Left));
                emit(InputEvent::PointerLeft);
                PointerTrace::Gone
            }
            // Left first, so the release hit-tests nothing and the press ends as a
            // gesture cut off does (`Capture::abandon_press`): no click, but a drag
            // under way still stops.
            TouchPhase::Cancelled if held => {
                self.finger = None;
                emit(InputEvent::PointerLeft);
                emit(InputEvent::PointerReleased(PointerButton::Left));
                PointerTrace::Gone
            }
            _ => PointerTrace::Unchanged,
        }
    }
}

/// The two units a wheel reports in.
#[derive(Clone, Copy, Debug)]
enum WheelUnit {
    Lines,
    Pixels,
}

#[cfg(test)]
mod tests;
