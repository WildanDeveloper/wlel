// strlib.wl — std string library demo (Fase 2)
// str_find/str_sub/str_trim/str_split/str_parse_int/int_to_str ditulis di
// Wlel sendiri (embedded in the compiler, monomorphized on demand);
// std::format / std::parse_float / std::float_to_str are runtime builtins.
// Semua string baru dialokasikan di arena aktif — zero leak by construction.
use std;

fn main() -> int {
    std::println_str("== Wlel String Library Demo ==");

    // search
    std::println_int(str_find("arena-first", "first"));

    // slice + trim
    std::println_str(str_sub("arena-first", 6, 5));
    std::println_str("<" + str_trim("   center   ") + ">");

    // split + iterate
    csv := "rust,c,wlel,,zig";
    for lang in str_split(csv, ",") {
        std::println_str("- " + lang);
    }

    // parse + format round-trip
    n := 0;
    if str_parse_int("  -42", &n) {
        std::println_str("parsed");
    } else {        std::println_str("strict: whitespace rejected");
    }
    if str_parse_int(str_trim("  -42 "), &n) {
        std::println_str(std::format("parsed {}", n * 2));
    }

    // string indexing reads bytes (u8 — cast for arithmetic)
    word := "wlel";
    std::println_str(std::format("first byte = {}", word[0] as int));
    return 0;
}

test "find and find_from" {
    assert_eq(str_find("hello world", "world"), 6);
    assert_eq(str_find("hello", "xyz"), -1);
    assert_eq(str_find("aaa", "a"), 0);
    assert_eq(str_find("abc", ""), 0);
    assert_eq(str_find_from("aXaXa", "Xa", 2), 3);
    assert_eq(str_find_from("aXaXa", "Xa", 4), -1);
}

test "sub clamps to bounds" {
    assert_eq(str_sub("hello world", 6, 5), "world");
    assert_eq(str_sub("hello", 0, 100), "hello");
    assert_eq(str_sub("hello", 3, 2), "lo");
    assert_eq(str_sub("hello", 10, 1), "");
    assert_eq(str_sub("hello", -1, 3), "hel");
    assert_eq(str_sub("hello", 2, 0), "");
}

test "trim removes ascii whitespace" {
    assert_eq(str_trim("  hi  "), "hi");
    assert_eq(str_trim("\t\nhi\r\n"), "hi");
    assert_eq(str_trim("   "), "");
    assert_eq(str_trim(""), "");
    assert_eq(str_trim("no-touch"), "no-touch");
    assert_eq(str_trim(" mid dle "), "mid dle");
}

test "split yields empty pieces" {
    parts := str_split("a,b,,c", ",");
    assert_eq(parts.len(), 4);
    assert_eq(parts.get(0), "a");
    assert_eq(parts.get(2), "");
    assert_eq(parts.get(3), "c");
    one := str_split("solo", ",");
    assert_eq(one.len(), 1);
    assert_eq(one.get(0), "solo");
    empty := str_split("", ",");
    assert_eq(empty.len(), 1);
    assert_eq(empty.get(0), "");
    trail := str_split("x,", ",");
    assert_eq(trail.len(), 2);
    assert_eq(trail.get(1), "");
}

test "parse_int strict and overflow-safe" {
    n := 0;
    assert(str_parse_int("42", &n));
    assert_eq(n, 42);
    assert(str_parse_int("-42", &n));
    assert_eq(n, -42);
    assert(str_parse_int("+7", &n));
    assert_eq(n, 7);
    assert(str_parse_int("9223372036854775807", &n));
    assert_eq(n, 9223372036854775807);
    assert(str_parse_int("-9223372036854775808", &n));
    assert_eq(n, -9223372036854775807 - 1);
    assert(!str_parse_int("9223372036854775808", &n));
    assert(!str_parse_int("", &n));
    assert(!str_parse_int("-", &n));
    assert(!str_parse_int("  1", &n));
    assert(!str_parse_int("99999999999999999999", &n));
    // on false *out is untouched
    keep := 5;
    assert(!str_parse_int("zz", &keep));
    assert_eq(keep, 5);
}

test "int_to_str round-trips" {
    assert_eq(int_to_str(0), "0");
    assert_eq(int_to_str(7), "7");
    assert_eq(int_to_str(-987654321), "-987654321");
    assert_eq(int_to_str(9223372036854775807), "9223372036854775807");
    assert_eq(int_to_str(-9223372036854775807 - 1), "-9223372036854775808");
    n := 0;
    assert(str_parse_int(int_to_str(-123456789), &n));
    assert_eq(n, -123456789);
    assert(str_parse_int(int_to_str(-9223372036854775807 - 1), &n));
    assert_eq(n, -9223372036854775807 - 1);
}

test "format round-trip across kinds" {
    assert_eq(std::format("{} {}", 1, "x"), "1 x");
    assert_eq(std::format("{} + {} = {}", 2, 3, 5), "2 + 3 = 5");
    assert_eq(std::format("on = {}", true), "on = true");
    assert_eq(std::format("off = {}", false), "off = false");
    assert_eq(std::format("pi ~ {}", 3.5), "pi ~ 3.5");
    assert_eq(std::format("no slots", 1, 2), "no slots");
    assert_eq(std::format("brace { literal"), "brace { literal");
    assert_eq(std::format("surplus {} stays", "one"), "surplus one stays");
    // format -> parse round trip
    n := 0;
    assert(str_parse_int(std::format("{}", 1234), &n));
    assert_eq(n, 1234);
}

test "parse_float strict" {
    f := 0.0;
    assert(std::parse_float("2.5", &f));
    assert_eq(f, 2.5);
    assert(std::parse_float("-0.5", &f));
    assert_eq(f, -0.5);
    assert(!std::parse_float("1.5x", &f));
    assert(!std::parse_float("", &f));
    keep := 1.0;
    assert(!std::parse_float("zz", &keep));
    assert_eq(keep, 1.0);
}

test "string indexing reads bytes" {
    s := "abc";
    assert_eq(s[0] as int, 97);
    assert_eq(s[2] as int, 99);
    assert_eq(std::strlen(s), 3);
    assert(s[3] == 0);
}
