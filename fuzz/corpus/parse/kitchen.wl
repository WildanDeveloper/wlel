// kitchen.wl — casts, heap, arena, arrays, imports, std
use std;
use "lib/geom.wl";

fn sum(arr: *int, n: int) -> int {
    total := 0;
    i := 0;
    while i < n {
        total = total + arr[i];
        i = i + 1;
    }
    return total;
}

fn main() -> int {

    // std module
    std::println_str("== kitchen sink ==");
    std::println_int(std::max(40, std::abs(0 - 42)));

    // arrays
    let nums: [int; 5] = [3, 1, 4, 1, 5];
    std::println_int(std::len(nums));
    std::println_int(sum(nums, std::len(nums)));

    // heap alloc + cast from *void
    let heap: *int = wlel_alloc(8) as *int;
    heap[0] = 123;
    std::println_int(heap[0]);
    wlel_free(heap);

    // arena: many allocations, one free
    let a: *void = wlel_arena_new(1024);
    let v: *Vec2 = wlel_arena_alloc(a, wlel_sizeof(Vec2)) as *Vec2;
    v.x = 1.5;
    v.y = 2.5;
    let w: Vec2 = Vec2 { x: 1.0, y: 1.0 };
    let r: Vec2 = add(*v, w);
    std::println_float(r.x);
    std::println_float(r.y);
    std::println_float(dot(*v, *v));
    wlel_arena_free(a);

    // casts between numerics
    let f: float = 9.75;
    let truncated: int = f as int;
    std::println_int(truncated);
    defer std::println_str("done");
    return 0;
}

test "sum adds up the array" {
    let nums: [int; 5] = [3, 1, 4, 1, 5];
    assert_eq(sum(nums, std::len(nums)), 14);
}
