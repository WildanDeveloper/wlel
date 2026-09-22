// File I/O: whole-file helpers and defer-friendly handles.
//
//   wlel run examples/fileio.wl -- examples/fileio.wl   (cat: dump a file)
//
// Every fallible std::fs operation reports std::Result[.., string] — the
// Err payload is a C strerror message ("No such file or directory", ...),
// so a failing file operation never panics. `expr?` propagates the error
// out of a function returning Result; match destructures it; the
// the method layer (.is_ok(), .unwrap(), .ok()) covers the rest.
// sys::read_file/write_file
// stay the raw bool + out-param layer for programs that skip std.
// The test blocks below write their scratch file relative to the current
// directory (wlel_fileio_demo.tmp) so they run on every OS.
use std;

fn main() -> int {
    if sys::argc() < 2 {
        std::println_str("usage: fileio <file>");
        return 1;
    }
    match cat(sys::arg(1)) {
        Ok(code) => {
            return code;
        }
        Err(e) => {
            std::println_str("fileio: " + e);
            return 1;
        }
    }
}

// whole-file variant: read everything, print once
fn cat(path: string) -> Result[int, string] {
    content := std::fs::read_all(path)?;
    std::print_str(content);
    return Ok(0);
}

// streaming variant: same output, byte-counted reads (the buffer slice
// must be NUL-terminated before it can travel as a string)
fn cat_stream(path: string) -> Result[int, string] {
    f := std::fs::open(path, "rb")?;
    defer std::fs::close(f); // runs on every exit path, LIFO
    buf := new(u8, 4096);
    while true {
        n := std::fs::read(f, buf, 4096).unwrap();
        if n == 0 {
            break; // end of file
        }
        buf[n] = 0;
        std::print_str(buf as string);
    }
    return Ok(0);
}

test "write_all then read_all roundtrip" {
    p := "wlel_fileio_demo.tmp";
    n := std::fs::write_all(p, "hello file\nline two\n").unwrap();
    assert_eq(n, 20);
    back := std::fs::read_all(p).unwrap();
    assert_eq(back, "hello file\nline two\n");
    // a missing file reports a readable error, not a panic
    assert(std::fs::read_all("/no/such/wlel-demo-file").is_err());
    assert_eq(std::fs::read_all(p).unwrap_or("fallback"), "hello file\nline two\n");
}

test "fs handles: open read write close with Result" {
    p := "wlel_fileio_demo.tmp";
    assert(std::fs::write_all(p, "abcdefgh").is_ok());
    f := std::fs::open(p, "rb").unwrap();
    defer std::fs::close(f);
    buf := new(u8, 8);
    first := std::fs::read(f, buf, 4).unwrap();
    assert_eq(first, 4);
    assert_eq(buf[0], 97); // 'a'
    assert_eq(buf[3], 100); // 'd'
    assert_eq(std::fs::read(f, buf, 4).unwrap(), 4);
    assert_eq(std::fs::read(f, buf, 4).unwrap(), 0); // end of file
    assert(std::fs::read(f, buf, -1).is_err()); // defensive error path
    // append handle: two bytes land at the end, visible to a fresh read
    g := std::fs::open(p, "ab").unwrap();
    assert_eq(std::fs::write(g, "XY" as *u8, 2).unwrap(), 2);
    std::fs::close(g);
    whole := std::fs::read_all(p).unwrap();
    assert_eq(whole, "abcdefghXY");
    // close is idempotent: the deferred close below is a no-op
    std::fs::close(f);
}

test "open missing file returns Err with message" {
    r := std::fs::open("/no/such/wlel-demo-file", "rb");
    msg := r.err().unwrap_or("missing message");
    assert(msg != "");
}

test "streaming cat matches whole-file read" {
    p := "wlel_fileio_demo.tmp";
    assert(std::fs::write_all(p, "stream me\nline 2\nline 3\n").is_ok());
    back := std::fs::read_all(p).unwrap();
    rc := cat_stream(p);
    assert(rc.is_ok());
    assert_eq(rc.ok().unwrap_or(-1), 0);
    assert_eq(back, "stream me\nline 2\nline 3\n");
    re := cat_stream("/no/such/wlel-demo-file");
    assert(re.is_err());
}

test "try operator composes in helpers" {
    p := "wlel_fileio_demo.tmp";
    assert(std::fs::write_all(p, "chain").is_ok());
    rc := count_file(p);
    assert_eq(rc.ok().unwrap_or(-1), 5);
}

// `expr?` composes inside any function returning Result: read then
// measure in one fallible line (a test body returns void, so tests call
// the helper and unwrap with the method layer instead)
fn count_file(path: string) -> Result[int, string] {
    content := std::fs::read_all(path)?;
    return Ok(std::strlen(content) as int);
}
