//! Post-arrange hook registry. Widgets register hooks during
//! recording; the registry runs them between `layout.run` and
//! `cascades.run` so they see this frame's measured rects + content
//! extents and can update cross-frame state (`StateMap` rows) and
//! request a relayout when their record-time decisions used stale
//! data. See `docs/roadmap/deferred-shapes.md` (sibling concept) for
//! design rationale on the trait + typed-bucket storage.
//!
//! # Trait + typed buckets
//!
//! Anyone — built-in widgets here, downstream user code — implements
//! [`PostArrange`] on a `'static` struct. Storage is per-type: each
//! unique `T: PostArrange` gets its own `Vec<T>` bucket, type-erased
//! only at the bucket boundary. No `unsafe`, no payload size cap, no
//! per-entry heap alloc. One `Box::new` per unique `T` ever (not per
//! frame).
//!
//! # Relayout signal
//!
//! `run` returns `bool`: `true` requests a relayout pass. The registry
//! ORs every hook's return into a single output that
//! `Ui::end_frame_record_phase` propagates as the relayout signal.

use crate::forest::Forest;
use crate::layout::result::LayoutResult;
use crate::ui::state::StateMap;
use std::any::{Any, TypeId};

/// Implemented by any `'static` struct that wants to run post-arrange
/// logic. `run` reads the just-finished `LayoutResult`, optionally
/// mutates `StateMap` rows, and returns `true` if the widget's
/// record-time decisions were based on stale state and the frame
/// needs a relayout pass.
pub(crate) trait PostArrange: 'static {
    fn run(&self, forest: &Forest, results: &LayoutResult, state: &mut StateMap) -> bool;
}

trait TypedBucket: Any {
    fn clear(&mut self);
    fn run_all(&self, forest: &Forest, results: &LayoutResult, state: &mut StateMap) -> bool;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct Bucket<T: PostArrange> {
    entries: Vec<T>,
}

impl<T: PostArrange> Default for Bucket<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<T: PostArrange> TypedBucket for Bucket<T> {
    fn clear(&mut self) {
        self.entries.clear();
    }

    fn run_all(&self, forest: &Forest, results: &LayoutResult, state: &mut StateMap) -> bool {
        let mut relayout = false;
        for e in &self.entries {
            relayout |= e.run(forest, results, state);
        }
        relayout
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Default)]
pub(crate) struct PostArrangeRegistry {
    /// Linear scan ordered by first-push. Usually <10 unique hook
    /// types per frame; faster than `FxHashMap` at that N.
    buckets: Vec<(TypeId, Box<dyn TypedBucket>)>,
}

impl PostArrangeRegistry {
    pub(crate) fn begin_frame(&mut self) {
        for (_, b) in &mut self.buckets {
            b.clear();
        }
    }

    /// Register a post-arrange hook for this frame. The first push of
    /// any new `T` allocates one `Bucket<T>`; subsequent pushes (this
    /// frame and across frames) reuse it.
    pub(crate) fn push<T: PostArrange>(&mut self, hook: T) {
        self.bucket_mut::<T>().entries.push(hook);
    }

    /// Run every registered hook in bucket order, returning `true` if
    /// any hook requested a relayout. Called by
    /// `Ui::end_frame_record_phase` after `layout.run`.
    pub(crate) fn run_all(
        &self,
        forest: &Forest,
        results: &LayoutResult,
        state: &mut StateMap,
    ) -> bool {
        let mut relayout = false;
        for (_, b) in &self.buckets {
            relayout |= b.run_all(forest, results, state);
        }
        relayout
    }

    fn bucket_mut<T: PostArrange>(&mut self) -> &mut Bucket<T> {
        let tid = TypeId::of::<T>();
        let i = match self.buckets.iter().position(|(t, _)| *t == tid) {
            Some(i) => i,
            None => {
                let bucket: Box<dyn TypedBucket> = Box::new(Bucket::<T>::default());
                self.buckets.push((tid, bucket));
                self.buckets.len() - 1
            }
        };
        self.buckets[i]
            .1
            .as_any_mut()
            .downcast_mut()
            .expect("bucket TypeId always matches its concrete type")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Ui;
    use crate::layout::types::display::Display;
    use std::cell::Cell;

    /// Hook that ticks a thread-local counter and returns whatever
    /// relayout flag the caller seeded into a static cell. Two distinct
    /// types so we can exercise multiple buckets.
    struct A;
    struct B;

    thread_local! {
        static A_RUNS: Cell<u32> = const { Cell::new(0) };
        static B_RUNS: Cell<u32> = const { Cell::new(0) };
        static A_FLAG: Cell<bool> = const { Cell::new(false) };
    }

    fn reset_counters() {
        A_RUNS.with(|c| c.set(0));
        B_RUNS.with(|c| c.set(0));
        A_FLAG.with(|c| c.set(false));
    }

    impl PostArrange for A {
        fn run(&self, _: &Forest, _: &LayoutResult, _: &mut StateMap) -> bool {
            A_RUNS.with(|c| c.set(c.get() + 1));
            A_FLAG.with(|c| c.get())
        }
    }
    impl PostArrange for B {
        fn run(&self, _: &Forest, _: &LayoutResult, _: &mut StateMap) -> bool {
            B_RUNS.with(|c| c.set(c.get() + 1));
            false
        }
    }

    /// Run `body` against a real `Ui::end_frame_record_phase` so the
    /// registry sees the same `Forest` / `LayoutResult` / `StateMap`
    /// values production hooks see. Returns the relayout signal.
    fn drive(ui: &mut Ui, body: impl FnOnce(&mut PostArrangeRegistry)) -> bool {
        ui.begin_frame(Display::default());
        body(&mut ui.post_arrange);
        ui.end_frame_record_phase()
    }

    #[test]
    fn run_all_dispatches_every_bucket() {
        reset_counters();
        let mut ui = Ui::new();
        let _ = drive(&mut ui, |reg| {
            reg.push(A);
            reg.push(B);
            reg.push(A);
        });
        assert_eq!(A_RUNS.with(|c| c.get()), 2, "A bucket fired twice");
        assert_eq!(B_RUNS.with(|c| c.get()), 1, "B bucket fired once");
    }

    #[test]
    fn run_all_or_semantics_for_relayout_signal() {
        reset_counters();
        let mut ui = Ui::new();
        let relayout = drive(&mut ui, |reg| {
            reg.push(A);
            reg.push(B);
        });
        assert!(!relayout, "all hooks returned false → no relayout");

        reset_counters();
        A_FLAG.with(|c| c.set(true));
        let relayout = drive(&mut ui, |reg| {
            reg.push(A);
            reg.push(B);
        });
        assert!(relayout, "A returned true → relayout requested");
        assert_eq!(
            B_RUNS.with(|c| c.get()),
            1,
            "B still ran (no short-circuit)"
        );
    }

    #[test]
    fn buckets_are_retained_across_frames() {
        reset_counters();
        let mut ui = Ui::new();
        let _ = drive(&mut ui, |reg| reg.push(A));
        let after_first = ui.post_arrange.buckets.len();
        let _ = drive(&mut ui, |reg| reg.push(A));
        let after_second = ui.post_arrange.buckets.len();
        assert_eq!(
            after_first, after_second,
            "second frame must not allocate a new bucket for the same `T`",
        );
        assert_eq!(after_second, 1, "still only one bucket type seen");
    }

    #[test]
    fn begin_frame_clears_entries() {
        reset_counters();
        let mut ui = Ui::new();
        let _ = drive(&mut ui, |reg| {
            reg.push(A);
            reg.push(A);
        });
        assert_eq!(A_RUNS.with(|c| c.get()), 2);
        reset_counters();
        let _ = drive(&mut ui, |_| {});
        assert_eq!(
            A_RUNS.with(|c| c.get()),
            0,
            "begin_frame must clear pushed hooks from the previous frame",
        );
    }
}
