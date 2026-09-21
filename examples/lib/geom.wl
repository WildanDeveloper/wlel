// geom.wl — reusable geometry helpers (imported by kitchen.wl)
struct Vec2 {
    x: float,
    y: float
}

fn add(a: Vec2, b: Vec2) -> Vec2 {
    return Vec2 { x: a.x + b.x, y: a.y + b.y };
}

fn dot(a: Vec2, b: Vec2) -> float {
    return a.x * b.x + a.y * b.y;
}

test "add sums the components" {
    r := add(Vec2 { x: 1.0, y: 2.0 }, Vec2 { x: 3.0, y: 4.0 });
    assert_eq(r.x, 4.0);
    assert_eq(r.y, 6.0);
}

test "dot product" {
    assert_eq(dot(Vec2 { x: 2.0, y: 3.0 }, Vec2 { x: 4.0, y: 1.0 }), 11.0);
}
