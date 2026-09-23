// json.wl — flagship app 1: a JSON parser + encoder written in Wlel.
//
//   wlel run flagship/json.wl              — demo: parse, query, dump, errors
//   wlel run flagship/json.wl -- bench     — throughput benchmark (64 MB corpus)
//   wlel run flagship/json.wl -- bench 8 50
//   wlel test flagship/json.wl             — the test blocks below
//   wlel build flagship/json.wl -O2 -o json
//
// The parser is a hand-rolled recursive-descent scanner over the raw bytes
// (strict RFC 8259): strings with full \uXXXX + surrogate-pair decoding,
// numbers via strtod (exact IEEE, non-finite rejected), depth-limited
// recursion, line:column positions in every error. Values live in the
// active arena — a document frees in O(1) when its arena closes.
//
// The benchmark parses a generated corpus in memory and asserts the
// flagship criterion: >= 100 MB/s on a single core (release build).
use std;

// ---------------------------------------------------------------------------
// value model
// ---------------------------------------------------------------------------

/// A parsed JSON value. Containers are heap (arena) headers the enum points
/// at — a 16-byte Jval keeps the Result/Vec/HashMap struct traffic small
/// (the parser's hot path passes these by value); everything is
/// arena-allocated, so a whole document dies with its arena.
enum Jval {
    JNull,
    JBool(bool),
    JNum(float),
    JStr(string),
    JArr(*Vec[Jval]),
    JObj(*HashMap[string, Jval])
}

/// recursive-descent parser state: input + byte offset
struct Jp {
    s: string,
    i: int
}

impl Jp {

    fn peek(self: *Jp) -> u8 {
        /// current byte (0 at end of input — NUL is the sentinel)
        return self.s[self.i];
    }

    fn skip_ws(self: *Jp) {
        while true {
            c := self.peek();
            if c == 32 || c == 9 || c == 10 || c == 13 {
                self.i += 1;
            } else {                return;
            }
        }
    }

    fn pos(self: *Jp) -> string {
        /// position text for error messages ("line L, column C, offset N")
        line := 1;
        col := 1;
        for k in 0..self.i {
            if self.s[k] == 10 {
                line += 1;
                col = 1;
            } else {                col += 1;
            }
        }
        return std::format("line {}, column {}, offset {}", line, col, self.i);
    }
}

/// build a parser error with position info
fn jerr(p: *Jp, msg: string) -> Result[Jval, string] {
    return Err("json: " + msg + " at " + p.pos());
}

/// strict JSON number grammar [-]digits[.digits][eE[+-]digits]; the scan
/// validates the shape, strtod does the exact IEEE conversion (std's
/// parse_float is strict full-consume and overflow-safe)
fn jp_number(p: *Jp) -> Result[Jval, string] {
    start := p.i;
    if p.peek() == 45 {
        // '-'
        p.i += 1;
    }
    // strict RFC 8259: no leading zeros ("0" alone is fine, "01" is not)
    if p.peek() == 48 {
        // '0'
        p.i += 1;
        if p.peek() >= 48 && p.peek() <= 57 {
            return jerr(p, "leading zero in number");
        }
    } else {        digits := 0;
        while p.peek() >= 48 && p.peek() <= 57 {
            p.i += 1;
            digits += 1;
        }
        if digits == 0 {
            return jerr(p, "bad number");
        }
    }
    if p.peek() == 46 {
        // '.'
        p.i += 1;
        if !(p.peek() >= 48 && p.peek() <= 57) {
            return jerr(p, "bad number fraction"); // "1." is not JSON
        }
        while p.peek() >= 48 && p.peek() <= 57 {
            p.i += 1;
        }
    }
    if p.peek() == 101 || p.peek() == 69 {
        // 'e' 'E'
        p.i += 1;
        if p.peek() == 43 || p.peek() == 45 {
            // '+' '-'
            p.i += 1;
        }
        if !(p.peek() >= 48 && p.peek() <= 57) {
            return jerr(p, "bad number exponent");
        }
        while p.peek() >= 48 && p.peek() <= 57 {
            p.i += 1;
        }
    }
    // copy the token into a small buffer: str_sub would strlen() the whole
    // document per number (O(n^2) parse); tokens > 39 bytes take the slow
    // clamped path (valid JSON, vanishingly rare)
    v := 0.0;
    toklen := p.i - start;
    if toklen <= 39 {
        buf := new(u8, 40);
        for k in 0..toklen {
            buf[k] = p.s[start + k];
        }
        buf[toklen] = 0;
        if !std::parse_float(buf as string, &v) {
            return jerr(p, "number out of range");
        }
    } else {        tok := str_sub(p.s, start, toklen);
        if !std::parse_float(tok, &v) {
            return jerr(p, "number out of range");
        }
    }
    if v != v || v - v != 0.0 {
        return jerr(p, "number out of range"); // nan / inf
    }
    return Ok(JNum(v));
}

/// one hex digit of a \uXXXX escape
fn jp_hex(p: *Jp) -> Result[int, string] {
    c := p.peek();
    if c >= 48 && c <= 57 {
        p.i += 1;
        return Ok((c - 48) as int);
    }
    if c >= 97 && c <= 102 {
        p.i += 1;
        return Ok((c - 97 + 10) as int);
    }
    if c >= 65 && c <= 70 {
        p.i += 1;
        return Ok((c - 65 + 10) as int);
    }
    return Err("json: bad \\u escape at " + p.pos());
}

/// append one code point to buf as UTF-8, return the next write index
fn jp_utf8(buf: *u8, n: int, cp: int) -> int {
    if cp < 128 {
        buf[n] = cp as u8;
        return n + 1;
    }
    if cp < 2048 {
        buf[n] = (192 | cp >> 6) as u8;
        buf[n + 1] = (128 | cp & 63) as u8;
        return n + 2;
    }
    if cp < 65536 {
        buf[n] = (224 | cp >> 12) as u8;
        buf[n + 1] = (128 | cp >> 6 & 63) as u8;
        buf[n + 2] = (128 | cp & 63) as u8;
        return n + 3;
    }
    buf[n] = (240 | cp >> 18) as u8;
    buf[n + 1] = (128 | cp >> 12 & 63) as u8;
    buf[n + 2] = (128 | cp >> 6 & 63) as u8;
    buf[n + 3] = (128 | cp & 63) as u8;
    return n + 4;
}

/// parse a JSON string starting at the opening quote. Fast path: no
/// escapes and no control characters -> slice the input in place.
/// Slow path: decode into an arena buffer (escapes, surrogate pairs,
/// UTF-8 encoding). Strict: raw control characters are rejected.
fn jp_string(p: *Jp) -> Result[Jval, string] {
    if p.peek() != 34 {
        // '"'
        return jerr(p, "expected string");
    }
    p.i += 1;
    start := p.i;
    // fast scan: find the closing quote or the first backslash
    while true {
        c := p.peek();
        if c == 34 {
            // '"'
            // copy straight into an arena buffer: str_sub would strlen()
            // the whole document per string (O(n^2) parse)
            n := p.i - start;
            buf := new(u8, n + 1);
            for k in 0..n {
                buf[k] = p.s[start + k];
            }
            buf[n] = 0;
            p.i += 1;
            return Ok(JStr(buf as string));
        }
        if c == 92 || c == 0 || c < 32 {
            // '\\' NUL control
            break;
        }
        p.i += 1;
    }
    if p.peek() != 92 {
        if p.peek() == 0 {
            return jerr(p, "unterminated string");
        }
        return jerr(p, "raw control character in string");
    }
    // slow path with escapes: find the closing quote first (bounded to this
    // string — sizing via strlen(p.s) would allocate document-size per
    // escaped string and explode the arena), then decode in place
    end := p.i;
    while true {
        c2 := p.s[end];
        if c2 == 34 {
            // '"'
            break;
        }
        if c2 == 92 {
            // '\\' — escape char + payload byte (hex digits of \u
            // cannot contain a quote, so no raw quote is ever skipped)
            end += 2;
            continue;
        }
        if c2 == 0 {
            return jerr(p, "unterminated string");
        }
        if c2 < 32 {
            return jerr(p, "raw control character in string");
        }
        end += 1;
    }
    buf := new(u8, end - start + 1);
    n := p.i - start;
    for k in 0..n {
        buf[k] = p.s[start + k];
    }
    while true {
        c := p.peek();
        if c == 34 {
            // '"'
            buf[n] = 0;
            p.i += 1;
            return Ok(JStr(buf as string));
        }
        if c == 0 {
            return jerr(p, "unterminated string");
        }
        if c < 32 {
            return jerr(p, "raw control character in string");
        }
        if c != 92 {
            // ordinary byte
            buf[n] = c;
            n += 1;
            p.i += 1;
            continue;
        }
        p.i += 1; // consume backslash
        e := p.peek();
        if e == 34 || e == 92 || e == 47 {
            // '"' '\' '/'
            buf[n] = e;
            n += 1;
            p.i += 1;
            continue;
        }
        if e == 98 {
            buf[n] = 8; // \b
        } else if e == 102 {
            buf[n] = 12; // \f
        } else if e == 110 {
            buf[n] = 10; // \n
        } else if e == 114 {
            buf[n] = 13; // \r
        } else if e == 116 {
            buf[n] = 9; // \t
        } else if e == 117 {

            // \uXXXX with surrogate-pair decoding (strict)
            p.i += 1;
            cp := jp_hex4(p)?;
            if cp >= 55296 && cp <= 56319 {
                if p.peek() != 92 || p.s[p.i + 1] != 117 {
                    return jerr(p, "lone leading surrogate");
                }
                p.i += 2;
                lo := jp_hex4(p)?;
                if !(lo >= 56320 && lo <= 57343) {
                    return jerr(p, "bad trailing surrogate");
                }
                cp = 65536 + (cp - 55296 << 10) + (lo - 56320);
            } else if cp >= 56320 && cp <= 57343 {
                return jerr(p, "lone trailing surrogate");
            }
            n = jp_utf8(buf, n, cp);
            continue;
        } else {            return jerr(p, "bad escape");
        }
        n += 1;
        p.i += 1;
    }
    return jerr(p, "unreachable");
}

/// four hex digits after \u (the backslash-u is already consumed)
fn jp_hex4(p: *Jp) -> Result[int, string] {
    cp := 0;
    for _k in 0..4 {
        d := jp_hex(p)?;
        cp = cp * 16 + d;
    }
    return Ok(cp);
}

/// true/false/null with a prefix check (direct byte compare: str_sub
/// would strlen() the whole document per literal — O(n^2) parse)
fn jp_lit(p: *Jp, word: string) -> bool {
    n := std::strlen(word);
    for k in 0..n {
        if p.s[p.i + k] != word[k] {
            return false;
        }
    }
    p.i += n;
    return true;
}

/// one JSON value; depth caps the C recursion (hostile nesting gets a
/// clean error, mirroring the compiler's own parser guard)
fn jp_value(p: *Jp, depth: int) -> Result[Jval, string] {
    if depth > 256 {
        return jerr(p, "nesting too deep (limit 256)");
    }
    p.skip_ws();
    c := p.peek();
    if c == 123 {
        // '{'
        return jp_object(p, depth);
    }
    if c == 91 {
        // '['
        return jp_array(p, depth);
    }
    if c == 34 {
        // '"'
        return jp_string(p);
    }
    if c == 45 || c >= 48 && c <= 57 {
        // '-' digits
        return jp_number(p);
    }
    if jp_lit(p, "true") {
        return Ok(JBool(true));
    }
    if jp_lit(p, "false") {
        return Ok(JBool(false));
    }
    if jp_lit(p, "null") {
        return Ok(JNull);
    }
    return jerr(p, "unexpected character");
}

fn jp_array(p: *Jp, depth: int) -> Result[Jval, string] {
    p.i += 1; // '['
    items := new(Vec[Jval]);
    *items = vec_new[Jval]();
    p.skip_ws();
    if p.peek() == 93 {
        // ']'
        p.i += 1;
        return Ok(JArr(items));
    }
    while true {
        v := jp_value(p, depth + 1)?;
        items.push(v);
        p.skip_ws();
        c := p.peek();
        if c == 44 {
            // ','
            p.i += 1;
            continue;
        }
        if c == 93 {
            // ']'
            p.i += 1;
            return Ok(JArr(items));
        }
        return jerr(p, "expected ',' or ']'");
    }
    return jerr(p, "unreachable");
}

fn jp_object(p: *Jp, depth: int) -> Result[Jval, string] {
    p.i += 1; // '{'
    m := new(HashMap[string, Jval]);
    *m = map_new[string, Jval]();
    p.skip_ws();
    if p.peek() == 125 {
        // '}'
        p.i += 1;
        return Ok(JObj(m));
    }
    while true {
        k := jp_string(p)?;
        key := jstr(k).unwrap_or("");
        p.skip_ws();
        if p.peek() != 58 {
            // ':'
            return jerr(p, "expected ':'");
        }
        p.i += 1;
        v := jp_value(p, depth + 1)?;
        m.set(key, v); // duplicate keys: last wins (documented)
        p.skip_ws();
        c := p.peek();
        if c == 44 {
            // ','
            p.i += 1;
            p.skip_ws();
            continue;
        }
        if c == 125 {
            // '}'
            p.i += 1;
            return Ok(JObj(m));
        }
        return jerr(p, "expected ',' or '}'");
    }
    return jerr(p, "unreachable");
}

/// parse one complete JSON document (strict: trailing garbage is an error)
fn jparse(s: string) -> Result[Jval, string] {
    p := Jp { s: s, i: 0 };
    v := jp_value(&p, 0)?;
    p.skip_ws();
    if p.peek() != 0 {
        return jerr(&p, "trailing garbage");
    }
    return Ok(v);
}

/// parse a whole file (whole-file read, then jparse)
fn jparse_file(path: string) -> Result[Jval, string] {
    content := std::fs::read_all(path)?;
    return jparse(content);
}

// ---------------------------------------------------------------------------
// accessors — the Option-returning query layer
// ---------------------------------------------------------------------------
fn jget(v: Jval, key: string) -> Option[Jval] {
    let hit: Option[Jval] = None;
    match v {
        JObj(m) => {
            if m.has(key) {
                hit = Some(m.get(key));
            }
        }
        _ => {
        }
    }
    return hit;
}

fn jat(v: Jval, i: int) -> Option[Jval] {
    let hit: Option[Jval] = None;
    match v {
        JArr(items) => {
            if i >= 0 && i < items.len() {
                hit = Some(items.get(i));
            }
        }
        _ => {
        }
    }
    return hit;
}

fn jnum(v: Jval) -> Option[float] {
    let hit: Option[float] = None;
    match v {
        JNum(x) => {
            hit = Some(x);
        }
        _ => {
        }
    }
    return hit;
}

fn jstr(v: Jval) -> Option[string] {
    let hit: Option[string] = None;
    match v {
        JStr(s) => {
            hit = Some(s);
        }
        _ => {
        }
    }
    return hit;
}

fn jbool(v: Jval) -> Option[bool] {
    let hit: Option[bool] = None;
    match v {
        JBool(b) => {
            hit = Some(b);
        }
        _ => {
        }
    }
    return hit;
}

/// element count of arrays and objects (0 for scalars)
fn jsize(v: Jval) -> int {
    n := 0;
    match v {
        JArr(items) => {
            n = items.len();
        }
        JObj(m) => {
            keys := m.keys();
            n = keys.len();
        }
        _ => {
        }
    }
    return n;
}

// ---------------------------------------------------------------------------
// encoder — compact output, strings escaped, floats round-trip-exact
// ---------------------------------------------------------------------------

/// growable byte buffer for encoding (arena-backed via Vec[u8])
struct Jb {
    v: Vec[u8]
}

impl Jb {

    fn put(self: *Jb, txt: string) {
        for k in 0..std::strlen(txt) {
            self.v.push(txt[k]);
        }
    }

    fn byte(self: *Jb, b: u8) {
        self.v.push(b);
    }

    fn done(self: *Jb) -> string {
        out := new(u8, self.v.len() + 1);
        for k in 0..self.v.len() {
            out[k] = self.v.get(k);
        }
        out[self.v.len()] = 0;
        return out as string;
    }
}

fn jhex(n: u8) -> u8 {
    if n < 10 {
        return 48 + n; // '0'
    }
    return 87 + n; // 'a' - 10
}

fn jb_str(b: *Jb, s: string) {
    b.byte(34); // '"'
    for k in 0..std::strlen(s) {
        ch := s[k];
        if ch == 34 {
            b.put("\\\"");
        } else if ch == 92 {
            b.put("\\\\");
        } else if ch == 8 {
            b.put("\\b");
        } else if ch == 9 {
            b.put("\\t");
        } else if ch == 10 {
            b.put("\\n");
        } else if ch == 12 {
            b.put("\\f");
        } else if ch == 13 {
            b.put("\\r");
        } else if ch < 32 {
            b.put("\\u00");
            b.byte(jhex(ch >> 4));
            b.byte(jhex(ch & 15));
        } else {            b.byte(ch); // UTF-8 bytes pass through verbatim
        }
    }
    b.byte(34);
}

/// exact powers of ten (1e22 is the last exactly-representable power)
fn p10(n: int) -> float {
    t := [1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0, 1000000.0, 10000000.0, 100000000.0, 1000000000.0, 10000000000.0, 100000000000.0, 1000000000000.0, 10000000000000.0, 100000000000000.0, 1000000000000000.0, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22];
    return t[n];
}

/// a * 10^p via exact table chunks (one rounding per chunk keeps the
/// estimate close; the binary search below guarantees the final digits)
fn scale_pow10(a: float, p: int) -> float {
    s := a;
    pp := p;
    while pp >= 22 {
        s = s * 1e22;
        pp -= 22;
    }
    while pp <= -22 {
        s = s / 1e22;
        pp += 22;
    }
    if pp > 0 {
        s = s * p10(pp);
    }
    if pp < 0 {
        s = s / p10(-pp);
    }
    return s;
}

/// render a 17-significant-digit integer at decimal exponent -p (byte
/// buffer, not concat: exponents build 300+-character strings)
fn jrender(mi: int, p: int, neg: bool) -> string {
    b := Jb { v: vec_new[u8]() };
    if neg {
        b.byte(45);
    }
    digits := int_to_str(mi);
    if p <= 0 {
        // integer rendering: digits followed by -p zeros (no stripping —
        // those zeros are significant)
        b.put(digits);
        for _k in 0..-p {
            b.byte(48); // '0'
        }
        return b.done();
    }
    if p > 16 {
        // 0.000…digits
        b.put("0.");
        for _k in 0..p - 17 {
            b.byte(48);
        }
        b.put(digits);
    } else {
        // point sits after the (17 - p)-th digit
        for k in 0..17 {
            b.byte(digits[k]);
            if k == 16 - p {
                b.byte(46); // '.'
            }
        }
    }
    // strip trailing '0's (and a dangling '.') — value-preserving
    while b.v.len() > 0 {
        last := b.v.get(b.v.len() - 1);
        if last == 48 {
            b.v.pop();
        } else if last == 46 {
            b.v.pop();
            break;
        } else {            break;
        }
    }
    return b.done();
}

fn jparse_eq(s: string, x: float) -> bool {
    v := 0.0;
    return std::parse_float(s, &v) && v == x;
}

fn jparse_ge(s: string, x: float) -> bool {
    v := 0.0;
    return std::parse_float(s, &v) && v >= x;
}

/// float -> decimal text. Integral values print as integers; other values
/// are rendered at 17 significant digits with the digits chosen by a
/// binary search over the parse-back (strtod is the oracle), so every
/// emitted number parses back to the exact same double — self-verifying
/// by construction, locked by the fuzz test below.
fn jnum_str(x: float) -> string {
    if x != x || x - x != 0.0 {
        return "null"; // nan/inf have no JSON form (JS precedent)
    }
    if x == std::math::floor(x) && x > -9007199254740992.0 && x < 9007199254740992.0 {
        return int_to_str(x as int);
    }
    neg := x < 0.0;
    a := x;
    if neg {
        a = -x;
    }
    e := std::math::floor(std::math::log10(a)) as int;
    p := 16 - e; // a * 10^p lands near [1e16, 1e17)
    scaled := scale_pow10(a, p);
    est := (scaled + 0.5) as int;
    // decade wobble from log10 rounding: renormalize into [1e16, 1e17)
    while est >= 100000000000000000 {
        est = est / 10;
        p -= 1;
    }
    while est < 10000000000000000 {
        est = est * 10;
        p += 1;
    }
    // the estimate is the canonical 17-digit rendering — prefer it when it
    // round-trips (it gives the cleanest output, e.g. 42.5 -> "42.5").
    // All verification happens on the MAGNITUDE: with the sign attached,
    // larger integers parse to smaller (more negative) values and the
    // search predicate would run backwards.
    if jparse_eq(jrender(est, p, false), a) {
        return jrender(est, p, neg);
    }
    // otherwise binary search the first 17-digit integer whose rendering
    // parses back >= a; the previous one either equals a or the estimate
    // was off. Candidates stay within [1e16, 1e17): a shorter integer
    // would render one decade too large and corrupt the predicate.
    lo := est - 400;
    hi := est + 400;
    if lo < 10000000000000000 {
        lo = 10000000000000000;
    }
    if hi > 99999999999999999 {
        hi = 99999999999999999;
    }
    while lo < hi {
        mid := (lo + hi) / 2;
        if jparse_ge(jrender(mid, p, false), a) {
            hi = mid;
        } else {            lo = mid + 1;
        }
    }
    if jparse_eq(jrender(lo, p, false), a) {
        return jrender(lo, p, neg);
    }
    // paranoid fallback (the fuzz test would catch a real miss)
    return jrender(est, p, neg);
}

fn jb_val(b: *Jb, v: Jval) {
    match v {
        JNull => {
            b.put("null");
        }
        JBool(val) => {
            if val {
                b.put("true");
            } else {                b.put("false");
            }
        }
        JNum(x) => {
            b.put(jnum_str(x));
        }
        JStr(s) => {
            jb_str(b, s);
        }
        JArr(items) => {
            b.byte(91); // '['
            for k in 0..items.len() {
                if k > 0 {
                    b.byte(44); // ','
                }
                jb_val(b, items.get(k));
            }
            b.byte(93); // ']'
        }
        JObj(m) => {
            b.byte(123); // '{'
            keys := m.keys();
            for k in 0..keys.len() {
                if k > 0 {
                    b.byte(44);
                }
                jb_str(b, keys.get(k));
                b.byte(58); // ':'
                jb_val(b, m.get(keys.get(k)));
            }
            b.byte(125); // '}'
        }
    }
}

/// encode to compact JSON (object key order follows the HashMap table)
fn jdump(v: Jval) -> string {
    b := Jb { v: vec_new[u8]() };
    jb_val(&b, v);
    return b.done();
}

// ---------------------------------------------------------------------------
// deep equality — the semantic comparison (dump text is not canonical for
// objects: HashMap key order depends on insertion history)
// ---------------------------------------------------------------------------
fn jeq(a: Jval, b: Jval) -> bool {
    match a {
        JNull => {
            return match b {
                JNull => true,
                _ => false,
            };
        }
        JBool(x) => {
            return match b {
                JBool(y) => x == y,
                _ => false,
            };
        }
        JNum(x) => {
            return match b {
                JNum(y) => x == y,
                _ => false,
            };
        }
        JStr(x) => {
            return match b {
                JStr(y) => x == y,
                _ => false,
            };
        }
        JArr(xs) => {
            return jeq_arr(xs, b);
        }
        JObj(m) => {
            return jeq_obj(m, b);
        }
    }
}

fn jeq_arr(xs: *Vec[Jval], b: Jval) -> bool {
    match b {
        JArr(ys) => {
            if xs.len() != ys.len() {
                return false;
            }
            for k in 0..xs.len() {
                if !jeq(xs.get(k), ys.get(k)) {
                    return false;
                }
            }
            return true;
        }
        _ => {
            return false;
        }
    }
}

fn jeq_obj(m: *HashMap[string, Jval], b: Jval) -> bool {
    match b {
        JObj(m2) => {
            keys := m.keys();
            keys2 := m2.keys();
            if keys.len() != keys2.len() {
                return false;
            }
            for k in 0..keys.len() {
                key := keys.get(k);
                if !m2.has(key) {
                    return false;
                }
                if !jeq(m.get(key), m2.get(key)) {
                    return false;
                }
            }
            return true;
        }
        _ => {
            return false;
        }
    }
}

// ---------------------------------------------------------------------------
// demo + benchmark
// ---------------------------------------------------------------------------
fn show(label: string, v: Result[Jval, string]) {
    match v {
        Ok(val) => {
            std::println_str(label + " -> " + jdump(val));
        }
        Err(e) => {
            std::println_str(label + " -> ERROR " + e);
        }
    }
}

fn strmul(s: string, n: int) -> string {
    out := "";
    for _k in 0..n {
        out = out + s;
    }
    return out;
}

fn demo() -> int {
    sample := "{\"name\": \"Wlel\", \"year\": 2026, \"flags\": [true, false, null], " + "\"pi\": 3.141592653589793, \"msg\": \"arena\\nfirst\", " + "\"deps\": [{\"name\": \"json\", \"ok\": true}]}";
    v := jparse(sample);
    match v {
        Err(e) => {
            std::println_str("demo parse failed: " + e);
            return 1;
        }
        Ok(doc) => {
            name := jstr(jget(doc, "name").unwrap_or(JNull)).unwrap_or("?");
            year := jnum(jget(doc, "year").unwrap_or(JNull)).unwrap_or(0.0);
            std::println_str("name  = " + name);
            std::println_str("year  = " + int_to_str(year as int));
            flags := jget(doc, "flags").unwrap_or(JNull);
            std::println_str("flags = " + int_to_str(jsize(flags)) + " items");
            pi := jnum(jget(doc, "pi").unwrap_or(JNull)).unwrap_or(0.0);
            std::println_str("pi    = " + jnum_str(pi));
            again := jparse(jdump(doc));
            same := jeq(again.unwrap_or(JNull), doc);
            std::println_str("round-trip identical: " + std::format("{}", same));
            dep0 := jat(jget(doc, "deps").unwrap_or(JNull), 0).unwrap_or(JNull);
            std::println_str("dep0  = " + jstr(jget(dep0, "name").unwrap_or(JNull)).unwrap_or("?"));
        }
    }
    show("bad trailing ", jparse("{\"a\": 1} x"));
    show("bad literal   ", jparse("[tru]"));
    show("bad escape    ", jparse("\"\\q\""));
    show("unterminated  ", jparse("\"abc"));
    show("deep nesting  ", jparse(strmul("[", 300) + strmul("]", 300)));
    return 0;
}

// --- benchmark corpus writer -------------------------------------------------
struct Jw {
    b: *u8,
    n: int
}

impl Jw {

    fn s(self: *Jw, txt: string) {
        n := std::strlen(txt);
        for k in 0..n {
            self.b[self.n + k] = txt[k];
        }
        self.n += n;
    }

    fn finish(self: *Jw) -> string {
        self.b[self.n] = 0;
        return self.b as string;
    }
}

/// realistic corpus: user records with numbers, strings, arrays, objects
fn gen_corpus(items: int) -> string {
    buf := new(u8, items * 320 + 1024);
    w := Jw { b: buf, n: 0 };
    w.s("[");
    for i in 0..items {
        if i > 0 {
            w.s(",");
        }
        w.s("{\"id\":");
        w.s(int_to_str(i));
        w.s(",\"name\":\"user_");
        w.s(int_to_str(i));
        w.s("\",\"email\":\"u");
        w.s(int_to_str(i));
        w.s("@example.com\",\"score\":");
        w.s(jnum_str((i % 1000) as float / 8.0));
        w.s(",\"active\":");
        if i % 3 == 0 {
            w.s("true");
        } else {            w.s("false");
        }
        w.s(",\"tags\":[\"alpha\",\"beta\",\"gamma\"]," + "\"meta\":{\"karma\":");
        w.s(int_to_str(i % 97));
        w.s(",\"bio\":\"line\\nwith \\\"escapes\\\"\"}," + "\"friends\":[1,2,3,4,5]}");
    }
    w.s("]");
    return w.finish();
}

fn bench(megs: int, min_mbs: float) -> int {
    std::print_str(std::format("json bench: generating {} MB corpus ... ", megs));
    total := megs * 1000000;
    // one arena for the whole run: generation + parse churn stays bounded
    // and frees in O(1) at block exit (the root arena would keep every
    // Vec-doubling retiree alive until exit)
    arena(total * 8 + (1 << 20)) {
        s := gen_corpus(total / 260); // ~260 bytes per record
        std::println_str("done");
        t0 := sys::mono_ms();
        v := jparse(s);
        t1 := sys::mono_ms();
        match v {
            Err(e) => {
                std::println_str("bench parse FAILED: " + e);
                return 1;
            }
            Ok(_) => {
            }
        }
        dt := t1 - t0;
        if dt <= 0 {
            dt = 1;
        }
        mbs := std::strlen(s) as float / 1000000.0 / (dt as float / 1000.0);
        std::println_str(std::format("parsed {} bytes in {} ms — {} MB/s", std::strlen(s), dt, mbs));
        if mbs < min_mbs {
            std::println_str(std::format("BENCH FAIL: below the {} MB/s floor", min_mbs));
            return 1;
        }
        st := arena_stats();
        std::println_str(std::format("arena: {} bytes in {} chunks, peak {}", st.bytes, st.chunks, st.peak));
        std::println_str("bench ok");
    }
    return 0;
}

fn main() -> int {
    if sys::argc() >= 2 && sys::arg(1) == "bench" {
        megs := 64;
        min := 100.0;
        if sys::argc() >= 3 {
            n := 0;
            if str_parse_int(sys::arg(2), &n) {
                megs = n;
            }
        }
        if sys::argc() >= 4 {
            f := 0.0;
            if std::parse_float(sys::arg(3), &f) {
                min = f;
            }
        }
        return bench(megs, min);
    }
    return demo();
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------
test "scalars" {
    assert(jparse("null").is_ok());
    match jparse("true") {
        Ok(v) => {
            assert_eq(jbool(v).unwrap_or(false), true);
        }
        Err(_) => {
            assert(false);
        }
    }
    match jparse("-12.5") {
        Ok(v) => {
            assert_eq(jnum(v).unwrap_or(0.0), -12.5);
        }
        Err(_) => {
            assert(false);
        }
    }
    match jparse("1e3") {
        Ok(v) => {
            assert_eq(jnum(v).unwrap_or(0.0), 1000.0);
        }
        Err(_) => {
            assert(false);
        }
    }
    match jparse("\"hi\"") {
        Ok(v) => {
            assert_eq(jstr(v).unwrap_or(""), "hi");
        }
        Err(_) => {
            assert(false);
        }
    }
}

test "containers and accessors" {
    doc := jparse("{\"a\": [10, 20, {\"b\": \"x\"}], \"n\": null}").unwrap_or(JNull);
    arr := jget(doc, "a").unwrap_or(JNull);
    assert_eq(jsize(arr), 3);
    assert_eq(jnum(jat(arr, 1).unwrap_or(JNull)).unwrap_or(0.0), 20.0);
    inner := jat(arr, 2).unwrap_or(JNull);
    assert_eq(jstr(jget(inner, "b").unwrap_or(JNull)).unwrap_or(""), "x");
    assert(jget(doc, "n").is_some()); // key exists, value is null
    assert(jnum(jget(doc, "n").unwrap_or(JNull)).is_none());
    assert_eq(jsize(doc), 2);
    assert(jat(arr, 99).is_none());
    assert(jget(inner, "zzz").is_none());
}

test "escapes and unicode" {
    s := jparse("\"a\\n\\t\\\"\\\\\\/\\u0041\"").unwrap_or(JNull);
    assert_eq(jstr(s).unwrap_or(""), "a\n\t\"\\/A");
    // surrogate pair: U+1F600 grinning face -> 4-byte UTF-8
    e := jparse("\"\\uD83D\\uDE00\"").unwrap_or(JNull);
    got := jstr(e).unwrap_or("");
    assert_eq(std::strlen(got), 4);
    assert_eq(got[0] as int, 240);
    assert_eq(got[1] as int, 159);
    assert_eq(got[2] as int, 152);
    assert_eq(got[3] as int, 128);
    // snowman directly in UTF-8 survives verbatim
    sn := jparse("\"☃\"").unwrap_or(JNull);
    assert_eq(std::strlen(jstr(sn).unwrap_or("")), 3);
}

test "numbers exact via strtod" {
    v := jparse("3.141592653589793").unwrap_or(JNull);
    assert_eq(jnum(v).unwrap_or(0.0), 3.141592653589793);
    big := jparse("9007199254740993").unwrap_or(JNull); // 2^53+1
    assert_eq(jnum(big).unwrap_or(0.0), 9007199254740992.0); // double rounds
    neg := jparse("-0.0").unwrap_or(JNull);
    assert_eq(jnum(neg).unwrap_or(1.0), -0.0);
    assert(jparse("1e999").is_err()); // inf rejected
}

test "errors carry position and strictness" {
    match jparse("{\n  \"a\": tru\n}") {
        Ok(_) => {
            assert(false);
        }
        Err(e) => {
            assert(str_find(e, "line 2") > 0);
            assert(str_find(e, "unexpected character") > 0);
        }
    }
    assert(jparse("{\"a\": 1} trailing").is_err());
    assert(jparse("").is_err());
    assert(jparse("[1,]").is_err());
    assert(jparse("{\"a\"}").is_err());
    assert(jparse("\"\\q\"").is_err());
    assert(jparse("\"\\uD800\"").is_err()); // lone surrogate
    assert(jparse("\"\\uD800\\u0041\"").is_err()); // bad pair
    assert(jparse("01").is_err()); // no leading zeros
    assert(jparse("+1").is_err());
    assert(jparse("1.").is_err());
    assert(jparse(".5").is_err());
    assert(jparse("1e").is_err());
    assert(jparse("1e+").is_err());
    assert(jparse("\"a").is_err());
    // hostile nesting gets a clean error, not a crash
    deep := strmul("[", 300) + strmul("]", 300);
    match jparse(deep) {
        Ok(_) => {
            assert(false);
        }
        Err(e) => {
            assert(str_find(e, "nesting too deep") > 0);
        }
    }
    assert(jparse(strmul("[", 200) + strmul("]", 200)).is_ok());
}

test "jdump round-trips strings and escaping" {
    match jparse("\"q\\\"\\\\\\n\\u0001\"") {
        Ok(v) => {
            d := jdump(v);
            match jparse(d) {
                Ok(v2) => {
                    assert_eq(jstr(v2).unwrap_or(""), jstr(v).unwrap_or(""));
                }
                Err(_) => {
                    assert(false);
                }
            }
        }
        Err(_) => {
            assert(false);
        }
    }
    assert_eq(jdump(JNull), "null");
    match jparse("true") {
        Ok(v) => {
            assert_eq(jdump(v), "true");
        }
        Err(_) => {
            assert(false);
        }
    }
}

test "jnum_str formats exactly" {
    assert_eq(jnum_str(0.0), "0");
    assert_eq(jnum_str(-3.0), "-3");
    assert_eq(jnum_str(42.5), "42.5");
    assert_eq(jnum_str(0.5), "0.5");
    assert_eq(jnum_str(255.0), "255");
    assert_eq(jnum_str(1e21), "1000000000000000000000");
    // non-integral values: the exact string may use the full 17 digits,
    // so assert the round-trip instead of a canonical form
    for v in [3.141592653589793, 0.1, -2.718281828459045, 1e-300, 6.02e23] {
        back := 0.0;
        assert(std::parse_float(jnum_str(v), &back));
        assert_eq(back, v);
    }
}

/// reinterpret a u64 as float bits (byte-copy through a one-element array —
/// the honest way without float<->int pointer casts)
fn float_from_bits(bits: u64) -> float {
    b := new(u8, 8);
    for k in 0..8 {
        b[k] = (bits >> k * 8 & 255) as u8;
    }
    src := b as *float;
    return *src;
}

test "float round-trip fuzz (seeded, 5000 doubles)" {
    std::random::seed(12345);
    checked := 0;
    for _k in 0..5000 {
        bits := std::random::next();
        // one arena per sample: renders allocate, and nothing here may
        // outlive the number under test
        arena(1 << 20) {
            x := float_from_bits(bits);
            if x == x && x - x == 0.0 {
                // finite: not nan, not inf
                s := jnum_str(x);
                back := 0.0;
                assert(std::parse_float(s, &back)); // dump must be parseable
                assert_eq(back, x); // self-verifying encoder must round-trip
                checked += 1;
            }
        }
    }
    assert(checked > 4000); // almost all bit patterns are finite
}

test "jeq deep equality" {
    a := jparse("{\"x\": [1, {\"y\": \"z\"}], \"n\": null}").unwrap_or(JNull);
    b := jparse(" {\"n\":null, \"x\":[1,{\"y\":\"z\"}]} ").unwrap_or(JNull);
    assert(jeq(a, b)); // key order differs, values equal
    c := jparse("{\"x\": [1, 2]}").unwrap_or(JNull);
    assert(!jeq(a, c));
    assert(jeq(jparse("[1, 2]").unwrap_or(JNull), jparse("[1,2]").unwrap_or(JNull)));
    assert(!jeq(jparse("[1, 2]").unwrap_or(JNull), jparse("[2, 1]").unwrap_or(JNull)));
    assert(!jeq(jparse("1").unwrap_or(JNull), jparse("\"1\"").unwrap_or(JNull)));
}

test "file parse" {
    p := "wlel_json_test.tmp";
    assert(std::fs::write_all(p, "{\"x\": [1, 2, 3]}").is_ok());
    v := jparse_file(p);
    assert(v.is_ok());
    arr := jget(v.unwrap_or(JNull), "x").unwrap_or(JNull);
    assert_eq(jsize(arr), 3);
    assert(jparse_file("/no/such/wlel-json-file").is_err());
}
