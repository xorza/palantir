//! [`MotionRole`]: what an animation's motion tells the person.

/// What an animation's motion tells the person, which decides whether it runs while motion is
/// reduced; each animation says its own, as only its author knows: a paint animation through
/// [`PaintAnimation::with_motion`](crate::widget::PaintAnimation::with_motion), a value
/// transition through [`AnimationRequest::with_motion`](crate::widget::AnimationRequest::with_motion).
/// One role for both, as a turn and a moving bar tell the person alike.
///
/// WCAG 2.3.3 lets motion that is essential to what the content says stay on under that
/// preference, and asks for the rest to stop
/// ([WCAG 2.3.3](https://www.w3.org/WAI/WCAG22/Understanding/animation-from-interactions)).
/// Refused: telling them apart by channel, as WCAG leaves an opacity change out of motion, which
/// stops a turn that shows work and keeps a fade that only decorates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionRole {
    /// Motion that only decorates. While motion is reduced a value transition snaps to its
    /// target, a single paint pass paints its end value
    /// ([`PaintRepeat::Held`](crate::widget::PaintRepeat::Held)) and a repeating one paints as if
    /// unanimated. Under reduced power with motion that follows the system a transition snaps, and
    /// a paint loop runs on at its 30 Hz.
    #[default]
    Decorative,
    /// Motion essential to what the content says, which runs whatever the preference: a spinner's
    /// turn shows work under way, a caret's blink where typing goes, as GTK, Windows and macOS
    /// blink a caret under their reduced-motion settings, and a demo of motion shows motion.
    Essential,
}
