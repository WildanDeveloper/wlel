// Wide number types: u8/u16/u32/u64, i8/i32/i64, usize, f32 —
// `int` is an alias of i64, `float` of f64, `byte`/`char` of u8.
// Narrowing always requires an explicit cast.
fn saturate_u8(x: int) -> u8 {
    if x > 255 {
        return 255;
    }
    if x < 0 {
        return 0;
    }
    return x as u8;
}

fn main() -> int {

    // literal suffix: the value sticks to the type
    let small: u8 = 250u8;
    let medium: i32 = 2000000i32;
    let wide: u64 = 18000000000000000000u64;
    let precise: f32 = 0.25f32;

    // suffix-less literals adapt to the target type (range-checked)
    let tiny: u8 = 42;

    // same-width arithmetic: the result stays u8 (two's-complement wrap)
    let wrapped: u8 = small + 10;

    // mixed widths = error; an explicit cast is required
    let total: int = small as int + medium as int + wide as int + precise as int + tiny as int;

    // the u64 maximum can be written directly with a suffix
    let max_u64: u64 = 18446744073709551615u64;
    flags := if_stats(max_u64, wrapped as int, total);
    sys::exit(flags);
    return 0;
}

/// bool->int conversion via if: is the u64 max right? did the wrap happen? is the total positive?
fn if_stats(x: u64, w: int, t: int) -> int {
    out := 0;
    if x == 18446744073709551615u64 {
        out += 1;
    }
    if w == 4 {
        out += 10; // 250 + 10 wraps to 4 (mod 256)
    }
    if t > 0 {
        out += 100;
    }
    return out;
}

test "saturate clamps to the u8 range" {
    assert_eq(saturate_u8(300), 255);
    assert_eq(saturate_u8(-5), 0);
    assert_eq(saturate_u8(42), 42);
}

test "suffix and wrap per-width" {
    assert_eq(250u8 + 10, 4);
    assert_eq(18000000000000000000u64, 18000000000000000000u64);
    p := 0.25f32;
    assert_eq(p, 0.25f32);
}
