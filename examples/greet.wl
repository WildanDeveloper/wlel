// greet.wl — strings, floats, and forward references
fn fib(n: int) -> int {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}

fn banner() -> void {
    wlel_print_str("== Wlel demo ==\n");
}

fn main() -> int {
    banner();
    let pi: float = 3 + 0.14;
    if pi > 3.0 {
        wlel_print_str("pi looks fine\n");
    }
    wlel_print_int(fib(10));
    return 0;
}
