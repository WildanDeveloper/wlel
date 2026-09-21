#!/usr/bin/env bash
# Benchmark trend for CI (Linux, release build). Emits github-action-benchmark
# "customSmallerIsBetter" JSON: [{name, unit, value}].
#
#   scripts/bench.sh benchmark-data.json
#
# Metrics (median of 5 runs):
#   fib(35)          — runtime parity with C (the headline number)
#   sort 1M ints     — std::sort < 100 ms criterion (internal mono_ms timing)
#   compile kitchen  — front-end speed (parse+check+codegen, no cc)
set -euo pipefail

OUT="${1:?usage: bench.sh <out.json>}"
WLEL="./target/release/wlel"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# median of 5 wall-clock runs of "$@", in milliseconds (µs clock → awk)
median_ms() {
    local vals=() t0 t1 v
    for _ in 1 2 3 4 5; do
        t0=${EPOCHREALTIME/./}
        "$@" >/dev/null 2>&1
        t1=${EPOCHREALTIME/./}
        vals+=( $(( t1 - t0 )) )
    done
    printf '%s\n' "${vals[@]}" | sort -n | sed -n '3p' | awk '{printf "%.1f", $1/1000}'
}

# --- 1. fib(35): runtime ----------------------------------------------------
cat > "$TMP/fib35.wl" <<'EOF'
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
"$WLEL" build "$TMP/fib35.wl" -O2 -o "$TMP/fib35"
fib_ms=$(median_ms "$TMP/fib35")

# --- 2. sort 1M ints: internal timing printed by examples/sort.wl -----------
"$WLEL" build examples/sort.wl -O2 -o "$TMP/sort"
sort_vals=()
for _ in 1 2 3 4 5; do
    line=$("$TMP/sort" | grep 'sorted .* ints in ')
    sort_vals+=( "$(sed -n 's/.* in \([0-9][0-9]*\) ms.*/\1/p' <<<"$line")" )
done
sort_ms=$(printf '%s\n' "${sort_vals[@]}" | sort -n | sed -n '3p')

# --- 3. front-end compile speed (no cc in the loop) -------------------------
"$WLEL" build examples/kitchen.wl --emit-c -o "$TMP/kitchen.c"
compile_ms=$(median_ms "$WLEL" build examples/kitchen.wl --emit-c -o "$TMP/kitchen.c")

# --- emit JSON ---------------------------------------------------------------
{
    printf '{"name":"fib(35) runtime","unit":"ms","value":%s}\n' "$fib_ms"
    printf '{"name":"sort 1M ints","unit":"ms","value":%s}\n' "$sort_ms"
    printf '{"name":"compile kitchen.wl (front-end)","unit":"ms","value":%s}\n' "$compile_ms"
} > "$OUT"
cat "$OUT"
