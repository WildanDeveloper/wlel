// bitwise.wl — v0.4 demo: compound assign, break, continue, bitwise, radix
use std;

fn popcount(n: int) -> int {
    count := 0;
    v := n;
    while v != 0 {
        count += v & 1;
        v = v >> 1;
    }
    return count;
}

fn main() -> int {
    std::println_str("== bitwise & loop demo ==");

    // radix literals & bitwise ops
    mask := 240;
    val := 170; // 10101010
    both := mask & val; // 10100000 = 160
    std::println_int(both);

    // popcount of 0x7F = 7
    std::println_int(popcount(127));

    // compound assign with break/continue
    sum := 0;
    i := 0;
    while true {
        i += 1;
        if (i & 1) == 0 {
            continue;
        }
        if i > 10 {
            break;
        }
        sum += i; // 1 + 3 + 5 + 7 + 9 = 25
    }
    std::println_int(sum);
    defer std::println_str("all ok");
    return 0;
}
