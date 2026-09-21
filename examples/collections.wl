// collections.wl — std collections demo (Fase 2)
// Vec[T] and HashMap[K, V] are written in Wlel itself, embedded in the
// compiler, and monomorphized per concrete type: no boxing, arena-aware.
use std;

struct Word {
    text: string,
    count: int
}

fn main() -> int {
    std::println_str("== Wlel Collections Demo ==");

    // Vec: growable, arena-backed, monomorphized per element type
    squares := vec_new[int]();
    for i in 0..5 {
        vec_push(&squares, i * i);
    }
    for x in squares {
        std::print_int(x);
        std::print_str(" ");
    }
    std::println_str("");

    // HashMap: open addressing, string or int keys
    pop := map_new[string, int]();
    map_set(&pop, "jakarta", 10);
    map_set(&pop, "bandung", 2);
    map_set(&pop, "jakarta", 11); // overwrite
    std::println_int(map_get(&pop, "jakarta"));
    std::println_int(map_get_or(&pop, "medan", 0));

    // iterate keys (order unspecified), count total
    total := 0;
    for k in map_keys(&pop) {
        total += map_get(&pop, k);
    }
    std::println_int(total);

    // mixed types compose: Vec of structs, element copied per iteration
    words := vec_new[Word]();
    vec_push(&words, Word { text: "arena", count: 1 });
    vec_push(&words, Word { text: "first", count: 2 });
    best := vec_get(&words, 0);
    for w in words {
        if w.count > best.count {
            best = w;
        }
    }
    std::println_str(best.text);

    // deletion + membership
    map_del(&pop, "bandung");
    had := 0;
    if map_has(&pop, "bandung") {
        had = 1;
    }
    std::println_int(had);
    return 0;
}

test "vec grow and iterate" {
    v := vec_new[int]();
    for i in 0..50 {
        vec_push(&v, i);
    }
    s := 0;
    for x in v {
        s += x;
    }
    assert_eq(s, 1225);
    assert_eq(vec_len(&v), 50);
}

test "map set get delete" {
    m := map_new[string, int]();
    map_set(&m, "a", 1);
    map_set(&m, "b", 2);
    map_set(&m, "a", 3);
    assert_eq(map_get(&m, "a"), 3);
    assert_eq(m.len, 2);
    assert(map_del(&m, "b"));
    assert(!map_has(&m, "b"));
    assert_eq(map_get_or(&m, "b", -1), -1);
}

test "map rehash across many keys" {
    m := map_new[int, int]();
    for i in 0..200 {
        map_set(&m, i, i * 3);
    }
    ok := 0;
    for i in 0..200 {
        if map_get(&m, i) == i * 3 {
            ok += 1;
        }
    }
    assert_eq(ok, 200);
}
