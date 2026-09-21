// args.wl — v0.6 sys:: builtins: real CLI programs
// usage: wlel run examples/args.wl -- hello world 123
use std;

fn main() -> int {
    argc := sys::argc();
    std::println_str("== sys::args ==");
    std::println_str("argument count:");
    std::println_int(argc);

    for i in 0..argc {
        std::println_str(sys::arg(i));
    }

    if argc > 1 {
        first := sys::arg(1);
        if first == "greet" {
            if argc > 2 {
                std::println_str("hello, " + sys::arg(2) + "!");
            } else {
                std::println_str("hello, world!");
            }
        }
    }
    return 0;
}
