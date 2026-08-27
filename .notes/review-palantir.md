# Palantir crate review

Findings from a read of `src/` (~86.6k non-test lines, 505 files). Each item is
a checklist entry: **when you address one, delete it.** The file lists open
findings only — no "done" markers, no resolved section.

Findings are grouped by the root cause they share, and the groups are ordered by
severity and benefit. Descriptions state what is wrong and where; they
deliberately do not propose fixes.

Two things are deliberately out of scope: the record-time-geometry limitation
(already surveyed in `.notes/record-time-geometry.md`) and test structure.
Behavioural defects found along the way are logged separately in
`.notes/ISSUES.md` rather than here.

---

## Policies that spend more than their doc claims

- [ ] `build_mask_plan` (`src/renderer/backend/schedule/mod.rs:29`)
      deduplicates a group's stencil mask chain against **the previous group
      only**. A group with no scissor resets `previous_chain` to empty, so two
      groups sharing one rounded-clip chain with any scissor-less group
      between them each stage their own copy of the mask quads, and the
      schedule then clears and re-stamps the chain between them. Nothing in
      the module says the dedup is one-deep.

- [ ] `RasterAtlas::evict_one`
      (`src/renderer/backend/raster_atlas/mod.rs:664`) latches
      `Side::dry_frame` for the rest of the frame the moment one clock
      rotation finds no victim. `allocate` (`:593`) can grow the side after
      that latch, which adds slots the clock could sweep — but the latch is
      keyed on the frame, not on the side's generation, so no further
      eviction runs until the next frame however much the atlas changed.

---

## Dead surface

- [ ] `ClipMode::is_rounded` (`src/layout/types/clip_mode.rs:31`) has no caller
      anywhere in the crate.

- [ ] `IconSet::nominal` (`src/icons/icon_set.rs:110`) has no caller and returns
      exactly `self.handle(icon).view_box`, which `IconHandle` already carries as
      a public field.
