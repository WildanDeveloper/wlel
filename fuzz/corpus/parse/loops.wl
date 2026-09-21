// loops.wl — v0.6 for-in range loop demo
use std;

fn main() -> int {
    std::println_str("== for-in ranges ==");

    // basic range loop
    total := 0;
    for i in 0..10 {
        total += i; // 0 + 1 + ... + 9 = 45
    }
    std::println_int(total);

    // for loop with continue & defer
    odds := 0;
    for x in 1..11 {
        if (x & 1) == 0 {
            continue;
        }
        odds += x; // 1 + 3 + 5 + 7 + 9 = 25
    }
    std::println_int(odds);

    // nested 2D grid iteration
    matches := 0;
    for r in 0..4 {
        for c in 0..4 {
            if r == c {
                matches += 1;
            }
        }
    }
    std::println_int(matches); // 4 diagonal elements
    return 0;
}
