//! The Wlel standard library, written in Wlel itself and compiled into the
//! compiler binary. It is spliced into every program that declares
//! `use std;` (see `main::front`), then flows through the normal
//! type-check and monomorphization pipeline — collections are only
//! instantiated for the types a program actually uses.

/// Source marker recorded on every std function/struct so diagnostics can
/// point into the embedded library instead of a user file.
pub const STD_FILE: &str = "<std>";

pub const STD_WL: &str = r#"
/// The Wlel standard library, pulled in with `use std;`. Collections,
/// Result/Option, the string library and sort/search are written in Wlel
/// itself and flow through the normal pipeline, so only what your program
/// uses is emitted. The compiler builtins (print, math, random, ...) are
/// listed under Builtin functions.


// ---- Result[T, E]: success or failure ------------------------------------

/// The canonical way to report recoverable errors: either `Ok(value)` or
/// `Err(error)`. Construct with the variants, destructure with `match`
/// (exhaustiveness is enforced by the checker), propagate with `expr?`.
enum Result[T, E] {
    Ok(T),
    Err(E),
}

// ---- Option[T]: a value or nothing ---------------------------------------

/// A value (`Some(v)`) or nothing (`None`) — the type of optional data and
/// of lookups that can miss.
enum Option[T] {
    Some(T),
    None,
}

// ---- Result/Option helpers ------------------------------------------------
// The idiomatic layer over match: cheap predicates, non-panicking fallbacks
// and the explicit unwraps. `expr?` inside a function returning Result or
// Option is the terse way to propagate; these cover the remaining cases.

/// true when `r` is the `Ok` variant
fn result_is_ok[T, E](r: Result[T, E]) -> bool {
    return match r {
        Ok(_) => true,
        Err(_) => false,
    };
}

/// true when `r` is the `Err` variant
fn result_is_err[T, E](r: Result[T, E]) -> bool {
    return !result_is_ok(r);
}

/// the success payload; panics with a message when r is an Err
fn result_unwrap[T, E](r: Result[T, E]) -> T {
    match r {
        Ok(v) => { return v; }
        Err(_) => {
            panic("called 'result_unwrap' on an Err value");
        }
    }
}

/// the success payload, or the fallback when r is an Err
fn result_unwrap_or[T, E](r: Result[T, E], fallback: T) -> T {
    match r {
        Ok(v) => { return v; }
        Err(_) => { return fallback; }
    }
}

/// the success payload as Some, or None when r is an Err
fn result_ok[T, E](r: Result[T, E]) -> Option[T] {
    match r {
        Ok(v) => { return Some(v); }
        Err(_) => { return None; }
    }
}

/// the error payload as Some, or None when r is an Ok
fn result_err[T, E](r: Result[T, E]) -> Option[E] {
    match r {
        Ok(_) => { return None; }
        Err(e) => { return Some(e); }
    }
}

/// true when `o` holds a value
fn option_is_some[T](o: Option[T]) -> bool {
    return match o {
        Some(_) => true,
        None => false,
    };
}

/// true when `o` is empty
fn option_is_none[T](o: Option[T]) -> bool {
    return !option_is_some(o);
}

/// the payload; panics with a message when o is None
fn option_unwrap[T](o: Option[T]) -> T {
    match o {
        Some(v) => { return v; }
        None => {
            panic("called 'option_unwrap' on a None value");
        }
    }
}

/// the payload, or the fallback when o is None
fn option_unwrap_or[T](o: Option[T], fallback: T) -> T {
    match o {
        Some(v) => { return v; }
        None => { return fallback; }
    }
}

// ---- method layer ----------------------------------------------------------
// The same operations as the free functions above, attached to their types:
// `r.is_ok()`, `v.push(x)`, `m.set(k, val)`. The free functions stay — std
// internals and existing programs call them directly.

impl Result[T, E] {
    /// true when this Result is `Ok`
    fn is_ok(self) -> bool {
        return result_is_ok(self);
    }
    /// true when this Result is `Err`
    fn is_err(self) -> bool {
        return result_is_err(self);
    }
    /// the success payload; panics when this is an `Err`
    fn unwrap(self) -> T {
        return result_unwrap(self);
    }
    /// the success payload, or `fallback` when this is an `Err`
    fn unwrap_or(self, fallback: T) -> T {
        return result_unwrap_or(self, fallback);
    }
    /// the success payload as an Option (None on `Err`)
    fn ok(self) -> Option[T] {
        return result_ok(self);
    }
    /// the error payload as an Option (None on `Ok`)
    fn err(self) -> Option[E] {
        return result_err(self);
    }
}

impl Option[T] {
    /// true when this Option is `Some`
    fn is_some(self) -> bool {
        return option_is_some(self);
    }
    /// true when this Option is `None`
    fn is_none(self) -> bool {
        return option_is_none(self);
    }
    /// the payload; panics when this is `None`
    fn unwrap(self) -> T {
        return option_unwrap(self);
    }
    /// the payload, or `fallback` when this is `None`
    fn unwrap_or(self, fallback: T) -> T {
        return option_unwrap_or(self, fallback);
    }
}

// ---- Vec[T]: growable array, arena-aware --------------------------------
// Buffers are bump-allocated in the active arena (or the root arena);
// growth abandons the old buffer — freed with the arena, O(1), zero leaks.

/// Growable array. Buffers are bump-allocated in the active arena, so growth
/// abandons the old buffer — it is freed with the arena in O(1), and there
/// are zero leaks by construction. Iterate with `for x in vec { }` (a
/// snapshot: pushing during iteration does not extend the loop).
struct Vec[T] {
    data: *T,
    len: int,
    cap: int,
}

/// a new empty Vec
fn vec_new[T]() -> Vec[T] {
    return Vec { data: 0 as *T, len: 0, cap: 0 };
}

/// a new empty Vec with room for `n` elements preallocated
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

/// append `x` to the end (grows the buffer amortized 2x)
fn vec_push[T](v: *Vec[T], x: T) {
    if v.len == v.cap {
        vec_grow(v, v.len + 1);
    }
    v.data[v.len] = x;
    v.len += 1;
}

/// remove and return the last element (asserts when empty)
fn vec_pop[T](v: *Vec[T]) -> T {
    assert(v.len > 0);
    v.len -= 1;
    return v.data[v.len];
}

/// element at index `i` (asserts on out-of-bounds)
fn vec_get[T](v: *Vec[T], i: int) -> T {
    assert(i >= 0);
    assert(i < v.len);
    return v.data[i];
}

/// overwrite the element at index `i` (asserts on out-of-bounds)
fn vec_set[T](v: *Vec[T], i: int, x: T) {
    assert(i >= 0);
    assert(i < v.len);
    v.data[i] = x;
}

/// number of elements
fn vec_len[T](v: *Vec[T]) -> int {
    return v.len;
}

/// drop all elements (capacity is kept)
fn vec_clear[T](v: *Vec[T]) {
    v.len = 0;
}

impl Vec[T] {
    /// append `x` to the end
    fn push(self: *Vec[T], x: T) {
        vec_push(self, x);
    }
    /// remove and return the last element (asserts when empty)
    fn pop(self: *Vec[T]) -> T {
        return vec_pop(self);
    }
    /// element at index `i` (asserts on out-of-bounds)
    fn get(self: *Vec[T], i: int) -> T {
        return vec_get(self, i);
    }
    /// overwrite the element at index `i` (asserts on out-of-bounds)
    fn set(self: *Vec[T], i: int, x: T) {
        vec_set(self, i, x);
    }
    /// number of elements
    fn len(self: *Vec[T]) -> int {
        return vec_len(self);
    }
    /// drop all elements (capacity is kept)
    fn clear(self: *Vec[T]) {
        vec_clear(self);
    }
}

// ---- HashMap[K, V]: open addressing, linear probing ----------------------
// Slots carry a state byte: 0 empty, 1 live, 2 tombstone. Keys hash through
// the _wlel_hash builtin (checker picks the implementation per key type).
// The table rehashes when more than half the slots are live or deleted, so
// probes stay short and an insert can never run out of empty slots.

/// Hash map with open addressing and tombstones. Keys may be int, float,
/// bool, string or pointer (struct keys are rejected). Buffers live in the
/// active arena; the table rehashes when more than half the slots are used.
struct HashMap[K, V] {
    state: *u8,
    keys: *K,
    vals: *V,
    len: int,
    used: int,
    cap: int,
}

/// a new empty map
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

/// insert or overwrite `key -> val` (grows/rehashes as needed)
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

/// value stored under `key` (asserts when the key is absent)
fn map_get[K, V](m: *HashMap[K, V], key: K) -> V {
    s := map_find(m, key);
    assert(s >= 0);
    return m.vals[s];
}

/// value stored under `key`, or `fallback` when absent
fn map_get_or[K, V](m: *HashMap[K, V], key: K, fallback: V) -> V {
    s := map_find(m, key);
    if s < 0 {
        return fallback;
    }
    return m.vals[s];
}

/// true when `key` is present
fn map_has[K, V](m: *HashMap[K, V], key: K) -> bool {
    return map_find(m, key) >= 0;
}

/// remove `key`; true when it was present
fn map_del[K, V](m: *HashMap[K, V], key: K) -> bool {
    s := map_find(m, key);
    if s < 0 {
        return false;
    }
    m.state[s] = 2;
    m.len -= 1;
    return true;
}

/// all live keys in table order (unspecified); pair with
/// `for k in map_keys(&m)`
fn map_keys[K, V](m: *HashMap[K, V]) -> Vec[K] {
    keys := vec_new[K]();
    j := 0;
    while j < m.cap {
        if m.state[j] == 1 {
            keys.push(m.keys[j]);
        }
        j += 1;
    }
    return keys;
}

/// drop all entries (capacity is kept)
fn map_clear[K, V](m: *HashMap[K, V]) {
    j := 0;
    while j < m.cap {
        m.state[j] = 0;
        j += 1;
    }
    m.len = 0;
    m.used = 0;
}

impl HashMap[K, V] {
    /// insert or overwrite `key -> val`
    fn set(self: *HashMap[K, V], key: K, val: V) {
        map_set(self, key, val);
    }
    /// value stored under `key` (asserts when the key is absent)
    fn get(self: *HashMap[K, V], key: K) -> V {
        return map_get(self, key);
    }
    /// value stored under `key`, or `fallback` when absent
    fn get_or(self: *HashMap[K, V], key: K, fallback: V) -> V {
        return map_get_or(self, key, fallback);
    }
    /// true when `key` is present
    fn has(self: *HashMap[K, V], key: K) -> bool {
        return map_has(self, key);
    }
    /// remove `key`; true when it was present
    fn del(self: *HashMap[K, V], key: K) -> bool {
        return map_del(self, key);
    }
    /// all live keys in table order (unspecified)
    fn keys(self: *HashMap[K, V]) -> Vec[K] {
        return map_keys(self);
    }
    /// drop all entries (capacity is kept)
    fn clear(self: *HashMap[K, V]) {
        map_clear(self);
    }
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

/// byte index of the first occurrence of `needle` in `hay` at or after
/// `from` (-1 when absent); an empty needle matches at `from`
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

/// byte index of the first occurrence of `needle` in `hay` (-1 when absent)
fn str_find(hay: string, needle: string) -> int {
    return str_find_from(hay, needle, 0);
}

/// copy of `s[start .. start+n)`, clamped to the string bounds; an empty or
/// out-of-range window yields ""
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

/// copy of `s` without leading/trailing ASCII whitespace
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

/// pieces of `s` between occurrences of `sep` (the separator never appears
/// in the result); consecutive separators yield empty pieces and splitting
/// "" yields one empty piece; `sep` must not be empty (asserts)
fn str_split(s: string, sep: string) -> Vec[string] {
    sl := std::strlen(sep);
    assert(sl > 0);
    parts := vec_new[string]();
    start := 0;
    pos := str_find_from(s, sep, start);
    while pos >= 0 {
        parts.push(str_sub(s, start, pos - start));
        start = pos + sl;
        pos = str_find_from(s, sep, start);
    }
    parts.push(str_sub(s, start, std::strlen(s) - start));
    return parts;
}

/// three-way byte compare (like C strcmp): negative when a < b, zero when
/// equal, positive when a > b — the ready-made comparator for sorting
/// strings (pass it to std::sort / std::binary_search directly)
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

/// strict decimal parse: optional +/- then digits only, no whitespace;
/// overflow-safe via checked arithmetic; on false `*out` is untouched.
/// Digits accumulate as a NEGATIVE value so every 64-bit magnitude fits,
/// including i64::MIN; the '+' sign only fits magnitudes <= i64::MAX.
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

/// decimal string of `v` (minimal digits, '-' for negatives)
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

/// Splice the embedded library into a parsed program (structs, enums,
/// impls, functions), tagging every definition with STD_FILE — the exact
/// transformation `use std` triggers in the CLI front end.
pub fn splice_std(program: &mut crate::ast::Program) {
    let mut std_prog = parse_std();
    for f in std_prog.funcs.iter_mut() {
        f.file = STD_FILE.into();
    }
    for s in std_prog.structs.iter_mut() {
        s.file = STD_FILE.into();
    }
    for e in std_prog.enums.iter_mut() {
        e.file = STD_FILE.into();
    }
    for i in std_prog.impls.iter_mut() {
        i.file = STD_FILE.into();
    }
    program.structs.splice(0..0, std_prog.structs);
    program.enums.splice(0..0, std_prog.enums);
    program.impls.splice(0..0, std_prog.impls);
    program.funcs.splice(0..0, std_prog.funcs);
}
