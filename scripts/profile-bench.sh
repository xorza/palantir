#!/usr/bin/env bash
# Record a criterion bench with samply and emit a headless text report
# (top self-time + inclusive). macOS counterpart to bench-perf.sh.
#
# Outputs:
#   tmp/profile-<bench>.json   - raw samply profile (open with: samply load <file>)
#   tmp/profile-<bench>.txt    - flat self + inclusive top-functions report
#
# Usage:
#   scripts/profile-bench.sh                            # default: frame bench, --profile-time 5
#   scripts/profile-bench.sh --profile-time 10
#   BENCH=frame FILTER='end_frame$' scripts/profile-bench.sh
#
# Env:
#   BENCH   bench target name (default: frame)
#   FILTER  criterion filter regex (default: empty = all cases)
#
# Requires: samply, atos (Xcode CLT), python3, otool.

set -euo pipefail

cd "$(dirname "$0")/.."

BENCH_NAME="${BENCH:-frame}"
FILTER_ARG="${FILTER:-}"
EXTRA_ARGS=("$@")
if [ ${#EXTRA_ARGS[@]} -eq 0 ]; then
    EXTRA_ARGS=(--profile-time 5)
fi

for cmd in samply atos python3 otool jq; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "error: '$cmd' not on PATH" >&2
        exit 1
    fi
done

mkdir -p tmp
PROFILE_JSON="tmp/profile-${BENCH_NAME}.json"
REPORT="tmp/profile-${BENCH_NAME}.txt"

echo "==> Building bench '$BENCH_NAME'"
cargo bench --bench "$BENCH_NAME" --no-run 2>&1 | tail -3

BENCH_BIN=$(ls -t "target/release/deps/${BENCH_NAME}"-* 2>/dev/null \
    | grep -vE '\.(d|dSYM)$' | head -1)
if [ -z "$BENCH_BIN" ] || [ ! -x "$BENCH_BIN" ]; then
    echo "error: could not locate built bench binary for '$BENCH_NAME'" >&2
    exit 1
fi
echo "    binary: $BENCH_BIN"
if [ -n "$FILTER_ARG" ]; then
    echo "    filter: $FILTER_ARG"
fi

BENCH_ARGS=(--bench)
[ -n "$FILTER_ARG" ] && BENCH_ARGS+=("$FILTER_ARG")
BENCH_ARGS+=("${EXTRA_ARGS[@]}")

echo "==> samply record -> $PROFILE_JSON"
rm -f "$PROFILE_JSON"
samply record --save-only -o "$PROFILE_JSON" "$BENCH_BIN" "${BENCH_ARGS[@]}"

echo "==> Symbolicating + aggregating"
python3 - "$PROFILE_JSON" "$BENCH_BIN" > "$REPORT" <<'PY'
import json, sys, subprocess, re, collections

prof_path, bin_path = sys.argv[1], sys.argv[2]
prof = json.load(open(prof_path))

# Locate the bench binary's lib index by basename match.
import os
target_name = os.path.basename(bin_path)
lib_idx = next((i for i, lib in enumerate(prof["libs"]) if lib["name"] == target_name), None)
if lib_idx is None:
    print(f"error: lib '{target_name}' not found in profile", file=sys.stderr); sys.exit(1)

# __TEXT vmaddr -> load addr for atos.
otool = subprocess.run(["otool", "-l", bin_path], capture_output=True, text=True, check=True).stdout
m = re.search(r"segname __TEXT\b.*?vmaddr\s+(0x[0-9a-fA-F]+)", otool, re.S)
load_addr = int(m.group(1), 16) if m else 0x100000000

t = prof["threads"][0]
st, ft, fnt, samps = t["stackTable"], t["frameTable"], t["funcTable"], t["samples"]["stack"]

# Collect unique RVAs from frames that belong to the bench binary.
addrs = sorted({ ft["address"][f]
                 for f in range(ft["length"])
                 if fnt["resource"][ft["func"][f]] == lib_idx })

# Batch-symbolicate via atos stdin (avoids inline-arg limit + 64-bit awk bug).
abs_addrs = "\n".join(f"0x{load_addr + a:x}" for a in addrs) + "\n"
res = subprocess.run(["atos", "-o", bin_path, "-l", f"0x{load_addr:x}"],
                     input=abs_addrs, capture_output=True, text=True, check=True)
syms = res.stdout.splitlines()

def clean(sym: str) -> str:
    sym = re.sub(r" \(in [^)]+\)(?: \([^)]+\))?(?: \+ \d+)?$", "", sym)
    sym = re.sub(r"::h[0-9a-f]{16}", "", sym)
    sym = (sym.replace("$LT$", "<").replace("$GT$", ">")
              .replace("$C$", ",").replace("$u20$", " ")
              .replace("$u27$", "'").replace("..", "::"))
    return sym.lstrip("_")

addr2sym = { a: clean(s) for a, s in zip(addrs, syms) }

def walk(s):
    while s is not None:
        yield s
        s = st["prefix"][s]

def sym_for(sidx):
    f = st["frame"][sidx]
    if fnt["resource"][ft["func"][f]] != lib_idx: return None
    return addr2sym.get(ft["address"][f], f"0x{ft['address'][f]:x}")

incl = collections.Counter(); self_ = collections.Counter()
total = 0
for s in samps:
    if s is None: continue
    total += 1
    seen = set(); leaf_done = False
    for sidx in walk(s):
        sym = sym_for(sidx)
        if sym is None: continue
        if not leaf_done:
            self_[sym] += 1; leaf_done = True
        if sym not in seen:
            incl[sym] += 1; seen.add(sym)

print(f"# bench:  {target_name}")
print(f"# samples (in-binary frames): {total}")
print(f"# profile: {prof_path}\n")
print("## Self-time (top 25)")
print(f"{'pct':>6}  {'n':>5}  function")
for s, n in self_.most_common(25):
    print(f"{100*n/total:5.1f}%  {n:5d}  {s}")

SKIP_PREFIX = ("criterion::", "std::", "core::ops::function", "alloc::rt::")
SKIP_CONTAINS = ("rust_begin_short_backtrace", "lang_start", "c_with_alloca",
                 "alloca::trampoline", "Routine::bench", "Routine::warm_up",
                 "Routine::profile", "Bencher", "BenchmarkGroup")
def harness(s):
    return s.startswith(SKIP_PREFIX) or any(k in s for k in SKIP_CONTAINS) \
        or s in ("main", "frame::main")

print("\n## Inclusive (top 25, criterion/std harness filtered)")
print(f"{'pct':>6}  {'n':>5}  function")
shown = 0
for s, n in incl.most_common():
    if harness(s): continue
    print(f"{100*n/total:5.1f}%  {n:5d}  {s}")
    shown += 1
    if shown >= 25: break
PY

echo
cat "$REPORT"
echo
echo "Profile JSON  : $PROFILE_JSON"
echo "Text report   : $REPORT"
echo "Interactive   : samply load $PROFILE_JSON"
