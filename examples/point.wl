// point.wl — structs, pointers, mutation through auto-deref
struct Pt {
    x: float,
    y: float
}

fn bump(p: *Pt) -> void {
    p.x = p.x + 1.0;
    p.y = p.y + 2.0;
}

fn main() -> int {
    let o: Pt = Pt { x: 0.0, y: 0.0 };
    let q: *Pt = &o;
    bump(q);
    wlel_print_float(o.x);
    wlel_print_float(o.y);
    return 0;
}
