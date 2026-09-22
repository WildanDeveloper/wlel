// Concurrency v1: threads, mutexes, channels — the honest C-like model.
//
//   wlel run examples/concurrency.wl   (worker pool demo)
//
// sys::thread(work, data) spawns an OS thread running work(data): the
// worker is resolved by name (Wlel has no function pointers), takes exactly
// one parameter, returns void, and gets a fresh arena of its own. Shared
// state is plain memory guarded by sys::mutex_*; threads coordinate through
// sys::chan_new[T] — an unbounded FIFO whose recv blocks until a value
// arrives (false = closed and drained). Data races are the user's
// responsibility, exactly like C: the language documents the discipline and
// the sanitizer mode (`wlel build -sanitize`) catches the fallout — a pool
// like the one below runs LeakSanitizer-clean: join frees each thread
// handle, and each thread frees its own arena on exit.
use std;

// shared pool state: jobs flow in, squared results flow out, the running
// total is plain shared memory guarded by the mutex
struct Pool {
    jobs: *Chan[int],
    done: *Chan[int],
    mu: *Mutex,
    total: *int
}

// one worker: pull jobs until the channel closes and drains
fn worker(p: *Pool) {
    job := 0;
    while sys::chan_recv(p.jobs, &job) {
        s := job * job;
        sys::mutex_lock(p.mu);
        *p.total = *p.total + s;
        sys::mutex_unlock(p.mu);
        sys::chan_send(p.done, s);
    }
}

fn main() {
    n := 8; // jobs
    workers := 4;
    jobs := sys::chan_new[int]();
    done := sys::chan_new[int]();
    mu := sys::mutex_new();
    total := new(int);
    *total = 0;
    pool := Pool { jobs: jobs, done: done, mu: mu, total: total };
    ts := new(*Thread, workers);
    for i in 0..workers {
        ts[i] = sys::thread(worker, &pool);
    }
    for j in 1..n + 1 {
        sys::chan_send(jobs, j);
    }
    sys::chan_close(jobs); // workers drain the queue, then see recv == false
    got := 0;
    for _k in 0..n {
        r := 0;
        if !sys::chan_recv(done, &r) {
            std::println_str("pool: results channel closed early");
            sys::exit(1);
        }
        got += r;
    }
    for t in 0..workers {
        sys::join(ts[t]);
    }
    want := n * (n + 1) * (2 * n + 1) / 6; // sum of squares, closed form
    std::print_str("pool: ");
    std::print_int(workers);
    std::print_str(" workers, sum of squares 1..");
    std::print_int(n);
    std::print_str(" = ");
    std::println_int(*total);
    if *total != want || got != want {
        std::println_str("pool: MISMATCH");
        sys::exit(1);
    }
    sys::mutex_free(mu);
    sys::chan_free(jobs);
    sys::chan_free(done);
    std::println_str("pool ok");
}

test "thread runs to completion and join releases it" {
    counter := new(int);
    *counter = 0;
    mu := sys::mutex_new();
    bump := Bump { mu: mu, target: counter };
    t := sys::thread(bump_it, &bump);
    sys::join(t);
    assert_eq(*counter, 1000);
    sys::mutex_free(mu);
}

struct Bump {
    mu: *Mutex,
    target: *int
}

// one parameter, void return — the sys::thread contract
fn bump_it(b: *Bump) {
    for _i in 0..1000 {
        sys::mutex_lock(b.mu);
        *b.target = *b.target + 1;
        sys::mutex_unlock(b.mu);
    }
}

test "channel: FIFO order, close semantics, send-after-close" {
    ch := sys::chan_new[int]();
    assert(sys::chan_send(ch, 1));
    assert(sys::chan_send(ch, 2));
    assert(sys::chan_send(ch, 3));
    a := 0;
    assert(sys::chan_recv(ch, &a));
    assert_eq(a, 1);
    sys::chan_close(ch);
    // queued values stay readable after close...
    assert(sys::chan_recv(ch, &a));
    assert_eq(a, 2);
    assert(sys::chan_recv(ch, &a));
    assert_eq(a, 3);
    // ...then recv reports drained (false) and *out is untouched
    assert(!sys::chan_recv(ch, &a));
    assert_eq(a, 3);
    // send after close drops the value
    assert(!sys::chan_send(ch, 99));
    sys::chan_free(ch);
}

test "channel carries strings and structs" {
    s := sys::chan_new[string]();
    assert(sys::chan_send(s, "hello"));
    assert(sys::chan_send(s, "arena"));
    x := "";
    assert(sys::chan_recv(s, &x));
    assert_eq(x, "hello");
    assert(sys::chan_recv(s, &x));
    assert_eq(x, "arena");
    sys::chan_free(s);
    p := sys::chan_new[Task]();
    assert(sys::chan_send(p, Task { id: 7, tag: "write" }));
    t := Task { id: 0, tag: "" };
    assert(sys::chan_recv(p, &t));
    assert_eq(t.id, 7);
    assert_eq(t.tag, "write");
    sys::chan_free(p);
}

struct Task {
    id: int,
    tag: string
}

test "recv blocks until another thread sends" {
    ch := sys::chan_new[int]();
    later := Later { ch: ch };
    t := sys::thread(send_later, &later);
    v := 0;
    assert(sys::chan_recv(ch, &v)); // blocks; sender sleeps first
    assert_eq(v, 42);
    sys::join(t);
    sys::chan_free(ch);
}

struct Later {
    ch: *Chan[int]
}

fn send_later(l: *Later) {
    sys::sleep_ms(10);
    sys::chan_send(l.ch, 42);
}

test "worker pool totals every job exactly once" {
    n := 64;
    jobs := sys::chan_new[int]();
    done := sys::chan_new[int]();
    mu := sys::mutex_new();
    total := new(int);
    *total = 0;
    pool := Pool { jobs: jobs, done: done, mu: mu, total: total };
    w1 := sys::thread(worker, &pool);
    w2 := sys::thread(worker, &pool);
    w3 := sys::thread(worker, &pool);
    for j in 1..n + 1 {
        sys::chan_send(jobs, j);
    }
    sys::chan_close(jobs);
    got := 0;
    for _k in 0..n {
        r := 0;
        assert(sys::chan_recv(done, &r));
        got += r;
    }
    sys::join(w1);
    sys::join(w2);
    sys::join(w3);
    assert_eq(got, n * (n + 1) * (2 * n + 1) / 6);
    assert_eq(*total, got); // mutex discipline held under contention
    sys::mutex_free(mu);
    sys::chan_free(jobs);
    sys::chan_free(done);
}
