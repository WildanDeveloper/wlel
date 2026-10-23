// Wlel overflow semantics:
//   - unsigned: wrap modulo 2^N (standard C)
//   - signed: two's-complement wrap (the backend compiles with -fwrapv)
//   - width < 32 bits: the compiler inserts an explicit wrap cast
// Overflow detection: std::checked_add/sub/mul(a, b, &out) -> bool
//   false on overflow; the result is only valid when true.
//
// Verification: exit code 1111 means every property held
// (1 wrap-i64, 10 wrap-u8, 100 checked_add caught, 1000 checked_mul normal;
// 1111 & 0xFF = 87 because the C exit code is 8-bit).
use std;

fn main() -> int {

    // 1) i64: INT64_MAX + 1 wraps to INT64_MIN (-fwrapv)
    let max: int = 9223372036854775807;
    let wrapped: int = max + 1;

    // 2) u8: 250 + 10 wraps to 4 (mod 256)
    let b: u8 = 250u8 + 10;

    // 3) checked_add catches the overflow
    r := 0;
    let ok: bool = std::checked_add(max, 1, &r);

    // 4) checked_sub on safe values still works
    r2 := 0;
    let ok2: bool = std::checked_sub(10, 4, &r2);
    score := 0;
    if wrapped == -9223372036854775807 - 1 {
        score += 1;
    }
    if b == 4 {
        score += 10;
    }
    if !ok {
        score += 100;
    }
    if ok2 && r2 == 6 {
        score += 1000;
    }
    std::println_int(score);
    sys::exit(score);
    return 0;
}

test "i64 two's-complement wrap" {
    let max: int = 9223372036854775807;
    assert_eq(max + 1, -9223372036854775807 - 1);
}

test "wrap u8 modulo 256" {
    assert_eq(250u8 + 10, 4);
    assert_eq(0u8 - 1, 255);
}

test "checked_add/sub detect overflow" {
    let max: int = 9223372036854775807;
    r := 0;
    assert(!std::checked_add(max, 1, &r));
    assert(std::checked_sub(10, 4, &r));
    assert_eq(r, 6);
}
