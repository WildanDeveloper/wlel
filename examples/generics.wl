// generics.wl — generic functions & structs via monomorphization (Fase 2)
// fn id[T](x: T) -> T is compiled per concrete type: no boxing, no vtables,
// C output is exactly what hand-written C would be.
use std;

struct Box[T] {
    val: T,
    tag: int
}

fn box_new[T](v: T, tag: int) -> Box[T] {
    return Box { val: v, tag: tag };
}

fn box_get[T](b: Box[T]) -> T {
    return b.val;
}

fn first[T](a: T, _b: T) -> T {
    return a;
}

fn main() -> int {
    std::println_str("== Wlel Generics Demo ==");

    // one function, many concrete types — monomorphized per type
    i := first(42, 99);
    f := first(1.5, 2.5);
    if first(true, false) {
        std::print_int(i);
        std::print_str(" ");
        std::println_float(f);
    }

    // generic structs, including nested instantiation
    bi := box_new(7, 1);
    bf := box_new(2.25, 2);
    std::println_int(box_get(bi));
    std::println_float(box_get(bf));

    // nested generics: Box[Box[int]]
    nest := Box { val: Box { val: 3, tag: 0 }, tag: 0 };
    std::println_int(nest.val.val);

    // generics compose with arenas and safe mode
    arena(512) {
        arr := new(Box[int], 2);
        arr[0] = box_new(10, 0);
        arr[1] = box_new(20, 0);
        std::println_int(arr[0].val + arr[1].val); // 30
    }
    return 0;
}

test "generics across types" {
    assert_eq(first(5, 6), 5);
    assert_eq(first(1.5, 2.5), 1.5);
    assert_eq(box_get(box_new(9, 0)), 9);
}

test "nested generics" {
    n := Box { val: Box { val: 3, tag: 0 }, tag: 0 };
    assert_eq(n.val.val, 3);
}
