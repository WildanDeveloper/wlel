// arena.wl — first-class scoped arenas (Wlel v0.5 signature feature)
// Zero manual free, zero GC pause, bump-allocated in cache-friendly chunks.
use std;

struct Node {
    val: int,
    next: *Node
}

struct Point {
    x: float,
    y: float
}

fn build_list(n: int) -> *Node {
    // uses the caller's active arena!
    let head: *Node = 0 as *Node;
    i := 0;
    while i < n {
        node := new(Node);
        node.val = i;
        node.next = head;
        head = node;
        i += 1;
    }
    return head;
}

fn sum_list(list: *Node) -> int {
    total := 0;
    cur := list;
    while (cur as int) != 0 {
        total += cur.val;
        cur = cur.next;
    }
    return total;
}

fn main() -> int {
    std::println_str("== Wlel Arena Demo ==");

    // 1. Large allocation: 100k nodes in scratch arena
    sum := 0;
    arena(1 << 20) { // 1 MiB bump arena
        list := build_list(100);
        sum = sum_list(list); // 0 + 1 + ... + 99 = 4950
        std::println_int(sum);

        // mass array allocation
        pts := new(Point, 1000);
        pts[0].x = 3.0;
        pts[0].y = 4.0;
        std::println_float(pts[0].x + pts[0].y);

        // nested arena (scratchpad inside a frame)
        arena(512) {
            scratch := new(int, 5);
            scratch[0] = 42;
            std::println_int(scratch[0]);
        } // 512 bytes freed here

        defer std::println_str("outer arena exiting...");
    } // ALL 100 nodes + 1000 Points freed here in O(1)! Zero leak.

    // 2. Early return safety test
    res := test_early_return(true);
    std::println_int(res);

    return 0;
}

fn test_early_return(early: bool) -> int {
    arena(1024) {
        p := new(Point);
        p.x = 99.0;
        if early {
            // arena freed automatically before return via LIFO defer!
            return p.x as int;
        }
    }
    return 0;
}
