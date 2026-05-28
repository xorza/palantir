# Contributing to Palantir

Thanks for your interest in contributing. A few things to know before you
open a PR.

## Licensing and the CLA

Palantir is dual-licensed (GPL-3.0-or-later + a separate commercial
license). To preserve this model, every contribution must be made under
the terms of the [Contributor License Agreement](CLA.md).

The CLA grants the maintainer the right to relicense your contribution —
including under proprietary commercial terms — while you retain copyright
and the right to use your contribution for any other purpose.

**You accept the CLA by signing off each commit:**

    git commit -s -m "your message"

This adds a trailer like:

    Signed-off-by: Your Real Name <your.email@example.com>

Your sign-off certifies that you have read [CLA.md](CLA.md) and agree to
its terms for the commits in your pull request. PRs without sign-offs on
every commit will not be merged.

Use your real name and a reachable email. Pseudonymous sign-offs are not
accepted.

## Before you open a PR

Run, in this order:

    cargo fmt --all
    cargo clippy --all-targets -- -D warnings
    cargo test

For changes that touch feature-gated code, use the full feature matrix:

    scripts/test-all.sh

See [CLAUDE.md](CLAUDE.md) for the project's coding conventions.

## Scope

Palantir is pre-1.0 and breaks freely. Before investing time in a large
change, open an issue to discuss direction — speculative refactors with no
motivating workload tend to be shelved.
