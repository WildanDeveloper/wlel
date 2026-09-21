// strings.wl — v0.6 string ops: concat (+), equality (==), strlen
// Concatenation allocates from the ACTIVE arena (or heap if none) —
// arena-first, just like everything else in Wlel.
use std;

fn main() -> int {
    std::println_str("== string ops ==");

    // concat with +
    lang := "Wlel";
    tagline := "the " + "arena-first" + " systems language";
    std::println_str("Hello, " + lang + "! " + tagline);

    // equality via ==
    if "abc" == "abc" {
        std::println_str("literal == literal: yes");
    }
    if lang != "Rust" {
        std::println_str("lang != Rust: yes");
    }

    // strlen
    std::println_int(std::strlen(tagline));

    // concat inside an arena: all temporary strings die together
    arena(1024) {
        csv := "";
        for _i in 1..6 {
            // `_` prefix: intentionally unused loop var
            csv = csv + sys::arg(0); // reuse argv[0] as filler text
        }
        std::println_int(std::strlen(csv));
    } // every intermediate concat freed here, O(1)
    return 0;
}

test "concat and comparison" {
    assert_eq("Wlel" + "!", "Wlel!");
    assert("arena" != "borrow");
    assert_eq("abc", "abc");
}

test "strlen" {
    assert_eq(std::strlen("hello"), 5);
    assert_eq(std::strlen(""), 0);
}
