// defer.wl — LIFO cleanup at scope exit, incl. early returns & loops
fn work(n: int) -> int {
    defer wlel_print_str("  cleanup work\n");
    if n > 10 {
        wlel_print_str("  big, returning early\n");
        return n;
    }
    wlel_print_str("  small path\n");
    return 0;
}

fn main() -> int {
    defer wlel_print_str("3\n");
    defer wlel_print_str("2\n");
    defer wlel_print_str("1\n");

    wlel_print_str("go\n");
    wlel_print_str("loop:");
    i := 0;
    while i < 3 {
        defer wlel_print_str(" iter");
        i = i + 1;
    }
    wlel_print_str("\nnested:\n");
    {
        defer wlel_print_str(" inner-defer\n");
        wlel_print_str(" inner-body\n");
    }
    wlel_print_str("call:\n");
    work(99);
    return 0;
}
