// Semantik overflow Wlel:
//   - unsigned: wrap modulo 2^N (standar C)
//   - signed: two's-complement wrap (backend dikompilasi dengan -fwrapv)
//   - width < 32 bit: compiler menyisipkan cast wrap eksplisit
// Deteksi overflow: std::checked_add/sub/mul(a, b, &out) -> bool
//   false saat overflow; hasil hanya valid saat true.
//
// Verifikasi: exit code 1111 berarti semua properti benar
// (1 wrap-i64, 10 wrap-u8, 100 checked_add menangkap, 1000 checked_mul normal;
// 1111 & 0xFF = 87 karena exit code C 8-bit).

use std;

fn main() -> int {
    // 1) i64: INT64_MAX + 1 wrap ke INT64_MIN (-fwrapv)
    let max: int = 9_223_372_036_854_775_807;
    let wrapped: int = max + 1;

    // 2) u8: 250 + 10 wrap ke 4 (mod 256)
    let b: u8 = 250u8 + 10;

    // 3) checked_add menangkap overflow
    r := 0;
    let ok: bool = std::checked_add(max, 1, &r);

    // 4) checked_sub pada nilai aman tetap bekerja
    r2 := 0;
    let ok2: bool = std::checked_sub(10, 4, &r2);

    score := 0;
    if wrapped == -9_223_372_036_854_775_807 - 1 { score += 1; }
    if b == 4 { score += 10; }
    if !ok { score += 100; }
    if ok2 && r2 == 6 { score += 1000; }

    std::println_int(score);
    sys::exit(score);
    return 0;
}
