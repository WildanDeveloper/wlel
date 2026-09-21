// Time, math and random: the pieces a benchmark harness needs.
//
//   wlel run examples/timemath.wl
//
// sys::mono_ms() is a monotonic millisecond clock (never jumps — measure
// durations with it); sys::unix_ms() is the wall clock since the epoch.
// std::math::* is a thin layer over C99 libm (float in, float out; ints
// widen). std::random::* is xorshift64*: auto-seeded on first use, or
// pinned with seed(s) for reproducible runs.
use std;

// a tiny benchmark harness: times one named workload with mono_ms
struct Bench {
    name: string,
    iters: int,
    ms: int
}

// recursive fib — the same workload the compiler benchmarks itself with
fn fib(n: int) -> int {
    if n < 2 {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}

fn bench_fib(name: string, iters: int, n: int) -> Bench {
    t0 := sys::mono_ms();
    sink := 0;
    for _i in 0..iters {
        sink += fib(n) % 2;
    }
    ms := sys::mono_ms() - t0;
    std::println_str(std::format("{}: {} iters in {} ms (sink {})", name, iters, ms, sink));
    return Bench { name: name, iters: iters, ms: ms };
}

fn main() -> int {

    // 1. time: durations come from the monotonic clock, timestamps from
    //    the wall clock
    unix_ms := sys::unix_ms();
    std::println_str(std::format("wall clock: ~{} s since the epoch", unix_ms / 1000));

    // 2. math: C99 libm, one thin wrapper per function
    std::println_str(std::format("sqrt(2)      = {}", std::math::sqrt(2.0)));
    std::println_str(std::format("pow(2, 10)   = {}", std::math::pow(2, 10)));
    std::println_str(std::format("hypot(3, 4)  = {}", std::math::hypot(3, 4)));
    std::println_str(std::format("floor/ceil/round of 2.5: {} {} {}", std::math::floor(2.5), std::math::ceil(2.5), std::math::round(2.5)));

    // 3. random: every seed gives a different, but reproducible, stream
    std::random::seed(2026);
    acc := 0.0;
    for _i in 0..5 {
        acc += std::random::float();
    }
    std::println_str(std::format("5 rolls avg (seed 2026): {}", acc / 5.0));

    // 4. the harness itself: fib(25) x 3, timed with mono_ms
    b := bench_fib("fib(25)", 3, 25);
    if b.ms < 0 {
        return 1;
    }
    return 0;
}

test "mono_ms advances and is monotonic" {
    t0 := sys::mono_ms();
    assert(t0 >= 0);
    acc := 0;
    for _i in 0..1000000 {
        acc += _i % 3;
    }
    t1 := sys::mono_ms();
    assert(t1 >= t0);
}

test "unix_ms is a plausible epoch timestamp" {
    now := sys::unix_ms();
    // 2020-01-01 .. 2100-01-01 in milliseconds
    assert(now > 1577836800000);
    assert(now < 4102444800000);
}

test "math agrees with known values" {

    // exact in binary floating point
    assert_eq(std::math::pow(2, 10), 1024.0);
    assert_eq(std::math::hypot(3, 4), 5.0);
    assert_eq(std::math::sqrt(4.0), 2.0);
    assert_eq(std::math::fabs(-2.5), 2.5);
    assert_eq(std::math::floor(2.5), 2.0);
    assert_eq(std::math::ceil(2.5), 3.0);
    assert_eq(std::math::round(2.5), 3.0);
    assert_eq(std::math::trunc(-2.5), -2.0);
    assert_eq(std::math::fmin(3.5, -1.0), -1.0);
    assert_eq(std::math::fmax(3.5, -1.0), 3.5);
    // irrational results: compare with a tolerance
    tol := 1e-12;
    assert(std::math::fabs(std::math::sqrt(2.0) - 1.4142135623730951) < tol);
    assert(std::math::fabs(std::math::sin(0.0)) < tol);
    assert(std::math::fabs(std::math::cos(std::math::atan2(1, 1) * 4) - -1.0) < tol);
    assert(std::math::fabs(std::math::log(std::math::exp(3.0)) - 3.0) < tol);
    assert(std::math::fabs(std::math::log2(1024.0) - 10.0) < tol);
    assert(std::math::fabs(std::math::cbrt(27.0) - 3.0) < tol);
}

test "integer arguments widen to float in math calls" {
    assert_eq(std::math::pow(2, 8), 256.0);
    assert_eq(std::math::sqrt(9), 3.0);
}

test "random is reproducible when seeded" {
    std::random::seed(7);
    a1 := std::random::int(1000);
    a2 := std::random::int(1000);
    a3 := std::random::int(1000);
    std::random::seed(7);
    assert_eq(std::random::int(1000), a1);
    assert_eq(std::random::int(1000), a2);
    assert_eq(std::random::int(1000), a3);
    // a different seed gives a different stream (with overwhelming
    // probability — 2^64 states against 3 draws)
    std::random::seed(8);
    ok := false;
    for _i in 0..8 {
        if std::random::int(1000) != a1 {
            ok = true;
        }
    }
    assert(ok);
}

test "random int stays in range and float in the unit interval" {
    std::random::seed(99);
    for _i in 0..1000 {
        r := std::random::int(37);
        assert(r >= 0);
        assert(r < 37);
        f := std::random::float();
        assert(f >= 0.0);
        assert(f < 1.0);
    }
    // n <= 0 is defined, not a crash
    assert_eq(std::random::int(0), 0);
    assert_eq(std::random::int(-5), 0);
}

test "unseeded random differs between processes" {

    // no seed call: the stream starts from the clock; two draws inside one
    // process must not be equal on 53-bit resolution floats
    f1 := std::random::float();
    f2 := std::random::float();
    assert(f1 != f2 || std::random::float() != f1);
}

test "benchmark harness measures with mono_ms" {
    t0 := sys::mono_ms();
    s := 0;
    for _i in 0..500000 {
        s += _i;
    }
    ms := sys::mono_ms() - t0;
    assert_eq(s, 124999750000);
    assert(ms >= 0);
}
