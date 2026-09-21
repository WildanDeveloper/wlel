// enums.wl — tagged enums + match (Fase 3)
// A tagged enum is a closed set of variants, each optionally carrying
// typed payloads. The C layout is a tag plus a union of payload structs;
// `match` is the only way to destructure one, and the checker rejects a
// match that does not cover every variant.
use std;

enum Shape {
    Circle(float),
    Rect(float, float),
    Point
}

fn area(s: Shape) -> float {
    return match s {
        Circle(r) => 3.141592653589793 * r * r,
        Rect(w, h) => w * h,
        Point => 0.0,
    };
}

fn perimeter(s: Shape) -> float {
    match s {
        Circle(r) => {
            return 2.0 * 3.141592653589793 * r;
        }
        Rect(w, h) => {
            return 2.0 * (w + h);
        }
        Point => {
        }
    }
    return 0.0;
}

fn name(s: Shape) -> string {
    return match s {
        Circle(_) => "circle",
        Rect(_, _) => "rect",
        Point => "point",
    };
}

fn scale(s: Shape, k: float) -> Shape {
    return match s {
        Circle(r) => Circle(r * k),
        Rect(w, h) => Rect(w * k, h * k),
        Point => Point,
    };
}

// enums nest and compose with structs and generics
enum Op {
    Add,
    Neg,
    Push(int)
}

struct Machine {
    acc: int,
    last: Op
}

fn apply(m: Machine, op: Op) -> Machine {
    match op {
        Add => {
            return Machine { acc: m.acc * 2, last: Add };
        }
        Neg => {
            return Machine { acc: -m.acc, last: Neg };
        }
        Push(v) => {
            return Machine { acc: m.acc + v, last: Push(v) };
        }
    }
}

// std ships Result[T, E] and Option[T] as generic enums; construction
// infers from payloads, or from the annotated let/return type
fn parse_age(raw: string) -> Result[int, string] {
    if raw == "" {
        return Err("empty");
    }
    total := 0;
    for i in 0..std::strlen(raw) {
        c := raw[i];
        if c < 48 || c > 57 {
            return Err("not a digit");
        }
        if !std::checked_mul(total, 10, &total) {
            return Err("overflow");
        }
        if !std::checked_add(total, (c - 48) as int, &total) {
            return Err("overflow");
        }
    }
    return Ok(total);
}

fn first_digit(raw: string) -> Option[int] {
    for i in 0..std::strlen(raw) {
        c := raw[i];
        if c >= 48 && c <= 57 {
            return Some((c - 48) as int);
        }
    }
    return None;
}

fn describe(r: Result[int, string]) -> string {
    return match r {
        Ok(v) => std::format("ok({})", v),
        Err(e) => std::format("err({})", e),
    };
}

fn main() -> int {
    std::println_str("== Wlel Enums Demo ==");

    // construct and destructure
    std::println_str(name(Circle(1.0)));
    std::println_str(name(Rect(2.0, 3.0)));
    std::println_str(name(Point));

    // payload bindings in arms; the wildcard `_` discards
    std::println_float(area(Circle(2.0)));
    std::println_float(area(Rect(3.0, 4.0)));
    std::println_float(area(Point));
    std::println_float(perimeter(Rect(3.0, 4.0)));
    std::println_float(perimeter(Point));

    // values copy like structs; constructors nest in expressions
    big := scale(Circle(1.5), 2.0);
    std::println_float(area(big));

    // enums as struct fields, matched through the field
    m0 := Machine { acc: 5, last: Add };
    m1 := apply(m0, Neg);
    m2 := apply(m1, Push(42));
    std::println_int(m2.acc);

    // generic enums from std: Result + Option
    std::println_str(describe(parse_age("42")));
    std::println_str(describe(parse_age("4x")));
    std::println_str(describe(parse_age("")));
    match first_digit("ab7c") {
        Some(d) => std::println_int(d),
        None => std::println_str("no digit"),
    }
    match first_digit("xyz") {
        Some(d) => std::println_int(d),
        None => std::println_str("no digit"),
    }
    return 0;
}

test "enum construct and area" {
    assert_eq(area(Circle(2.0)), 12.566370614359172);
    assert_eq(area(Rect(3.0, 4.0)), 12.0);
    assert_eq(area(Point), 0.0);
}

test "enum name via wildcard binds" {
    assert_eq(name(Circle(9.0)), "circle");
    assert_eq(name(Rect(9.0, 9.0)), "rect");
    assert_eq(name(Point), "point");
}

test "enum scale copies" {
    s := scale(Circle(1.0), 3.0);
    assert_eq(area(s), 28.274333882308138);
}

test "enum machine ops" {
    m := Machine { acc: 5, last: Add };
    m = apply(m, Neg);
    assert_eq(m.acc, -5);
    m = apply(m, Push(42));
    assert_eq(m.acc, 37);
    match m.last {
        Push(v) => assert_eq(v, 42),
        _ => assert_eq(0, 1),
    }
}

test "result ok and err paths" {
    let ok: Result[int, string] = Ok(42);
    match ok {
        Ok(v) => assert_eq(v, 42),
        Err(e) => {
            let dead: int = 0;
            assert_eq(e, "");
            assert_eq(dead, 1);
        }
    }
    let bad: Result[int, string] = Err("nope");
    match bad {
        Ok(v) => {
            let dead: int = 0;
            assert_eq(v, 1);
            assert_eq(dead, 1);
        }
        Err(e) => assert_eq(e, "nope"),
    }
}

test "result from parse_age" {
    match parse_age("42") {
        Ok(v) => assert_eq(v, 42),
        Err(e) => assert_eq(e, "unreachable"),
    }
    match parse_age("4x") {
        Ok(_) => assert_eq(0, 1),
        Err(e) => assert_eq(e, "not a digit"),
    }
}

test "option some and none" {
    match first_digit("ab7c") {
        Some(d) => assert_eq(d, 7),
        None => assert_eq(0, 1),
    }
    match first_digit("xyz") {
        Some(_) => assert_eq(0, 1),
        None => assert_eq(0, 0),
    }
}

test "match all arms return is terminating" {
    // every arm returns, so no trailing return is needed after the match
    assert_eq(name(Rect(1.0, 1.0)), "rect");
}
