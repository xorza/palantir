#!/usr/bin/env bash
# Run fmt + clippy + tests across every feature combination.
#
# Why: features can wire in distinct code paths (e.g. `internals`
# unlocks the damage-visualization fixtures in `tests/visual/`). A
# clean run with one combo doesn't prove the others compile; this
# script makes the matrix explicit.
#
# Combos covered:
#   - no features        (production-shaped build)
#   - internals          (cache helpers + render-debug knobs +
#                         damage fixtures)
#   - bench-deep         (criterion `caches.rs` bench gating; needs
#                         `internals` to actually run, but should
#                         still type-check standalone)
#   - internals + bench-deep (the full superset)
#
# Each combo runs:
#   1. cargo fmt --all -- --check          (once, up front)
#   2. cargo clippy --all-targets --features <combo> -- -D warnings
#   3. cargo test --features <combo>       (unit + integration + doctests)
#
# Usage:
#   scripts/test-all.sh           # full matrix
#   FAST=1 scripts/test-all.sh    # skip fmt + clippy, run tests only

set -euo pipefail

cd "$(dirname "$0")/.."

# ANSI helpers — quiet on dumb terminals / CI logs.
if [[ -t 1 ]]; then
  bold=$'\033[1m'; dim=$'\033[2m'; green=$'\033[32m'; reset=$'\033[0m'
else
  bold=""; dim=""; green=""; reset=""
fi

banner() { printf '\n%s== %s ==%s\n' "$bold" "$1" "$reset"; }
step()   { printf '%s-> %s%s\n' "$dim" "$1" "$reset"; }

COMBOS=(
  ""                       # no features
  "internals"
  "bench-deep"
  "internals bench-deep"
)

if [[ "${FAST:-0}" != "1" ]]; then
  banner "fmt --check"
  cargo fmt --all
fi

for features in "${COMBOS[@]}"; do
  label="${features:-<none>}"
  banner "features = $label"

  if [[ "${FAST:-0}" != "1" ]]; then
    step "clippy"
    if [[ -z "$features" ]]; then
      cargo clippy --all-targets -- -D warnings
    else
      cargo clippy --all-targets --features "$features" -- -D warnings
    fi
  fi

  step "test"
  if [[ -z "$features" ]]; then
    cargo test
  else
    cargo test --features "$features"
  fi
done

printf '\n%sall combos passed%s\n' "$green" "$reset"
