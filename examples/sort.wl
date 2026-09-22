// sort.wl — std::sort & std::binary_search (Fase 2)
//
//   wlel run  examples/sort.wl           — demo + 1M int benchmark
//   wlel test examples/sort.wl           — run the test blocks
//   wlel build examples/sort.wl -o sort -O2  — release benchmark binary
//
// std::sort takes a comparator `cmp(a, b) -> int` (negative / zero /
// positive, like strcmp) over three collection forms:
//
//   std::sort(arr, cmp)      — fixed array [T; N]
//   std::sort(ptr, n, cmp)   — pointer + element count
//   std::sort(vec, cmp)      — Vec[T] (its live elements)
//
// std::binary_search has the same forms with the needle before cmp and
// returns a matching index or -1. The comparator is an ordinary Wlel
// function — no function pointers, no qsort void*: the compiler emits a
// specialized C sort per (element, comparator) pair so the comparator
// call inlines at -O2.
use std;

struct Item {
    key: int,
    tag: string
}

fn asc(a: int, b: int) -> int {
    if a < b {
        return -1;
    }
    if a > b {
        return 1;
    }
    return 0;
}

fn desc(a: int, b: int) -> int {
    return asc(b, a);
}

fn asc_f(a: float, b: float) -> int {
    if a < b {
        return -1;
    }
    if a > b {
        return 1;
    }
    return 0;
}

fn by_key(a: Item, b: Item) -> int {
    return asc(a.key, b.key);
}

fn main() -> int {
    std::println_str("== Wlel Sort Demo ==");

    // 1. fixed arrays: two comparators on one element type produce two
    //    specialized sorts
    a := [5, 3, 9, 1, 7, 3];
    std::sort(a, asc);
    std::print_str("asc: ");
    for x in a {
        std::print_str(std::format("{} ", x));
    }
    std::println_str("");
    std::sort(a, desc);
    std::print_str("desc: ");
    for x in a {
        std::print_str(std::format("{} ", x));
    }
    std::println_str("");

    // 2. floats
    f := [2.5, -1.0, 3.25, 0.5];
    std::sort(f, asc_f);
    std::print_str("floats: ");
    for x in f {
        std::print_str(std::format("{} ", x));
    }
    std::println_str("");

    // 3. strings: str_cmp is the ready-made comparator from the stdlib
    words := vec_new[string]();
    words.push("pear");
    words.push("apple");
    words.push("fig");
    words.push("banana");
    std::sort(words, str_cmp);
    std::print_str("strings: ");
    for s in words {
        std::print_str(std::format("{} ", s));
    }
    std::println_str("");

    // 4. structs by a field
    items := vec_new[Item]();
    items.push(Item { key: 3, tag: "three" });
    items.push(Item { key: 1, tag: "one" });
    items.push(Item { key: 2, tag: "two" });
    std::sort(items, by_key);
    std::print_str("structs: ");
    for it in items {
        std::print_str(std::format("{}={} ", it.key, it.tag));
    }
    std::println_str("");

    // 5. binary search: an index of the needle, or -1
    i := std::binary_search(words, "fig", str_cmp);
    miss := std::binary_search(words, "kiwi", str_cmp);
    std::println_str(std::format("binary_search: 'fig' at {}, 'kiwi' -> {}", i, miss));

    // 6. benchmark: 1,000,000 random ints sorted in place (LCG input,
    //    reproducible across runs)
    n := 1000000;
    data := new(int, n);
    seed := 2685821657736338717u64;
    for i in 0..n {
        seed = seed * 6364136223846793005 + 1442695040888963407;
        data[i] = (seed >> 33) as int;
    }
    t0 := sys::mono_ms();
    std::sort(data, n, asc);
    ms := sys::mono_ms() - t0;
    ordered := true;
    for i in 1..n {
        if data[i - 1] > data[i] {
            ordered = false;
        }
    }
    std::println_str(std::format("sorted {} ints in {} ms (ordered: {})", n, ms, ordered));
    if !ordered {
        return 1;
    }
    return 0;
}

test "sort ints asc and desc" {
    a := [5, 3, 9, 1, 7, 3, 3];
    std::sort(a, asc);
    assert_eq(a[0], 1);
    assert_eq(a[2], 3);
    assert_eq(a[6], 9);
    std::sort(a, desc);
    assert_eq(a[0], 9);
    assert_eq(a[6], 1);
}

test "sort floats and strings" {
    f := [2.5, -1.0, 0.0, 3.25];
    std::sort(f, asc_f);
    assert(f[0] < f[1] && f[1] < f[2] && f[2] < f[3]);
    w := vec_new[string]();
    w.push("pear");
    w.push("apple");
    w.push("fig");
    std::sort(w, str_cmp);
    assert_eq(w.data[0], "apple");
    assert_eq(w.data[1], "fig");
    assert_eq(w.data[2], "pear");
}

test "binary search hit and miss" {
    a := [10, 20, 30, 40, 50];
    assert_eq(std::binary_search(a, 30, asc), 2);
    assert_eq(std::binary_search(a, 10, asc), 0);
    assert_eq(std::binary_search(a, 50, asc), 4);
    assert_eq(std::binary_search(a, 35, asc), -1);
    assert_eq(std::binary_search(a, 5, asc), -1);
    assert_eq(std::binary_search(a, 60, asc), -1);
}

test "sort pointer with explicit length" {
    p := new(int, 6);
    p[0] = 4;
    p[1] = -2;
    p[2] = 9;
    p[3] = 0;
    p[4] = -7;
    p[5] = 4;
    std::sort(p, 6, asc);
    assert_eq(p[0], -7);
    assert_eq(p[1], -2);
    assert_eq(p[2], 0);
    assert_eq(p[3], 4);
    assert_eq(p[4], 4);
    assert_eq(p[5], 9);
    assert_eq(std::binary_search(p, 6, -7, asc), 0);
    assert_eq(std::binary_search(p, 6, 5, asc), -1);
}

test "duplicates and degenerate inputs" {
    eq := [7, 7, 7, 7, 7];
    std::sort(eq, asc);
    assert_eq(eq[0], 7);
    assert_eq(eq[4], 7);
    sorted := [1, 2, 3, 4, 5];
    std::sort(sorted, asc);
    assert_eq(sorted[4], 5);
    rev := [5, 4, 3, 2, 1];
    std::sort(rev, asc);
    assert_eq(rev[0], 1);
    assert_eq(rev[4], 5);
}

test "sort structs by a field" {
    items := vec_new[Item]();
    items.push(Item { key: 3, tag: "c" });
    items.push(Item { key: 1, tag: "a" });
    items.push(Item { key: 2, tag: "b" });
    std::sort(items, by_key);
    assert_eq(items.data[0].key, 1);
    assert_eq(items.data[1].key, 2);
    assert_eq(items.data[2].tag, "c");
    assert_eq(std::binary_search(items, Item { key: 2, tag: "" }, by_key), 1);
}
