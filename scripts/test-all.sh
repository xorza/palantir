#!/usr/bin/env bash
# Run fmt + strict public docs + clippy + tests across every feature
# combination.
#
# Why: features can wire in distinct code paths (e.g. `internals`
# unlocks the damage-visualization fixtures in `tests/visual/`). A
# clean run with one combo doesn't prove the others compile; this
# script makes the matrix explicit.
#
# Combos covered:
#   - no default features (host-less: the renderer without `winit-host`,
#                         the shape an embedding host compiles)
#   - default features   (production-shaped build: winit host + clipboard)
#   - internals          (cache helpers + render-debug knobs +
#                         damage fixtures — the test reach-ins, no harness)
#   - bench              (internals + the colocated criterion/dhat drivers
#                         and the `benches/` targets that link them)
#   - showcase           (bundled widget-tour binary + logging setup)
#   - profile-with-tracy (the supported profiler backend)
#   - all features       (aggregate compatibility)
#
# The full run checks:
#   1. cargo fmt --all                     (once, up front)
#   2. strict cargo doc --no-deps          (once, up front)
#   3. cargo clippy --all-targets --features <combo> -- -D warnings
#   4. cargo test --features <combo>       (unit + integration + doctests)
#
# Usage:
#   scripts/test-all.sh           # full matrix
#   FAST=1 scripts/test-all.sh    # skip fmt + docs + clippy, run tests only

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
  ""                       # default features
  "internals"
  "bench"
  "showcase"
  "profile-with-tracy"
)

if [[ "${FAST:-0}" != "1" ]]; then
  banner "fmt --check"
  cargo fmt --all

  banner "docs --deny-warnings"
  RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
fi

banner "features = <no-default>"
if [[ "${FAST:-0}" != "1" ]]; then
  step "clippy"
  cargo clippy --all-targets --no-default-features -- -D warnings
fi

step "test"
cargo test --no-default-features

for features in "${COMBOS[@]}"; do
  label="${features:-<default>}"
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

banner "features = <all>"
if [[ "${FAST:-0}" != "1" ]]; then
  step "clippy"
  cargo clippy --all-targets --all-features -- -D warnings
fi

step "test"
cargo test --all-features

printf '\n%sall combos passed%s\n' "$green" "$reset"
