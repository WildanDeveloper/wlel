// httpd.wl — flagship app 2: an HTTP server on raw sockets, written in Wlel.
//
//   wlel run flagship/httpd.wl                 — echo + built-in index on :8080
//   wlel run flagship/httpd.wl -- 9000 ./site  — port + static docroot
//   wlel test flagship/httpd.wl                — end-to-end test over localhost
//   wlel build flagship/httpd.wl -O2 -o httpd
//
// POSIX sockets are bound through `extern fn` (libc): socket/bind/listen/
// accept/recv/send/close plus htons/htonl for byte order — no wrapper
// library, the C ABI is the API. Connections are served by a worker pool
// (sys::thread + sys::chan_new[int]): the accept loop pushes client fds,
// workers pull them, and each request runs inside its own `arena(1 << 20)`
// block so per-request memory frees in O(1).
//
// Endpoints:
//   GET  /            — built-in index page (or docroot/index.html)
//   GET  /path        — static file under the docroot (if given), path is
//                       sanitized: ".." segments are rejected (403)
//   POST/PUT /echo    — responds with the request body verbatim
//   anything else     — 404 (405 for non-GET on static paths)
//
// Routes reply with `Connection: close` and the worker closes the fd after
// one request — a deliberately small honest HTTP/1.1 subset. Text-oriented:
// bodies/bytes travel as NUL-terminated strings, so embedded NULs truncate
// (echo of binary payloads and binary static files are out of scope for the
// flagship demo). POSIX only: a Windows build maps this to winsock2
// (WSAStartup, SOCKET, closesocket) — noted in the roadmap, same wire code.
use std;

// ---------------------------------------------------------------------------
// libc socket FFI (Linux/macOS LP64: C int = i32, size_t = usize,
// ssize_t = i64). Wlel structs are C structs, so sockaddr_in is declared
// with the exact POSIX layout — no headers, no bindings layer.
// ---------------------------------------------------------------------------
extern fn socket(domain: i32, ty: i32, protocol: i32) -> i32;

extern fn bind(fd: i32, addr: *void, len: u32) -> i32;

extern fn listen(fd: i32, backlog: i32) -> i32;

extern fn accept(fd: i32, addr: *void, len: *u32) -> i32;

extern fn connect(fd: i32, addr: *void, len: u32) -> i32;

extern fn recv(fd: i32, buf: *void, n: usize, flags: i32) -> i64;

extern fn send(fd: i32, buf: *void, n: usize, flags: i32) -> i64;

extern fn close(fd: i32) -> i32;

extern fn shutdown(fd: i32, how: i32) -> i32;

extern fn htons(x: u16) -> u16;

extern fn htonl(x: u32) -> u32;

extern fn getsockname(fd: i32, addr: *void, len: *u32) -> i32;

extern fn setsockopt(fd: i32, level: i32, name: i32, val: *void, len: u32) -> i32;

extern fn signal(sig: i32, handler: *void) -> *void;

/// sockaddr_in, exact POSIX layout (16 bytes, no padding)
struct SockaddrIn {
    family: u16,
    port: u16,
    addr: u32,
    zero: [u8; 8]
}

// ---------------------------------------------------------------------------
// server plumbing
// ---------------------------------------------------------------------------

/// shared worker context: the job channel and the static docroot ("" =
/// built-in routes only). The string is immutable and shared read-only —
/// the honest cross-thread aliasing discipline documented in the spec.
struct Ctx {
    jobs: *Chan[int],
    root: string
}

/// acceptor loop context: the listening fd + the job channel
struct Ac {
    lfd: i32,
    jobs: *Chan[int]
}

/// create + bind + listen; returns the listening fd or -1
fn http_bind(port: int) -> i32 {
    // SIG_IGN for SIGPIPE must be process-wide before any traffic: a peer
    // closing early must not kill a worker (or the test runner)
    signal(13, 1 as *void);
    fd := socket(2, 1, 0); // AF_INET, SOCK_STREAM
    if fd < 0 {
        return -1;
    }
    one := 1;
    setsockopt(fd, 1, 2, &one as *void, 4u32); // SOL_SOCKET, SO_REUSEADDR
    sa := SockaddrIn { family: 2u16, port: htons(port as u16), addr: htonl(0), zero: [0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8] }; // AF_INET // INADDR_ANY
    if bind(fd, &sa as *void, 16u32) != 0 {
        close(fd);
        return -1;
    }
    if listen(fd, 64) != 0 {
        close(fd);
        return -1;
    }
    return fd;
}

/// the port an ephemeral bind (port 0) actually received
fn http_port(fd: i32) -> int {
    sa := SockaddrIn { family: 0u16, port: 0u16, addr: 0u32, zero: [0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8] };
    l := 16u32;
    if getsockname(fd, &sa as *void, &l) != 0 {
        return -1;
    }
    p := sa.port as int; // little-endian read of network-order bytes
    hi := p & 255; // low byte of the LE read = high byte of the host port
    lo := p >> 8 & 255;
    return hi * 256 + lo;
}

/// bind the first free port in a small explicit range — loopback connects
/// to kernel-picked ephemeral ports are rejected by some host firewalls
/// (ufw here rejects them even for 127.0.0.1), while explicit test-range
/// ports are reachable everywhere
fn http_bind_test() -> i32 {
    for p in 18400..18420 {
        fd := http_bind(p);
        if fd >= 0 {
            return fd;
        }
    }
    return -1;
}

/// send a whole string (loops over partial sends); false on hard error
fn send_all(fd: i32, s: string) -> bool {
    n := std::strlen(s);
    off := 0;
    while off < n {
        w := send(fd, (s as *u8 as usize + off as usize) as *void, (n - off) as usize, 0);
        if w <= 0 {
            return false;
        }
        off += w;
    }
    return true;
}

/// write one fully-formed response
fn respond(fd: i32, code: int, reason: string, ctype: string, body: string) {
    head := std::format("HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", code, reason, ctype, std::strlen(body));
    send_all(fd, head);
    if std::strlen(body) > 0 {
        send_all(fd, body);
    }
}

/// minimal MIME table by extension (text-safe file serving)
fn mime_of(path: string) -> string {
    if str_find(path, ".html") >= 0 || str_find(path, ".htm") >= 0 {
        return "text/html; charset=utf-8";
    }
    if str_find(path, ".css") >= 0 {
        return "text/css; charset=utf-8";
    }
    if str_find(path, ".js") >= 0 {
        return "text/javascript; charset=utf-8";
    }
    if str_find(path, ".json") >= 0 {
        return "application/json";
    }
    if str_find(path, ".txt") >= 0 {
        return "text/plain; charset=utf-8";
    }
    return "application/octet-stream";
}

/// the built-in index page (no docroot configured)
fn index_page() -> string {
    return "<!doctype html><html><head><title>Wlel httpd</title></head>" + "<body><h1>Wlel httpd</h1><p>Flagship app 2: raw sockets, worker pool," + " arenas.</p></body></html>";
}

fn close_fd(fd: i32) {
    close(fd);
}

/// serve one connection: read the request, dispatch, respond, close
fn handle_conn(fd: i32, root: string) {
    defer close_fd(fd);
    buf := new(u8, 65536);
    total := 0;
    head_end := -1;
    // read until the header block ends (or the buffer is full)
    while total < 65500 && head_end < 0 {
        n := recv(fd, (buf as *u8 as usize + total as usize) as *void, (65500 - total) as usize, 0);
        if n <= 0 {
            if total == 0 {
                return; // client hung up before sending anything
            }
            break;
        }
        total += n;
        buf[total] = 0;
        head_end = str_find(buf as string, "\r\n\r\n");
    }
    if head_end < 0 {
        respond(fd, 400, "Bad Request", "text/plain", "missing header block\n");
        return;
    }
    req := buf as string;
    line_end := str_find(req, "\r\n");
    if line_end < 0 {
        respond(fd, 400, "Bad Request", "text/plain", "no request line\n");
        return;
    }
    // request line: METHOD SP PATH SP VERSION
    sp1 := str_find(req, " ");
    if sp1 < 0 {
        respond(fd, 400, "Bad Request", "text/plain", "bad request line\n");
        return;
    }
    method := str_sub(req, 0, sp1);
    sp2 := str_find_from(req, " ", sp1 + 1);
    if sp2 < 0 {
        respond(fd, 400, "Bad Request", "text/plain", "bad request line\n");
        return;
    }
    path := str_sub(req, sp1 + 1, sp2 - sp1 - 1);
    // request body (echo): whatever arrived after the header block, topped
    // up to Content-Length when the header announces more
    body_start := head_end + 4;
    have := total - body_start;
    want := 0;
    cl := str_find(req, "Content-Length:");
    if cl >= 0 && cl < head_end {
        eol := str_find_from(req, "\r\n", cl);
        if eol > cl {
            vstart := cl + 15;
            while vstart < eol && req[vstart] == 32 {
                // skip OWS after ':'
                vstart += 1;
            }
            v := 0;
            if str_parse_int(str_sub(req, vstart, eol - vstart), &v) && v >= 0 {
                want = v;
            }
        }
    }
    if want > 1000000 {
        respond(fd, 413, "Payload Too Large", "text/plain", "body cap is 1 MB\n");
        return;
    }
    while have < want && total < 65500 {
        n := recv(fd, (buf as *u8 as usize + total as usize) as *void, (65500 - total) as usize, 0);
        if n <= 0 {
            break;
        }
        total += n;
        buf[total] = 0;
        have = total - body_start;
    }
    body := str_sub(req, body_start, have);
    // dispatch
    if method == "POST" || method == "PUT" {
        if path == "/echo" {
            respond(fd, 200, "OK", "application/octet-stream", body);
            return;
        }
        respond(fd, 404, "Not Found", "text/plain", "echo lives at /echo\n");
        return;
    }
    if method != "GET" {
        respond(fd, 405, "Method Not Allowed", "text/plain", "GET/POST/PUT only\n");
        return;
    }
    // GET / — built-in index (or docroot index)
    if path == "/" {
        if std::strlen(root) > 0 {
            f := std::fs::read_all(root + "/index.html");
            match f {
                Ok(txt) => {
                    respond(fd, 200, "OK", "text/html; charset=utf-8", txt);
                    return;
                }
                Err(_) => {
                }
            }
        }
        respond(fd, 200, "OK", "text/html; charset=utf-8", index_page());
        return;
    }
    // GET /path — static file under the docroot
    if std::strlen(root) == 0 {
        respond(fd, 404, "Not Found", "text/plain", "no docroot configured\n");
        return;
    }
    if str_find(path, "..") >= 0 {
        respond(fd, 403, "Forbidden", "text/plain", "path traversal rejected\n");
        return;
    }
    f := std::fs::read_all(root + path);
    match f {
        Ok(txt) => {
            respond(fd, 200, "OK", mime_of(path), txt);
        }
        Err(_) => {
            respond(fd, 404, "Not Found", "text/plain", "no such file\n");
        }
    }
}

/// worker: pull client fds until the channel closes and drain them
fn worker(c: *Ctx) {
    fd := 0;
    while sys::chan_recv(c.jobs, &fd) {
        // one arena per request: parse + response memory frees in O(1)
        arena(1 << 20) {
            handle_conn(fd as i32, c.root);
        }
    }
}

/// acceptor: push accepted client fds to the pool until accept fails
fn acceptor(c: *Ac) {
    while true {
        cfd := accept(c.lfd, 0 as *void, 0 as *u32);
        if cfd < 0 {
            return;
        }
        sys::chan_send(c.jobs, cfd as int);
    }
}

// ---------------------------------------------------------------------------
// test client (raw sockets, same FFI — used by the e2e test below)
// ---------------------------------------------------------------------------

/// send one raw request to 127.0.0.1:port, return the raw response
fn http_request(port: int, raw: string) -> string {
    fd := socket(2, 1, 0); // AF_INET, SOCK_STREAM
    if fd < 0 {
        return "";
    }
    sa := SockaddrIn { family: 2u16, port: htons(port as u16), addr: htonl(2130706433), zero: [0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8, 0u8] }; // AF_INET // 127.0.0.1
    if connect(fd, &sa as *void, 16u32) != 0 {
        close(fd);
        return "";
    }
    send_all(fd, raw);
    buf := new(u8, 65536);
    total := 0;
    while total < 65000 {
        n := recv(fd, (buf as *u8 as usize + total as usize) as *void, (65000 - total) as usize, 0);
        if n <= 0 {
            break;
        }
        total += n;
        buf[total] = 0;
    }
    close(fd);
    return buf as string;
}

// ---------------------------------------------------------------------------
// main — bind, spawn the pool, accept forever
// ---------------------------------------------------------------------------
fn main() -> int {
    port := 8080;
    root := "";
    if sys::argc() >= 2 {
        p := 0;
        if str_parse_int(sys::arg(1), &p) && p > 0 && p < 65536 {
            port = p;
        }
    }
    if sys::argc() >= 3 {
        root = sys::arg(2);
    }
    lfd := http_bind(port);
    if lfd < 0 {
        std::println_str(std::format("httpd: cannot bind port {}", port));
        return 1;
    }
    port = http_port(lfd);
    jobs := sys::chan_new[int]();
    ctx := Ctx { jobs: jobs, root: root };
    ac := Ac { lfd: lfd, jobs: jobs };
    workers := 4;
    _ts := new(*Thread, workers);
    for i in 0..workers {
        _ts[i] = sys::thread(worker, &ctx);
    }
    _acc := sys::thread(acceptor, &ac);
    std::println_str(std::format("httpd listening on http://127.0.0.1:{} (root: \"{}\")", port, root));
    while true {
        sys::sleep_ms(1000);
    }
    return 0;
}

// ---------------------------------------------------------------------------
// tests — end-to-end over localhost sockets, worker pool included
// ---------------------------------------------------------------------------
test "bind reports the bound port" {
    fd := http_bind_test();
    assert(fd >= 0);
    port := http_port(fd);
    assert(port >= 18400 && port < 65536);
    assert(close(fd) == 0);
}

test "http end-to-end: echo + static + errors" {
    lfd := http_bind_test();
    assert(lfd >= 0);
    port := http_port(lfd);
    jobs := sys::chan_new[int]();
    ctx := Ctx { jobs: jobs, root: "" };
    ac := Ac { lfd: lfd, jobs: jobs };
    w1 := sys::thread(worker, &ctx);
    w2 := sys::thread(worker, &ctx);
    acc := sys::thread(acceptor, &ac);
    // echo round-trip
    resp := http_request(port, "POST /echo HTTP/1.1\r\nHost: t\r\nContent-Length: 5\r\n\r\nhello");
    assert(str_find(resp, "200 OK") >= 0);
    assert(str_find(resp, "application/octet-stream") >= 0);
    assert(str_find(resp, "hello") >= 0);
    // built-in index
    resp2 := http_request(port, "GET / HTTP/1.1\r\nHost: t\r\n\r\n");
    assert(str_find(resp2, "200 OK") >= 0);
    assert(str_find(resp2, "Wlel httpd") >= 0);
    // unknown route under no-docroot
    resp3 := http_request(port, "GET /missing.html HTTP/1.1\r\nHost: t\r\n\r\n");
    assert(str_find(resp3, "404") >= 0);
    // wrong method on the static side
    resp4 := http_request(port, "DELETE / HTTP/1.1\r\nHost: t\r\n\r\n");
    assert(str_find(resp4, "405") >= 0);
    // malformed request line
    resp5 := http_request(port, "NONSENSE\r\n\r\n");
    assert(str_find(resp5, "400") >= 0);
    sys::chan_close(jobs);
    shutdown(lfd, 2); // wake the acceptor so its handle can be joined
    sys::join(acc);
    sys::join(w1);
    sys::join(w2);
    sys::chan_free(jobs);
    assert(close(lfd) == 0);
}

test "echo body cap is enforced" {
    lfd := http_bind_test();
    assert(lfd >= 0);
    port := http_port(lfd);
    jobs := sys::chan_new[int]();
    ctx := Ctx { jobs: jobs, root: "" };
    ac := Ac { lfd: lfd, jobs: jobs };
    w := sys::thread(worker, &ctx);
    acc := sys::thread(acceptor, &ac);
    // declare a huge body: the server must refuse without reading it
    resp := http_request(port, "POST /echo HTTP/1.1\r\nContent-Length: 9000000\r\n\r\nx");
    assert(str_find(resp, "413") >= 0);
    sys::chan_close(jobs);
    shutdown(lfd, 2);
    sys::join(acc);
    sys::join(w);
    sys::chan_free(jobs);
    assert(close(lfd) == 0);
}
