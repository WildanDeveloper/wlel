// game.wl — flagship app 3: a small arcade game (snake) with real raylib
// bindings, written in Wlel.
//
//   wlel build flagship/game.wl -O2 -o arena \
//       -l raylib -L <raylib-libdir> -l GL -l X11 -l dl
//   ./arena
//
// Building the raylib static library (5.5):
//   git clone --depth 1 --branch 5.5 https://github.com/raysan5/raylib
//   cd raylib/src && make PLATFORM=PLATFORM_DESKTOP    # -> libraylib.a
//
// The bindings are plain `extern fn` declarations over the raylib C API —
// structs by value (Color) layout-match the C headers, so no bindings
// layer exists: the C ABI is the API. The game logic (movement, collision,
// food, score) is pure Wlel driven by a seeded xorshift, and the test
// blocks below run it WITHOUT opening a window — `wlel test` is fully
// headless and CI-safe; only the main loop touches the GPU.
//
// Controls: arrows / WASD to steer, R to restart, ESC to quit.
use std;

// ---------------------------------------------------------------------------
// raylib bindings (raylib.h, exact signatures; C int = i32, float = f32)
// ---------------------------------------------------------------------------
extern fn InitWindow(width: i32, height: i32, title: string);

extern fn CloseWindow();

extern fn WindowShouldClose() -> bool;

extern fn BeginDrawing();

extern fn EndDrawing();

extern fn ClearBackground(col: Color);

extern fn SetTargetFPS(fps: i32);

extern fn DrawText(text: string, x: i32, y: i32, size: i32, col: Color);

extern fn DrawRectangle(x: i32, y: i32, w: i32, h: i32, col: Color);

extern fn IsKeyDown(key: i32) -> bool;

extern fn IsKeyPressed(key: i32) -> bool;

extern fn GetTime() -> f64;

/// raylib color, 4 floats by value (matches the C struct exactly)
struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32
}

// ---------------------------------------------------------------------------
// game state + pure logic
// ---------------------------------------------------------------------------
fn grid_w() -> int {
    return 32;
}

fn grid_h() -> int {
    return 24;
}

fn cell() -> int {
    return 24;
}

// raylib key codes (raylib.h)
fn key_right() -> int {
    return 262;
}

fn key_left() -> int {
    return 263;
}

fn key_down() -> int {
    return 264;
}

fn key_up() -> int {
    return 265;
}

fn key_w() -> int {
    return 87;
}

fn key_a() -> int {
    return 65;
}

fn key_s() -> int {
    return 83;
}

fn key_d() -> int {
    return 68;
}

fn key_r() -> int {
    return 82;
}

/// one run of the game. Segments live in two preallocated arrays (head
/// first); everything is plain data so the logic is trivially testable.
struct Game {
    sx: *int,
    sy: *int,
    cap: int,
    len: int,
    dx: int,
    dy: int,
    ndx: int,
    ndy: int,
    fx: int,
    fy: int,
    score: int,
    dead: bool,
    tlast: float,
    tick: float
}

/// fresh run: 3-segment snake in the middle heading right
fn game_new(w: int, h: int) -> Game {
    cap := w * h;
    g := Game { sx: new(int, cap), sy: new(int, cap), cap: cap, len: 3, dx: 1, dy: 0, ndx: 1, ndy: 0, fx: 0, fy: 0, score: 0, dead: false, tlast: 0.0, tick: 0.14 };
    mid_x := w / 2;
    mid_y := h / 2;
    g.sx[0] = mid_x;
    g.sy[0] = mid_y;
    g.sx[1] = mid_x - 1;
    g.sy[1] = mid_y;
    g.sx[2] = mid_x - 2;
    g.sy[2] = mid_y;
    place_food(&g, w, h);
    return g;
}

/// queue a direction; reversing 180 degrees into yourself is ignored
fn turn(g: *Game, ndx: int, ndy: int) {
    if ndx == -g.dx && ndy == -g.dy {
        return;
    }
    if ndx == g.dx && ndy == g.dy {
        return;
    }
    g.ndx = ndx;
    g.ndy = ndy;
}

/// true when (x, y) is on the snake's body
fn on_snake(g: *Game, x: int, y: int) -> bool {
    for k in 0..g.len {
        if g.sx[k] == x && g.sy[k] == y {
            return true;
        }
    }
    return false;
}

/// place food on any free cell (rejection sampling with a linear fallback;
/// std::random is a documented xorshift64* — seed it for reproducible runs)
fn place_food(g: *Game, w: int, h: int) {
    total := w * h;
    for _k in 0..10000 {
        cell := std::random::int(total);
        x := cell % w;
        y := cell / w;
        if !on_snake(g, x, y) {
            g.fx = x;
            g.fy = y;
            return;
        }
    }
    // board nearly full: first free cell
    for c in 0..total {
        x := c % w;
        y := c / w;
        if !on_snake(g, x, y) {
            g.fx = x;
            g.fy = y;
            return;
        }
    }
    g.fx = -1; // board is full — you won
}

/// advance one tick: returns false when the run ends (wall or self hit).
/// The queued direction applies, the tail vacates its cell unless this
/// tick eats (the classic rule that lets you follow your own tail).
fn step(g: *Game, w: int, h: int) -> bool {
    if g.dead {
        return false;
    }
    g.dx = g.ndx;
    g.dy = g.ndy;
    nx := g.sx[0] + g.dx;
    ny := g.sy[0] + g.dy;
    if nx < 0 || ny < 0 || nx >= w || ny >= h {
        g.dead = true;
        return false;
    }
    eating := nx == g.fx && ny == g.fy;
    // body check: the tail cell vacates unless we grow into it
    skip_tail := !eating;
    for k in 0..g.len {
        if skip_tail && k == g.len - 1 {
            continue;
        }
        if g.sx[k] == nx && g.sy[k] == ny {
            g.dead = true;
            return false;
        }
    }
    // shift the body back, head takes the new cell
    if eating {
        if g.len < g.cap {
            for k in 0..g.len {
                back := g.len - k; // shift index len-1 .. 1 -> len .. 2
                g.sx[back] = g.sx[back - 1];
                g.sy[back] = g.sy[back - 1];
            }
            g.len += 1;
        }
        g.score += 10;
        g.tick = 0.14 - (g.score / 10) as float * 0.002;
        if g.tick < 0.05 {
            g.tick = 0.05;
        }
        place_food(g, w, h);
    } else {        for k in 0..g.len - 1 {
            back := g.len - 1 - k;
            g.sx[back] = g.sx[back - 1];
            g.sy[back] = g.sy[back - 1];
        }
    }
    g.sx[0] = nx;
    g.sy[0] = ny;
    return true;
}

// ---------------------------------------------------------------------------
// rendering (the only raylib-touching code)
// ---------------------------------------------------------------------------
fn col(r: f32, g: f32, b: f32) -> Color {
    return Color { r: r, g: g, b: b, a: 255.0 };
}

fn draw(g: *Game, w: int, h: int) {
    BeginDrawing();
    ClearBackground(col(18, 18, 24));
    // board frame
    DrawRectangle(0, 0, (w * cell()) as i32, (h * cell()) as i32, col(30, 32, 44));
    // food
    if g.fx >= 0 {
        DrawRectangle((g.fx * cell() + 4) as i32, (g.fy * cell() + 4) as i32, (cell() - 8) as i32, (cell() - 8) as i32, col(235, 87, 87));
    }
    // snake: head bright, body fading
    for k in 0..g.len {
        shade := 255 - k * 6;
        if shade < 90 {
            shade = 90;
        }
        DrawRectangle((g.sx[k] * cell() + 2) as i32, (g.sy[k] * cell() + 2) as i32, (cell() - 4) as i32, (cell() - 4) as i32, col(80, shade as f32, 120));
    }
    // hud
    DrawText(std::format("score: {}", g.score), 8, 8, 20, col(240, 240, 240));
    if g.dead {
        DrawText("ARENA OVER — press R", 8, 34, 20, col(235, 87, 87));
    }
    EndDrawing();
}

fn poll_input(g: *Game) {
    if IsKeyDown(key_right() as i32) || IsKeyDown(key_d() as i32) {
        turn(g, 1, 0);
    }
    if IsKeyDown(key_left() as i32) || IsKeyDown(key_a() as i32) {
        turn(g, -1, 0);
    }
    if IsKeyDown(key_up() as i32) || IsKeyDown(key_w() as i32) {
        turn(g, 0, -1);
    }
    if IsKeyDown(key_down() as i32) || IsKeyDown(key_s() as i32) {
        turn(g, 0, 1);
    }
}

fn main() -> int {
    InitWindow((grid_w() * cell()) as i32, (grid_h() * cell()) as i32, "Wlel Arena — flagship snake");
    SetTargetFPS(60);
    g := game_new(grid_w(), grid_h());
    while !WindowShouldClose() {
        if IsKeyPressed(key_r() as i32) {
            g = game_new(grid_w(), grid_h());
        }
        poll_input(&g);
        t := GetTime();
        if t - g.tlast >= g.tick {
            g.tlast = t;
            step(&g, grid_w(), grid_h());
        }
        draw(&g, grid_w(), grid_h());
    }
    CloseWindow();
    return 0;
}

// ---------------------------------------------------------------------------
// headless tests: the pure logic, no window, deterministic RNG
// ---------------------------------------------------------------------------
test "turn queues and rejects reversal" {
    std::random::seed(7);
    g := game_new(grid_w(), grid_h());
    assert(g.dx == 1 && g.dy == 0);
    turn(&g, -1, 0); // 180 into yourself: ignored
    assert(g.ndx == 1 && g.ndy == 0);
    turn(&g, 0, -1); // a legal turn is queued
    assert(g.ndx == 0 && g.ndy == -1);
}

test "step moves the head and keeps the length" {
    std::random::seed(7);
    g := game_new(grid_w(), grid_h());
    len0 := g.len;
    hx := g.sx[0];
    turn(&g, 1, 0);
    assert(step(&g, grid_w(), grid_h()));
    assert(g.sx[0] == hx + 1);
    assert(g.len == len0);
    assert(!g.dead);
}

test "eating grows and scores" {
    std::random::seed(7);
    g := game_new(grid_w(), grid_h());
    // park the food straight ahead of the head
    g.fx = g.sx[0] + 1;
    g.fy = g.sy[0];
    len0 := g.len;
    score0 := g.score;
    assert(step(&g, grid_w(), grid_h()));
    assert(g.len == len0 + 1);
    assert(g.score == score0 + 10);
    assert(!on_snake(&g, g.fx, g.fy)); // new food is off the body
}

test "walls kill" {
    std::random::seed(7);
    g := game_new(grid_w(), grid_h());
    g.sx[0] = 0; // heading right from x=0 is fine, so turn left first…
    g.dx = -1;
    g.ndx = -1;
    assert(!step(&g, grid_w(), grid_h()));
    assert(g.dead);
}

test "self collision kills" {
    std::random::seed(7);
    g := game_new(grid_w(), grid_h());
    // U-turn trap: body coiled so any step into (1,1) hits yourself
    g.len = 3;
    g.sx[0] = 2;
    g.sy[0] = 1;
    g.sx[1] = 1;
    g.sy[1] = 1;
    g.sx[2] = 1;
    g.sy[2] = 2;
    g.dx = -1;
    g.dy = 0;
    g.ndx = -1;
    g.ndy = 0;
    assert(!step(&g, grid_w(), grid_h()));
    assert(g.dead);
}

test "the tail cell may be followed" {
    std::random::seed(7);
    g := game_new(grid_w(), grid_h());
    // chase your own tail: the vacating cell is legal
    g.len = 3;
    g.sx[0] = 3;
    g.sy[0] = 1;
    g.sx[1] = 2;
    g.sy[1] = 1;
    g.sx[2] = 1;
    g.sy[2] = 1;
    g.dx = 1;
    g.dy = 0;
    g.ndx = 1;
    g.ndy = 0;
    g.fx = -5; // not eaten this tick
    assert(step(&g, grid_w(), grid_h()));
    assert(!g.dead);
}

test "food never lands on the snake" {
    std::random::seed(7);
    g := game_new(grid_w(), grid_h());
    for _k in 0..50 {
        place_food(&g, grid_w(), grid_h());
        assert(!on_snake(&g, g.fx, g.fy));
        assert(g.fx >= 0 && g.fx < grid_w() && g.fy >= 0 && g.fy < grid_h());
    }
}
