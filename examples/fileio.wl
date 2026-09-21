// File I/O: whole-file helpers and defer-friendly handles.
//
//   wlel run examples/fileio.wl -- examples/fileio.wl   (cat: dump a file)
//
// sys::read_file/write_file load or store an entire file as one string;
// std::fs::open/read/write/close stream through a *File handle. Strings are
// NUL-terminated, so binary content belongs in *u8 buffers (std::fs::read).
// The test blocks below write their scratch file relative to the current
// directory (wlel_fileio_demo.tmp) so they run on every OS.
use std;

fn main() -> int {
    if sys::argc() < 2 {
        std::println_str("usage: fileio <file>");
        return 1;
    }
    path := sys::arg(1);
    content := "";
    if !sys::read_file(path, &content) {
        std::println_str("fileio: cannot read " + path);
        return 1;
    }
    std::print_str(content);
    return 0;
}

// streaming variant: same output, byte-counted reads (the buffer slice
// must be NUL-terminated before it can travel as a string)
fn cat_stream(path: string) -> int {
    f := std::fs::open(path, "rb");
    if f == 0 as *File {
        return 1;
    }
    defer std::fs::close(f); // runs on every exit path, LIFO
    buf := new(u8, 4096);
    while true {
        n := std::fs::read(f, buf, 4096);
        if n <= 0 {
            break; // 0 = end of file, -1 = error
        }
        buf[n] = 0;
        std::print_str(buf as string);
    }
    return 0;
}

test "write_file then read_file roundtrip" {
    p := "wlel_fileio_demo.tmp";
    assert(sys::write_file(p, "hello file\nline two\n"));
    back := "";
    assert(sys::read_file(p, &back));
    assert_eq(back, "hello file\nline two\n");
    // a missing file reports failure and leaves *out untouched
    assert(!sys::read_file("/no/such/wlel-demo-file", &back));
    assert_eq(back, "hello file\nline two\n");
}

test "fs handles: open read write close" {
    p := "wlel_fileio_demo.tmp";
    assert(sys::write_file(p, "abcdefgh"));
    f := std::fs::open(p, "rb");
    assert(f != 0 as *File);
    defer std::fs::close(f);
    buf := new(u8, 8);
    assert_eq(std::fs::read(f, buf, 4), 4);
    assert_eq(buf[0], 97); // 'a'
    assert_eq(buf[3], 100); // 'd'
    assert_eq(std::fs::read(f, buf, 4), 4);
    assert_eq(std::fs::read(f, buf, 4), 0); // end of file
    assert_eq(std::fs::read(f, buf, -1), -1); // defensive error path
    // append handle: two bytes land at the end, visible to a fresh read
    g := std::fs::open(p, "ab");
    assert(g != 0 as *File);
    assert_eq(std::fs::write(g, "XY" as *u8, 2), 2);
    std::fs::close(g);
    whole := "";
    assert(sys::read_file(p, &whole));
    assert_eq(whole, "abcdefghXY");
    // close is idempotent: the deferred close below is a no-op
    std::fs::close(f);
}

test "open missing file returns null" {
    f := std::fs::open("/no/such/wlel-demo-file", "rb");
    assert(f == 0 as *File);
}

test "streaming cat matches whole-file read" {
    p := "wlel_fileio_demo.tmp";
    assert(sys::write_file(p, "stream me\nline 2\nline 3\n"));
    back := "";
    assert(sys::read_file(p, &back));
    assert_eq(cat_stream(p), 0);
    assert_eq(back, "stream me\nline 2\nline 3\n");
    assert_eq(cat_stream("/no/such/wlel-demo-file"), 1);
}
