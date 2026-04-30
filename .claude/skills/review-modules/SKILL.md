---
name: review-modules
description: Review one or more modules/folders for bad architectural decisions, possible simplifications, and other improvements. The user provides the target paths as arguments. Use when the user asks for an architecture review, code-quality pass, or simplification audit of specific folders/files.
tools: Read, Glob, Grep, Agent
---

# Review modules

Review the modules/folders the user supplies as arguments. Look for bad decisions, architectural simplifications, and other improvements and simplifications.

## Inputs

The user provides one or more paths (folders or files) on invocation. Treat those as the review scope. Do not assume any specific folder — if no path is supplied, ask the user which paths to review.

## What to look for

- **Bad architectural decisions** — misplaced responsibilities, leaky abstractions, layering violations, coupling that should be inverted, types/fields that don't belong where they live.
- **Possible simplifications** — code that can be deleted, merged, or replaced with a simpler form; redundant indirection; premature abstraction; over-generic APIs with one caller; dead branches; unnecessary state.
- **Consistency** — naming, error handling, ownership/borrowing patterns, module boundaries that drift across files.
- **Correctness smells** — invariants enforced by convention rather than types, easy-to-misuse APIs, footguns, off-by-one risks at module seams.
- **Performance smells only when they're cheap wins** — unnecessary clones/allocations, O(n) where O(1) is trivial, hot-path heap traffic. Don't speculate about performance without a reason.
- **Tests and contracts** — semantics that should be pinned by tests but aren't; tests that overspecify implementation.

## What NOT to do


- Don't list nits (formatting, doc wording) unless they actually mislead.
- Don't repeat what the code obviously does — say what's wrong or could be better and why.
- Don't invent new features. Scope is the existing code.
- Don't write code yet. This is review-only unless the user explicitly asks for fixes.

## Process

1. Read every file under the supplied paths. For larger scopes, fan out with the Explore agent in parallel, but synthesize findings yourself — don't delegate judgment.
2. Build a mental model of how the modules fit together before critiquing.
3. Cross-reference: when a finding spans files, cite each `path:line`.
4. Re-read the project's `CLAUDE.md` / `DESIGN.md` if present — align critique with the project's stated design intent rather than generic best practices.

## Output format

Group findings by severity, most impactful first:

### Architectural issues
Each item: one-sentence claim, then 1–3 lines of reasoning, then `path:line` references and a concrete suggested change.

### Simplifications
Same format. Prefer "delete X" / "merge X into Y" / "replace X with Y" over abstract advice.

### Smaller improvements
Bulleted, terse. One line each.

### Open questions
Things that look suspicious but need user context to judge. Ask them directly.

End with a short prioritized shortlist (3–5 items) of what you'd change first if the user said "go."
