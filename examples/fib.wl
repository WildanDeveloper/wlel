// fib.wl — classic recursion test
fn fib(n: int) -> int {
    if n < 2 { return n; }
    return fib(n - 1) + fib(n - 2);
}

fn main() -> int {
    wlel_print_int(fib(28));
    return 0;
}
