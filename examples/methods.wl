// impl blocks attach methods to structs and enums: the checker lowers
// every method into a plain function (`Pt__len`) and `recv.method(args)`
// resolves into that call. A bare `self` is the impl type by value; a
// pointer receiver (`self: *T`) mutates in place. Value receivers on
// pointers auto-deref, pointer receivers on values auto-borrow.
use std;

struct Pt {
    x: float,
    y: float
}

impl Pt {

    fn len(self) -> float {
        // value receiver: reads a copy
        return std::math::sqrt(self.x * self.x + self.y * self.y);
    }

    fn add(self, o: Pt) -> Pt {
        return Pt { x: self.x + o.x, y: self.y + o.y };
    }

    fn dot(self, o: Pt) -> float {
        return self.x * o.x + self.y * o.y;
    }

    fn scale(self: *Pt, k: float) {
        // pointer receiver: mutates in place
        self.x *= k;
        self.y *= k;
    }
}

// generic impls: [T] repeats the struct's own parameters and the methods
// monomorphize per receiver, like generic functions
struct Box[T] {
    val: T
}

impl Box[T] {

    fn get(self) -> T {
        return self.val;
    }

    fn set(self: *Box[T], v: T) {
        self.val = v;
    }

    fn swapped(self, other: Box[T]) -> [Box[T]; 2] {
        return [Box { val: self.val }, Box { val: other.val }];
    }
}

// enums take methods too: the body destructures with match
enum Shape {
    Circle(float),
    Rect(float, float),
    Point
}

impl Shape {

    fn area(self) -> float {
        return match self {
            Circle(r) => 3.141592653589793 * r * r,
            Rect(w, h) => w * h,
            Point => 0.0,
        };
    }

    fn name(self) -> string {
        return match self {
            Circle(_) => "circle",
            Rect(_, _) => "rect",
            Point => "point",
        };
    }
}

// the std collections carry a method layer over the free functions
fn main() -> int {
    p := Pt { x: 3.0, y: 4.0 };
    std::println_float(p.len()); // 5
    q := p.add(Pt { x: 1.0, y: 1.0 });
    std::println_float(q.len()); // 6.40312
    p.scale(10.0);
    std::println_float(p.len()); // 50
    pp := &p;
    std::println_float(pp.len()); // auto-deref, still 50
    b := Box { val: 41 };
    b.set(b.get() + 1);
    std::println_int(b.get()); // 42
    bs := Box { val: "wlel" };
    std::println_str(bs.get());
    c := Circle(1.0);
    std::println_float(c.area()); // pi
    std::println_str(c.name());
    std::println_float(Point.area());
    v := vec_new[int]();
    v.push(2);
    v.push(3);
    v.push(5);
    std::println_int(v.len()); // 3
    std::println_int(v.pop()); // 5
    std::println_int(v.get(0)); // 2
    m := map_new[string, int]();
    m.set("a", 1);
    m.set("b", 2);
    std::println_int(m.get_or("a", 0)); // 1
    std::println_int(m.get_or("z", 0)); // 0
    if m.has("b") {
        std::println_str("has b");
    } else {        std::println_str("no b");
    }
    m.del("b");
    if m.has("b") {
        std::println_str("has b");
    } else {        std::println_str("no b");
    }
    let okv: Result[int, string] = Ok(10);
    if okv.is_ok() {
        std::println_str("ok is ok");
    }
    let e: Result[int, string] = Err("nope");
    if e.is_err() {
        std::println_str("err is err");
    }
    return 0;
}

test "method value receiver" {
    p := Pt { x: 3.0, y: 4.0 };
    assert_eq(p.len(), 5.0);
    assert_eq(p.dot(Pt { x: 1.0, y: 2.0 }), 11.0);
    q := p.add(Pt { x: -3.0, y: -4.0 });
    assert_eq(q.len(), 0.0);
}

test "method pointer receiver mutates" {
    p := Pt { x: 1.0, y: 1.0 };
    p.scale(4.0);
    assert_eq(p.x, 4.0);
    pp := &p;
    pp.scale(2.0);
    assert_eq(p.x, 8.0);
}

test "generic methods monomorphize" {
    b := Box { val: 42 };
    assert_eq(b.get(), 42);
    b.set(7);
    assert_eq(b.get(), 7);
    bs := Box { val: "hi" };
    assert_eq(bs.get(), "hi");
}

test "enum methods" {
    c := Circle(2.0);
    assert_eq(c.area(), 12.566370614359172);
    assert_eq(Rect(3.0, 4.0).area(), 12.0);
    assert_eq(Point.area(), 0.0);
    assert_eq(c.name(), "circle");
}

test "std collection methods" {
    v := vec_new[string]();
    v.push("a");
    v.push("b");
    assert_eq(v.len(), 2);
    assert_eq(v.pop(), "b");
    v.clear();
    assert_eq(v.len(), 0);
    m := map_new[int, string]();
    m.set(1, "one");
    assert_eq(m.get(1), "one");
    assert_eq(m.get_or(2, "?"), "?");
    assert(m.has(1));
    assert(!m.has(2));
    let r: Result[int, string] = Ok(17);
    assert(r.is_ok());
    assert_eq(r.unwrap(), 17);
    let bad: Result[int, string] = Err("no digits");
    assert(bad.is_err());
    assert_eq(bad.unwrap_or(-1), -1);
}
