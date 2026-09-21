//! The Wlel standard library, written in Wlel itself and compiled into the
//! compiler binary. It is spliced into every program that declares
//! `use std;` (see `main::front`), then flows through the normal
//! type-check and monomorphization pipeline — collections are only
//! instantiated for the types a program actually uses.

/// Source marker recorded on every std function/struct so diagnostics can
/// point into the embedded library instead of a user file.
pub const STD_FILE: &str = "<std>";

pub const STD_WL: &str = r#"
// ---- Result[T, E]: success or failure ------------------------------------
// The canonical way to report recoverable errors. Construct with Ok/Err,
// destructure with match (exhaustiveness is enforced by the checker).

enum Result[T, E] {
    Ok(T),
    Err(E),
}

// ---- Option[T]: a value or nothing ---------------------------------------

enum Option[T] {
    Some(T),
    None,
}

// ---- Vec[T]: growable array, arena-aware --------------------------------
// Buffers are bump-allocated in the active arena (or the root arena);
// growth abandons the old buffer — freed with the arena, O(1), zero leaks.

struct Vec[T] {
    data: *T,
    len: int,
    cap: int,
}

fn vec_new[T]() -> Vec[T] {
    return Vec { data: 0 as *T, len: 0, cap: 0 };
}

fn vec_with_cap[T](n: int) -> Vec[T] {
    v := Vec { data: new(T, n), len: 0, cap: n };
    return v;
}

fn vec_grow[T](v: *Vec[T], min_cap: int) {
    nc := v.cap * 2;
    if nc < 4 { nc = 4; }
    if nc < min_cap { nc = min_cap; }
    nd := new(T, nc);
    i := 0;
    while i < v.len {
        nd[i] = v.data[i];
        i += 1;
    }
    v.data = nd;
    v.cap = nc;
}

fn vec_push[T](v: *Vec[T], x: T) {
    if v.len == v.cap {
        vec_grow(v, v.len + 1);
    }
    v.data[v.len] = x;
    v.len += 1;
}

fn vec_pop[T](v: *Vec[T]) -> T {
    assert(v.len > 0);
    v.len -= 1;
    return v.data[v.len];
}

fn vec_get[T](v: *Vec[T], i: int) -> T {
    assert(i >= 0);
    assert(i < v.len);
    return v.data[i];
}

fn vec_set[T](v: *Vec[T], i: int, x: T) {
    assert(i >= 0);
    assert(i < v.len);
    v.data[i] = x;
}

fn vec_len[T](v: *Vec[T]) -> int {
    return v.len;
}

fn vec_clear[T](v: *Vec[T]) {
    v.len = 0;
}

// ---- HashMap[K, V]: open addressing, linear probing ----------------------
// Slots carry a state byte: 0 empty, 1 live, 2 tombstone. Keys hash through
// the _wlel_hash builtin (checker picks the implementation per key type).
// The table rehashes when more than half the slots are live or deleted, so
// probes stay short and an insert can never run out of empty slots.

struct HashMap[K, V] {
    state: *u8,
    keys: *K,
    vals: *V,
    len: int,
    used: int,
    cap: int,
}

fn map_new[K, V]() -> HashMap[K, V] {
    return HashMap { state: 0 as *u8, keys: 0 as *K, vals: 0 as *V, len: 0, used: 0, cap: 0 };
}

fn map_alloc[K, V](m: *HashMap[K, V], cap: int) {
    m.state = new(u8, cap);
    m.keys = new(K, cap);
    m.vals = new(V, cap);
    j := 0;
    while j < cap {
        m.state[j] = 0;
        j += 1;
    }
    m.cap = cap;
    m.used = 0;
}

// index of key if present, -1 otherwise; stops at the first empty slot
fn map_find[K, V](m: *HashMap[K, V], key: K) -> int {
    if m.cap == 0 {
        return -1;
    }
    mask := m.cap - 1;
    i := (_wlel_hash(key) as int) & mask;
    while true {
        st := m.state[i];
        if st == 0 {
            return -1;
        }
        if st == 1 {
            if m.keys[i] == key {
                return i;
            }
        }
        i = (i + 1) & mask;
    }
    return -1;
}

// slot to write: an existing key keeps its slot (value overwritten),
// otherwise the first tombstone at or after the hash position is reused
fn map_slot_for_insert[K, V](m: *HashMap[K, V], key: K) -> int {
    mask := m.cap - 1;
    i := (_wlel_hash(key) as int) & mask;
    tomb := -1;
    while true {
        st := m.state[i];
        if st == 0 {
            if tomb >= 0 {
                return tomb;
            }
            return i;
        }
        if st == 1 {
            if m.keys[i] == key {
                return i;
            }
        } else {
            if tomb < 0 {
                tomb = i;
            }
        }
        i = (i + 1) & mask;
    }
    return -1;
}

// low-level insert: table must have room (map_set enforces the invariant)
fn map_insert[K, V](m: *HashMap[K, V], key: K, val: V) {
    s := map_slot_for_insert(m, key);
    if m.state[s] == 1 {
        m.vals[s] = val;
        return;
    }
    if m.state[s] == 0 {
        m.used += 1;
    }
    m.len += 1;
    m.state[s] = 1;
    m.keys[s] = key;
    m.vals[s] = val;
}

fn map_set[K, V](m: *HashMap[K, V], key: K, val: V) {
    if m.cap == 0 {
        map_alloc(m, 8);
    } else if (m.used + 1) * 2 > m.cap {
        map_rehash(m, m.cap * 2);
    }
    map_insert(m, key, val);
}

fn map_rehash[K, V](m: *HashMap[K, V], new_cap: int) {
    old_state := m.state;
    old_keys := m.keys;
    old_vals := m.vals;
    old_cap := m.cap;
    old_len := m.len;
    map_alloc(m, new_cap);
    j := 0;
    while j < old_cap {
        if old_state[j] == 1 {
            map_insert(m, old_keys[j], old_vals[j]);
        }
        j += 1;
    }
    m.len = old_len;
}

fn map_get[K, V](m: *HashMap[K, V], key: K) -> V {
    s := map_find(m, key);
    assert(s >= 0);
    return m.vals[s];
}

fn map_get_or[K, V](m: *HashMap[K, V], key: K, fallback: V) -> V {
    s := map_find(m, key);
    if s < 0 {
        return fallback;
    }
    return m.vals[s];
}

fn map_has[K, V](m: *HashMap[K, V], key: K) -> bool {
    return map_find(m, key) >= 0;
}

fn map_del[K, V](m: *HashMap[K, V], key: K) -> bool {
    s := map_find(m, key);
    if s < 0 {
        return false;
    }
    m.state[s] = 2;
    m.len -= 1;
    return true;
}

// live keys in table order (unspecified); pair with `for k in map_keys(&m)`
fn map_keys[K, V](m: *HashMap[K, V]) -> Vec[K] {
    keys := vec_new[K]();
    j := 0;
    while j < m.cap {
        if m.state[j] == 1 {
            vec_push(&keys, m.keys[j]);
        }
        j += 1;
    }
    return keys;
}

fn map_clear[K, V](m: *HashMap[K, V]) {
    j := 0;
    while j < m.cap {
        m.state[j] = 0;
        j += 1;
    }
    m.len = 0;
    m.used = 0;
}

// ---- String library: search, slice, split, parse, to-string --------------
// Strings are immutable NUL-terminated byte strings (indexing s[i] reads
// one byte, the NUL at s[len] included). Every function that builds a new
// string allocates from the ACTIVE arena (or the root arena), so results
// and intermediates all die together at the block exit.

// true when c is ASCII whitespace (space, \t \n \v \f \r)
fn str_is_space(c: u8) -> bool {
    return c == 32 || (c >= 9 && c <= 13);
}

// index of the first occurrence of needle in hay at or after `from`
// (byte index, -1 when absent); an empty needle matches at `from`
fn str_find_from(hay: string, needle: string, from: int) -> int {
    hl := std::strlen(hay);
    nl := std::strlen(needle);
    if nl == 0 {
        return from;
    }
    i := from;
    if i < 0 {
        i = 0;
    }
    while i + nl <= hl {
        j := 0;
        while j < nl && hay[i + j] == needle[j] {
            j += 1;
        }
        if j == nl {
            return i;
        }
        i += 1;
    }
    return -1;
}

// first occurrence of needle in hay, -1 when absent
fn str_find(hay: string, needle: string) -> int {
    return str_find_from(hay, needle, 0);
}

// copy of s[start .. start+n), clamped to the string bounds; an empty or
// out-of-range window yields ""
fn str_sub(s: string, start: int, n: int) -> string {
    len := std::strlen(s);
    b := start;
    if b < 0 {
        b = 0;
    }
    if b > len {
        b = len;
    }
    e := b + n;
    if e > len {
        e = len;
    }
    if e <= b {
        return "";
    }
    count := e - b;
    buf := new(u8, count + 1);
    i := 0;
    while i < count {
        buf[i] = s[b + i];
        i += 1;
    }
    buf[count] = 0;
    return buf as string;
}

// copy of s without leading/trailing ASCII whitespace
fn str_trim(s: string) -> string {
    len := std::strlen(s);
    b := 0;
    while b < len && str_is_space(s[b]) {
        b += 1;
    }
    e := len;
    while e > b && str_is_space(s[e - 1]) {
        e -= 1;
    }
    return str_sub(s, b, e - b);
}

// pieces of s between occurrences of sep (the separator never appears in
// the result); consecutive separators yield empty pieces and splitting ""
// yields one empty piece; sep must not be empty (asserts)
fn str_split(s: string, sep: string) -> Vec[string] {
    sl := std::strlen(sep);
    assert(sl > 0);
    parts := vec_new[string]();
    start := 0;
    pos := str_find_from(s, sep, start);
    while pos >= 0 {
        vec_push(&parts, str_sub(s, start, pos - start));
        start = pos + sl;
        pos = str_find_from(s, sep, start);
    }
    vec_push(&parts, str_sub(s, start, std::strlen(s) - start));
    return parts;
}

// three-way byte compare (like C strcmp): negative when a < b, zero when
// equal, positive when a > b — the ready-made comparator for sorting
// strings (pass it to std::sort / std::binary_search directly)
fn str_cmp(a: string, b: string) -> int {
    i := 0;
    while a[i] != 0 && a[i] == b[i] {
        i += 1;
    }
    if a[i] < b[i] {
        return -1;
    }
    if a[i] > b[i] {
        return 1;
    }
    return 0;
}

// strict decimal parse: optional +/- then digits only, no whitespace;
// overflow-safe via checked arithmetic; on false *out is untouched.
// Digits accumulate as a NEGATIVE value so every 64-bit magnitude fits,
// including i64::MIN; the '+' sign only fits magnitudes <= i64::MAX.
fn str_parse_int(s: string, out: *int) -> bool {
    i := 0;
    neg := false;
    if s[0] == 45 {
        // '-'
        neg = true;
        i = 1;
    } else if s[0] == 43 {
        // '+'
        i = 1;
    }
    if s[i] == 0 {
        return false;
    }
    val := 0;
    while s[i] != 0 {
        c := s[i];
        if c < 48 || c > 57 {
            return false;
        }
        if !std::checked_mul(val, 10, &val) {
            return false;
        }
        if !std::checked_sub(val, (c - 48) as int, &val) {
            return false;
        }
        i += 1;
    }
    if !neg {
        if val < -9223372036854775807 {
            return false;
        }
        val = -val;
    }
    *out = val;
    return true;
}

// decimal string of v (minimal digits, '-' for negatives); 21 bytes always
// fits a 64-bit value with sign and NUL, so the buffer never grows
fn int_to_str(v: int) -> string {
    if v == 0 {
        return "0";
    }
    count := 0;
    if v < 0 {
        count = 1;
    }
    mut := v;
    while mut != 0 {
        count += 1;
        mut /= 10;
    }
    buf := new(u8, count + 1);
    buf[count] = 0;
    mut = v;
    i := count;
    while mut != 0 {
        d := mut % 10;
        if d < 0 {
            d = -d;
        }
        i -= 1;
        buf[i] = (48 + d) as u8;
        mut /= 10;
    }
    if v < 0 {
        buf[0] = 45;
    }
    return buf as string;
}
"#;

/// Lex + parse the embedded std source. Panics on failure: the library is a
/// compiler-internal constant, and a broken std is a build error, not a
/// user-facing diagnostic.
pub fn parse_std() -> crate::ast::Program {
    let toks = crate::lexer::Lexer::new(STD_WL)
        .tokenize()
        .expect("std library lexes");
    let (program, errs) = crate::parser::Parser::new(&toks).program();
    assert!(
        errs.is_empty(),
        "std library parses cleanly: {errs:?}"
    );
    program
}
