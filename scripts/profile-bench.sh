#!/usr/bin/env bash
# Record a criterion bench with samply and emit a headless text report
# (top self-time + inclusive + callers/callees). macOS counterpart to
# bench-perf.sh.
#
# Outputs:
#   tmp/profile-<bench>.json   - raw samply profile (open with: samply load <file>)
#   tmp/profile-<bench>.txt    - flat self + inclusive top-functions report
#
# Usage:
#   scripts/profile-bench.sh                            # default: frame bench, --profile-time 5
#   scripts/profile-bench.sh --profile-time 10
#   BENCH=frame FILTER='end_frame$' scripts/profile-bench.sh
#   BENCH=stroke_tessellate FEATURES=internals scripts/profile-bench.sh
#   MIN_PCT=1.0 TOPN=15 scripts/profile-bench.sh           # tweak cutoffs
#
# Env:
#   BENCH     bench target name              (default: frame)
#   FILTER    criterion filter regex         (default: empty = all cases)
#   FEATURES  cargo features, comma-list     (default: empty)
#   TOPN      rows per section               (default: 20)
#   MIN_PCT   drop entries under this %      (default: 0.5)
#   CONTEXT   show callers/callees per hot   (default: 1, 0 to skip)
#
# Requires: samply, atos (Xcode CLT), python3, otool, jq.
# Optional: rustfilt (cargo install rustfilt) for cleaner Rust v0 demangling.

set -euo pipefail

cd "$(dirname "$0")/.."

BENCH_NAME="${BENCH:-frame}"
FILTER_ARG="${FILTER:-}"
FEATURES_ARG="${FEATURES:-}"
TOPN="${TOPN:-20}"
MIN_PCT="${MIN_PCT:-0.5}"
CONTEXT="${CONTEXT:-1}"
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
BUILD_ARGS=(bench --bench "$BENCH_NAME" --no-run)
[ -n "$FEATURES_ARG" ] && BUILD_ARGS+=(--features "$FEATURES_ARG")
cargo "${BUILD_ARGS[@]}" 2>&1 | tail -3

BENCH_BIN=$(ls -t "target/release/deps/${BENCH_NAME}"-* 2>/dev/null \
    | grep -vE '\.(d|dSYM)$' | head -1)
if [ -z "$BENCH_BIN" ] || [ ! -x "$BENCH_BIN" ]; then
    echo "error: could not locate built bench binary for '$BENCH_NAME'" >&2
    exit 1
fi
echo "    binary: $BENCH_BIN"
[ -n "$FILTER_ARG" ] && echo "    filter: $FILTER_ARG"

BENCH_ARGS=(--bench)
[ -n "$FILTER_ARG" ] && BENCH_ARGS+=("$FILTER_ARG")
BENCH_ARGS+=("${EXTRA_ARGS[@]}")

echo "==> samply record -> $PROFILE_JSON"
rm -f "$PROFILE_JSON"
samply record --save-only -o "$PROFILE_JSON" "$BENCH_BIN" "${BENCH_ARGS[@]}"

echo "==> Symbolicating + aggregating"
TOPN="$TOPN" MIN_PCT="$MIN_PCT" CONTEXT="$CONTEXT" \
python3 - "$PROFILE_JSON" "$BENCH_BIN" > "$REPORT" <<'PY'
import json, sys, os, subprocess, re, collections, shutil

prof_path, bin_path = sys.argv[1], sys.argv[2]
TOPN = int(os.environ.get("TOPN", 20))
MIN_PCT = float(os.environ.get("MIN_PCT", 0.5))
CONTEXT = os.environ.get("CONTEXT", "1") != "0"
prof = json.load(open(prof_path))

target_name = os.path.basename(bin_path)
lib_idx = next((i for i, lib in enumerate(prof["libs"]) if lib["name"] == target_name), None)
if lib_idx is None:
    sys.exit(f"error: lib '{target_name}' not in profile")

otool = subprocess.run(["otool", "-l", bin_path], capture_output=True, text=True, check=True).stdout
m = re.search(r"segname __TEXT\b.*?vmaddr\s+(0x[0-9a-fA-F]+)", otool, re.S)
load_addr = int(m.group(1), 16) if m else 0x100000000

interval_ms = float(prof["meta"].get("interval", 1.0))
# Single-thread benches; aggregate just thread 0. Multi-thread support would
# need per-thread breakdown — out of scope for criterion benches.
t = prof["threads"][0]
st, ft, fnt = t["stackTable"], t["frameTable"], t["funcTable"]
samps_stack = t["samples"]["stack"]
samps_w = t["samples"].get("weight")  # may be None when weightType='samples'

# Inline expansion: atos -i prints one line per inline level (outermost first),
# blank line between addresses. The deepest inlined function is the last
# non-blank line and is what should own self-time at that address.
addrs = sorted({ ft["address"][f]
                 for f in range(ft["length"])
                 if fnt["resource"][ft["func"][f]] == lib_idx })
abs_in = "\n".join(f"0x{load_addr + a:x}" for a in addrs) + "\n"
res = subprocess.run(["atos", "-i", "-o", bin_path, "-l", f"0x{load_addr:x}"],
                     input=abs_in, capture_output=True, text=True, check=True)
# Split output into per-address blocks. atos with -i separates inline groups
# with a blank line; without inline, each line is its own address.
blocks_out = res.stdout.split("\n")
# atos emits groups separated by blank lines only when there's inline depth.
# In practice each address always produces one or more contiguous lines,
# terminated by a blank line. Easiest robust parse: walk and split on blanks.
groups = []
cur = []
for line in blocks_out:
    if line.strip() == "":
        if cur:
            groups.append(cur); cur = []
    else:
        cur.append(line)
if cur: groups.append(cur)
# Fallback when no blank separators are present (single-line-per-addr case):
if len(groups) == 1 and len(groups[0]) == len(addrs):
    groups = [[ln] for ln in groups[0]]

if len(groups) != len(addrs):
    # Degrade gracefully: re-symbolicate without -i.
    res = subprocess.run(["atos", "-o", bin_path, "-l", f"0x{load_addr:x}"],
                         input=abs_in, capture_output=True, text=True, check=True)
    groups = [[ln] for ln in res.stdout.splitlines()]

# rustfilt if available — handles legacy + v0 mangling correctly.
RUSTFILT = shutil.which("rustfilt")

def manual_demangle(sym: str) -> str:
    sym = re.sub(r" \(in [^)]+\)(?: \([^)]+\))?(?: \+ \d+)?$", "", sym)
    sym = re.sub(r"::h[0-9a-f]{16}", "", sym)
    sym = (sym.replace("$LT$", "<").replace("$GT$", ">")
              .replace("$C$", ",").replace("$u20$", " ")
              .replace("$u27$", "'").replace("..", "::"))
    return sym.lstrip("_")

def strip_atos_suffix(sym: str) -> str:
    return re.sub(r" \(in [^)]+\)(?: \([^)]+\))?(?: \+ \d+)?$", "", sym)

if RUSTFILT:
    # One rustfilt call: feed all lines, keep group boundaries via line count.
    flat_in = [strip_atos_suffix(ln) for g in groups for ln in g]
    r = subprocess.run([RUSTFILT], input="\n".join(flat_in) + "\n",
                       capture_output=True, text=True, check=True)
    flat_out = r.stdout.splitlines()
    it = iter(flat_out)
    groups = [[next(it) for _ in g] for g in groups]
else:
    groups = [[manual_demangle(ln) for ln in g] for g in groups]

# Map address -> (outer_sym, leaf_sym). Leaf is the deepest inline level.
addr_info = {}
for addr, g in zip(addrs, groups):
    if not g:
        addr_info[addr] = (f"0x{addr:x}", f"0x{addr:x}")
    else:
        addr_info[addr] = (g[0], g[-1])

def walk(s):
    while s is not None:
        yield s
        s = st["prefix"][s]

incl = collections.Counter()
self_ = collections.Counter()
callers = collections.defaultdict(collections.Counter)  # callee -> caller -> count
callees = collections.defaultdict(collections.Counter)  # caller -> callee -> count
total = 0

for i, s in enumerate(samps_stack):
    if s is None: continue
    w = samps_w[i] if samps_w else 1
    total += w

    # Walk: collect leaf syms for each frame in stack (leaf first → caller last).
    frame_syms = []
    for sidx in walk(s):
        f = st["frame"][sidx]
        if fnt["resource"][ft["func"][f]] != lib_idx: continue
        _, leaf = addr_info[ft["address"][f]]
        frame_syms.append(leaf)

    if not frame_syms: continue
    self_[frame_syms[0]] += w
    seen = set()
    for sym in frame_syms:
        if sym not in seen:
            incl[sym] += w
            seen.add(sym)
    # Caller/callee edges (immediate neighbors only).
    for a, b in zip(frame_syms, frame_syms[1:]):
        callers[a][b] += w
        callees[b][a] += w

if total == 0:
    sys.exit("error: no in-binary samples — bench may have exited before sampling")

dur_s = total * interval_ms / 1000.0
print(f"# bench:    {target_name}")
print(f"# filter:   {os.environ.get('SAMPLY_FILTER','(see invocation)')}")
print(f"# samples:  {total} in-binary  ({dur_s:.2f}s @ {interval_ms:.2f} ms/sample)")
print(f"# profile:  {prof_path}")
print(f"# cutoff:   >= {MIN_PCT}%   top {TOPN} per section")
if not RUSTFILT:
    print(f"# (install rustfilt for clean Rust v0 demangling)")
print()

SKIP_PREFIX = ("criterion::", "std::", "core::ops::function", "alloc::rt::")
SKIP_CONTAINS = ("rust_begin_short_backtrace", "lang_start", "c_with_alloca",
                 "alloca::trampoline", "as criterion::", "criterion::routine::",
                 "criterion::bencher::", "criterion::benchmark_group::")
def is_harness(s):
    return s.startswith(SKIP_PREFIX) or any(k in s for k in SKIP_CONTAINS) \
        or s in ("main", "frame::main")

def fmt_row(pct_n, ms, sym):
    return f"{pct_n[0]:5.1f}%  {pct_n[1]:6d}  {ms:7.1f}ms  {sym}"

def topn_filtered(counter, drop_harness, limit):
    out = []
    # Stable tie-break: sort by (-count, name).
    items = sorted(counter.items(), key=lambda kv: (-kv[1], kv[0]))
    for sym, n in items:
        pct = 100 * n / total
        if pct < MIN_PCT: break
        if drop_harness and is_harness(sym): continue
        out.append((sym, n, pct))
        if len(out) >= limit: break
    return out

print("## Self-time")
print(f"  {'pct':>5}  {'samp':>6}  {'wall':>7}  function")
self_top = topn_filtered(self_, drop_harness=False, limit=TOPN)
for sym, n, pct in self_top:
    print(fmt_row((pct, n), n * interval_ms, sym))

print("\n## Inclusive (criterion/std harness filtered)")
print(f"  {'pct':>5}  {'samp':>6}  {'wall':>7}  function")
for sym, n, pct in topn_filtered(incl, drop_harness=True, limit=TOPN):
    print(fmt_row((pct, n), n * interval_ms, sym))

if CONTEXT:
    print("\n## Callers/callees for top self-time entries")
    for sym, n, pct in self_top[:min(5, len(self_top))]:
        print(f"\n  {pct:5.1f}%  {sym}")
        top_callers = sorted(callers[sym].items(), key=lambda kv: (-kv[1], kv[0]))[:3]
        top_callees = sorted(callees[sym].items(), key=lambda kv: (-kv[1], kv[0]))[:3]
        if top_callers:
            print(f"      called by:")
            for c, cn in top_callers:
                print(f"        {100*cn/total:5.1f}%  {c}")
        if top_callees:
            print(f"      calls into:")
            for c, cn in top_callees:
                print(f"        {100*cn/total:5.1f}%  {c}")
PY

echo
cat "$REPORT"
echo
echo "Profile JSON  : $PROFILE_JSON"
echo "Text report   : $REPORT"
echo "Interactive   : samply load $PROFILE_JSON"
