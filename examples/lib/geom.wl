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
