// arena_stats(): active-arena statistics — bytes, chunks, peak (high-water)
// Called with no arguments; reports the innermost arena that is currently
// active (the root arena when no block is open).
use std;

fn show(s: ArenaStats) {
    std::print_str("  bytes=");
    std::print_int(s.bytes);
    std::print_str("  chunks=");
    std::print_int(s.chunks);
    std::print_str("  peak=");
    std::println_int(s.peak);
}

fn main() -> int {
    std::println_str("root (initial):");
    show(arena_stats());
    arena(4096) {
        xs := new(int, 100);
        for i in 0..100 {
            xs[i] = i * i;
        }
        std::println_str("inside arena(4096) after 100 ints:");
        show(arena_stats());
        ys := new(int, 1000);
        ys[999] = 7;
        std::println_str("plus 1000 more ints (chunk overflow):");
        show(arena_stats());
    }
    std::println_str("root (after the arena block exits):");
    show(arena_stats());
    return 0;
}

test "arena_stats reports allocations" {
    arena(1024) {
        let s0: ArenaStats = arena_stats();
        assert_eq(s0.bytes, 0);
        assert_eq(s0.chunks, 1);
        p := new(int, 10);
        p[9] = 1;
        let s1: ArenaStats = arena_stats();
        // int = i64 = 8 bytes -> 10 ints = 80 bytes
        assert_eq(s1.bytes, 80);
        assert_eq(s1.chunks, 1);
        assert_eq(s1.peak, 80);
    }
}
