#!/usr/bin/env bash
# Benchmark trend for CI (Linux, release build). Emits github-action-benchmark
# "customSmallerIsBetter" JSON: [{name, unit, value}].
#
#   scripts/bench.sh benchmark-data.json
#
# Metrics (fastest of 7 runs):
#   fib(35)          — runtime parity with C (the headline number)
#   sort 1M ints     — std::sort on random input (internal mono_ms timing)
#   sort 1M sorted   — std::sort on pre-sorted input: guards the partition
#                      against the O(n^2) shape regression
#   compile kitchen  — front-end speed (parse+check+codegen, no cc; the
#                      `.c` output target stops before the link step)
#
# The estimator is the MINIMUM, not the median. Every one of these numbers is
# a wall clock on a shared runner, where interference can only ever add time:
# a median of 5 samples still lands on a contended sample often enough to
# swing fib(35) between 11 and 20 ms run to run, which tripped the trend
# alert on a commit whose generated C was byte-identical to the previous one.
# The fastest sample is the closest estimate of the true cost.
set -euo pipefail

OUT="${1:?usage: bench.sh <out.json>}"
WLEL="./target/release/wlel"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# fastest of 7 wall-clock runs of "$@", in milliseconds (µs clock → awk)
min_ms() {
    local vals=() t0 t1
    for _ in 1 2 3 4 5 6 7; do
        t0=${EPOCHREALTIME/./}
        "$@" >/dev/null 2>&1
        t1=${EPOCHREALTIME/./}
        vals+=( $(( t1 - t0 )) )
    done
    printf '%s\n' "${vals[@]}" | sort -n | sed -n '1p' | awk '{printf "%.1f", $1/1000}'
}

# fastest of 7 samples of an internally-timed metric: read the number that
# precedes " ms" on the line matching $1, from the output of $2
min_internal_ms() {
    local pattern="$1"; shift
    local vals=() line v
    for _ in 1 2 3 4 5 6 7; do
        line=$("$@" | grep -- "$pattern" | head -1)
        v=$(sed -n 's/.* \([0-9][0-9]*\) ms.*/\1/p' <<<"$line")
        [ -n "$v" ] && vals+=( "$v" )
    done
    printf '%s\n' "${vals[@]}" | sort -n | sed -n '1p'
}

# --- 1. fib(35): runtime ----------------------------------------------------
cat > "$TMP/fib35.wlel" <<'EOF'
fn fib(n: int) -> int {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> int {
    wlel_print_int(fib(35));
    return 0;
}
EOF
"$WLEL" build "$TMP/fib35.wlel" -O2 -o "$TMP/fib35"
fib_ms=$(min_ms "$TMP/fib35")

# --- 2. sort 1M ints: internal timing printed by examples/sort.wlel -----------
"$WLEL" build examples/sort.wlel -O2 -o "$TMP/sort"
sort_ms=$(min_internal_ms 'sorted .* ints in ' "$TMP/sort")

# --- 2b. sort 1M already-sorted ints: worst-case guard ----------------------
cat > "$TMP/sortshapes.wlel" <<'EOF'
use std;
fn asc(a: int, b: int) -> int {
    if a < b { return -1; }
    if a > b { return 1; }
    return 0;
}
fn main() -> int {
    n := 1000000;
    d := new(int, n);
    for i in 0..n { d[i] = i; }
    t0 := sys::mono_ms();
    std::sort(d, n, asc);
    std::println_str(std::format("asc {} ms", sys::mono_ms() - t0));
    for i in 0..n { d[i] = n - i; }
    t0 = sys::mono_ms();
    std::sort(d, n, asc);
    std::println_str(std::format("desc {} ms", sys::mono_ms() - t0));
    return 0;
}
EOF
"$WLEL" build "$TMP/sortshapes.wlel" -O2 -o "$TMP/sortshapes"
sorted_ms=$(min_internal_ms 'asc ' "$TMP/sortshapes")

# --- 3. front-end compile speed (no cc in the loop) -------------------------
"$WLEL" build examples/kitchen.wlel --emit-c -o "$TMP/kitchen.c"
compile_ms=$(min_ms "$WLEL" build examples/kitchen.wlel --emit-c -o "$TMP/kitchen.c")

# --- emit JSON ---------------------------------------------------------------
# github-action-benchmark customSmallerIsBetter wants ONE JSON array, not
# newline-delimited objects
{
    printf '[\n'
    printf '  {"name":"fib(35) runtime","unit":"ms","value":%s},\n' "$fib_ms"
    printf '  {"name":"sort 1M ints","unit":"ms","value":%s},\n' "$sort_ms"
    printf '  {"name":"sort 1M pre-sorted ints","unit":"ms","value":%s},\n' "$sorted_ms"
    printf '  {"name":"compile kitchen.wlel (front-end)","unit":"ms","value":%s}\n' "$compile_ms"
    printf ']\n'
} > "$OUT"
cat "$OUT"
