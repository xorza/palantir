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

## Dead surface

- [ ] `ClipMode::is_rounded` (`src/layout/types/clip_mode.rs:31`) has no caller
      anywhere in the crate.

- [ ] `IconSet::nominal` (`src/icons/icon_set.rs:110`) has no caller and returns
      exactly `self.handle(icon).view_box`, which `IconHandle` already carries as
      a public field.
