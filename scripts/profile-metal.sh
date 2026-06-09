#!/usr/bin/env bash
# Capture a Metal System Trace of the showcase binary (or an example)
# via xctrace, plus enable the Metal HUD overlay for live frame/GPU-time
# numbers.
#
# Outputs:
#   tmp/metal-<target>.trace   - Instruments trace bundle (open in Xcode/Instruments)
#
# Usage:
#   scripts/profile-metal.sh                     # showcase binary, 10s
#   scripts/profile-metal.sh counter             # an example instead
#   DURATION=5 scripts/profile-metal.sh          # shorter capture
#   HUD=0 scripts/profile-metal.sh               # skip the live HUD overlay
#
# Env:
#   DURATION   seconds to capture (default 10)
#   HUD        set MTL_HUD_ENABLED=1 (default 1; 0 to disable)
#   FEATURES   cargo features
#
# Requires: xcrun (Xcode Command Line Tools).
# View the trace with:
#   open tmp/metal-<target>.trace          (Instruments.app)
# or
#   xcrun xctrace export --input tmp/metal-<target>.trace --xpath '/trace-toc/run/data/table[@schema="metal-pass"]'  for headless dump

set -euo pipefail

cd "$(dirname "$0")/.."

TARGET="${1:-showcase}"
DURATION="${DURATION:-10}"
HUD="${HUD:-1}"
FEATURES_ARG="${FEATURES:-}"

if ! command -v xcrun >/dev/null 2>&1; then
    echo "error: xcrun not on PATH — install Xcode Command Line Tools" >&2
    exit 1
fi

mkdir -p tmp
TRACE="tmp/metal-${TARGET}.trace"
rm -rf "$TRACE"

# The showcase lives in src/main.rs (the package binary); everything
# else resolves as a cargo example.
if [ "$TARGET" = "showcase" ]; then
    echo "==> Building showcase binary (release)"
    BUILD_ARGS=(build --release --bin palantir)
    BIN="target/release/palantir"
else
    echo "==> Building example '$TARGET' (release)"
    BUILD_ARGS=(build --release --example "$TARGET")
    BIN="target/release/examples/${TARGET}"
fi
[ -n "$FEATURES_ARG" ] && BUILD_ARGS+=(--features "$FEATURES_ARG")
cargo "${BUILD_ARGS[@]}" 2>&1 | tail -3

if [ ! -x "$BIN" ]; then
    echo "error: target binary not found at $BIN" >&2
    exit 1
fi
echo "    binary: $BIN"
echo "    duration: ${DURATION}s"
[ "$HUD" = "1" ] && echo "    Metal HUD: enabled (MTL_HUD_ENABLED=1)"

# Safety: MTL debug layer + shader validation silently tank GPU
# performance. Refuse to capture with them enabled.
for var in MTL_DEBUG_LAYER MTL_SHADER_VALIDATION; do
    val="${!var:-}"
    if [ -n "$val" ] && [ "$val" != "0" ]; then
        echo "error: \$$var=$val is set — would distort GPU timings. Unset it." >&2
        exit 1
    fi
done

ENV_ARGS=()
[ "$HUD" = "1" ] && ENV_ARGS+=(--env "MTL_HUD_ENABLED=1")

echo "==> xctrace record -> $TRACE  (Ctrl+C the target after ${DURATION}s if it doesn't self-exit)"
# `--time-limit` stops xctrace; the target keeps running until it
# also self-terminates or you Ctrl+C the window. For a window-based
# target (showcase, counter) close the window to exit cleanly.
xcrun xctrace record \
    --template 'Metal System Trace' \
    --output "$TRACE" \
    --time-limit "${DURATION}s" \
    --launch "${ENV_ARGS[@]}" -- \
    "$BIN" || true

if [ ! -d "$TRACE" ]; then
    echo "error: trace bundle not created — xctrace may have failed" >&2
    exit 1
fi

echo
echo "Trace        : $TRACE"
echo "Open in GUI  : open '$TRACE'"
echo
echo "What to look for:"
echo "  * GPU timeline gaps (CPU not feeding the GPU fast enough)"
echo "  * Per-pass duration: 'palantir.renderer.main.pass' should dominate"
echo "  * Sub-pass debug groups: preclear / mask / quads / text / meshes"
echo "  * Encode→submit→GPU-execute latency for steady-state frames"
