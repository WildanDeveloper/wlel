// FFI: `extern fn` declarations bind straight to C symbols at link time.
//
//   wlel run examples/ffi.wl
//
// The type checker guarantees the call shape; the linker guarantees the
// symbol. `wlel build -l <lib> -L <dir>` passes extra linker flags through
// to cc (repeatable, e.g. -l raylib -L raylib/lib). Variadic C functions
// (printf) cannot be declared — wrap them or use std::format.
//
// Declare the EXACT C types: C `int` is `i32` in Wlel (Wlel `int` is i64,
// C `long long`), C `size_t` is `usize`, C `double` is `f64`. Repeated
// compatible declarations are fine (the prelude already declares libc),
// but never redeclare a libc typedef — bind a name it does not define.
// Wlel structs are C structs (same order, same layout, verified with
// static_assert in the test suite), so third-party bindings (raylib,
// SDL) name their structs exactly as C does.
use std;

// libm (linked by default): double sqrt(double)
extern fn sqrt(x: f64) -> f64;

// libc: int puts(const char*), int atoi(const char*)
extern fn puts(s: string) -> i32;

extern fn atoi(s: string) -> i32;

// defer needs a void expression, so C calls hang off a void Wlel wrapper
fn say_bye() {
    puts("deferred C call");
}

fn main() -> int {
    r := sqrt(2.0);
    std::println_str(std::format("sqrt(2) = {}", r));
    std::println_str(std::format("atoi(\"42\") + 1 = {}", atoi("42") + 1));
    puts("hello from libc");
    return 0;
}

test "extern fns link and run" {
    assert_eq(sqrt(4.0), 2.0);
    assert_eq(atoi("123"), 123);
}

test "extern fns work inside arenas and defers" {
    arena(1024) {
        s := "17 is the answer";
        defer say_bye();
        assert_eq(atoi(s), 17);
    }
}
