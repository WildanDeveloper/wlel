// Tipe bilangan lebar: u8/u16/u32/u64, i8/i32/i64, usize, f32 —
// `int` adalah alias i64, `float` alias f64, `byte`/`char` alias u8.
// Pengecilan (narrowing) wajib cast eksplisit.

fn saturate_u8(x: int) -> u8 {
    if x > 255 { return 255; }
    if x < 0 { return 0; }
    return x as u8;
}

fn main() -> int {
    // literal suffix: nilai menempel pada tipenya
    let small: u8 = 250u8;
    let medium: i32 = 2_000_000i32;
    let wide: u64 = 18_000_000_000_000_000_000u64;
    let precise: f32 = 0.25f32;

    // literal tanpa suffix menyesuaikan tipe target (range-checked)
    let tiny: u8 = 42;

    // aritmetika se-width: hasil tetap u8 (wrap two's-complement)
    let wrapped: u8 = small + 10;

    // campuran width = error; cast eksplisit diperlukan
    let total: int = (small as int)
        + (medium as int)
        + (wide as int)
        + (precise as int)
        + (tiny as int);

    // u64 maksimum dapat ditulis langsung dengan suffix
    let max_u64: u64 = 18_446_744_073_709_551_615u64;
    let flags := if_stats(max_u64, wrapped as int, total);

    sys::exit(flags);
    return 0;
}

/// konversi bool->int lewat if: u64 max benar? wrap terjadi? total positif?
fn if_stats(x: u64, w: int, t: int) -> int {
    out := 0;
    if x == 18_446_744_073_709_551_615u64 { out += 1; }
    if w == 4 { out += 10; }        // 250 + 10 wrap ke 4 (mod 256)
    if t > 0 { out += 100; }
    return out;
}
