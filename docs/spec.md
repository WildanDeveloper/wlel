# The Wlel Language Specification

**Version 1.0 — draft for public review**

> This document is the normative definition of the Wlel programming language.
> It describes the language as implemented by the reference compiler (`wlel`,
> C99 backend). Where this document and the compiler disagree, that is a bug
> in one of them — please open an issue and cite the section number.
>
> The keywords **MUST**, **MUST NOT**, **SHALL**, **SHOULD** and **MAY** are
> to be read as described in RFC 2119. Behavior labeled *unspecified* or
> *undefined* places no requirement on any implementation.

---

## Table of contents

1. [Scope](#1-scope)
2. [Lexical structure](#2-lexical-structure)
3. [Types](#3-types)
4. [Conversions and literals](#4-conversions-and-literals)
5. [Expressions and evaluation order](#5-expressions-and-evaluation-order)
6. [Statements and control flow](#6-statements-and-control-flow)
7. [Functions, generics and methods](#7-functions-generics-and-methods)
8. [Enums and match](#8-enums-and-match)
9. [Programs and modules](#9-programs-and-modules)
10. [Memory model: arenas, aliasing, lifetime](#10-memory-model)
11. [Arithmetic integrity: overflow and division](#11-arithmetic-integrity)
12. [Safe-debug vs release](#12-safe-debug-vs-release)
13. [Concurrency](#13-concurrency)
14. [Foreign function interface](#14-foreign-function-interface)
15. [Standard library summary](#15-standard-library-summary)
16. [Grammar](#16-grammar)
17. [Open questions for review](#17-open-questions-for-review)

---

## 1. Scope

Wlel is an arena-first systems programming language. A Wlel source file
(extension `.wl`, UTF-8 text) is compiled to C99 and then by a C compiler
(`cc`/`gcc`/`clang`). The C99 backend is the normative semantics; any
alternative backend (e.g. the experimental QBE backend) MUST either agree
with it on the subset it accepts or reject the program.

The language has no garbage collector, no VM, no macros, no exceptions, no
borrow checker, and no operator overloading. Memory is managed by *arenas as
language syntax* (§10); recoverable errors are values (`Result`/`Option`,
§8); unrecoverable states abort via `panic` (§12.3).

## 2. Lexical structure

### 2.1 Source encoding

Source files are UTF-8. Only ASCII characters are significant to the lexer
outside string literals. Lines are terminated by `\n`; `\r\n` is accepted.
There are no block comments.

### 2.2 Comments

- `// text` — a line comment, ignored except as noted below.
- `/// text` — a *doc comment* (a `//` comment whose text begins with exactly
  one additional `/`). Doc comments are consumed by `wlel doc`; they have no
  effect on semantics. `////` is a plain comment.

### 2.3 Keywords

The following 22 words are reserved and MUST NOT be used as identifiers:

```
fn extern struct enum impl match use test as defer arena let return
if else while for in break continue true false
```

`new`, `wlel_sizeof`, and `wlel_arena` are not keywords but parser-level
builtin forms (§5.9). Identifiers match `[A-Za-z_][A-Za-z0-9_]*`.

The identifier prefix `_wlel_` is reserved: a user declaration of a variable
or parameter with this prefix is a compile-time error. Identifiers beginning
with a single `_` (but not `_wlel_`) are ordinary names that opt out of
unused warnings (§9.6).

### 2.4 Integer literals

- Radices: decimal, `0x`/`0X` (hexadecimal), `0b`/`0B` (binary),
  `0o`/`0O` (octal). Digit groups MAY contain `_` separators, which are
  ignored: `1_000_000`, `0xFF`, `0b1010_1010`.
- An unsuffixed integer literal SHALL denote a value representable in
  `i64` (`int`). Out of range: compile-time error.
- An unsuffixed integer literal is *untyped*: it adapts to the context's
  integer width with a range check (§4.2). Standing alone it has type `int`.
- A *suffix* fixes the type by desugaring to a cast: `10u8` is exactly
  `10 as u8`. Suffixes: `i8 i16 i32 i64 u8 u16 u32 u64 usize byte char`
  (and `f32 f64`, which make the literal a float literal). Because the
  suffix is a cast, **no range check is applied**: `300i8` is accepted and
  wraps to `44`, identically to `300 as i8`.
- Radix literals accept the width suffixes listed above (e.g. `0xFFi32`);
  `byte`/`char`/`f32`/`f64` are not accepted after a radix literal, since
  their letters are valid radix digits.

### 2.5 Floating-point literals

A number token is a float literal if and only if it contains a `.` followed
by a digit (`3.14`, `2.0`), or an exponent part (`1e3`, `1.5e-3`, `2E+8`).
Consequently:

- `1e3` is a float literal (value `1000.0`, type `float`).
- `1.` is not a float literal (`1` followed by the `.` token).
- `.5` is not a number token.

Unsuffixed float literals parse as `f64` and are untyped; they adapt to any
float width in context (§4.2). Suffixes `f32`/`f64` fix the width. A float
literal that cannot be parsed as `f64` is a compile-time error.

### 2.6 String literals

A string literal is delimited by `"`. Recognized escapes: `\n \t \r \" \\ \0`.
Any other escape is a compile-time error, as is an unterminated literal.
Multi-byte UTF-8 code points are collected verbatim and validated. A string
literal has type `string` (§3).

There are no character literals (`'c'` does not exist), no raw strings, and
no hex/unicode escapes. A single character is written as a string of length
one, or as a `u8` integer.

### 2.7 Operators and punctuation

```
( ) { } [ ] , . .. : :: ;
=  :=  += -= *= /= %=
+ - * / %  !  ~  & | ^ << >>  && ||
== != < > <= >=  ->  =>  ?
```

There is no `++`, no `--`, no compound bitwise assignment (`&=` etc.), no
ternary operator, and no comma operator.

## 3. Types

### 3.1 Primitive types

| Spelling | Alias for | C type | Notes |
|---|---|---|---|
| `i8` | — | `int8_t` | |
| `i16` | — | `int16_t` | |
| `i32` | — | `int32_t` | |
| `i64` / `int` | — | `long long` | `int` is a first-class alias, not sugar to be removed |
| `u8` / `byte` / `char` | — | `uint8_t` | `byte` and `char` are aliases of `u8` |
| `u16` | — | `uint16_t` | |
| `u32` | — | `uint32_t` | |
| `u64` | — | `uint64_t` | |
| `usize` | — | `size_t` | |
| `f32` | — | `float` | |
| `f64` / `float` | — | `double` | |
| `bool` | — | `_Bool` | |
| `string` | — | `const char *` | immutable, NUL-terminated (§3.5) |
| `void` | — | `void` | only as a return type (§3.6) |

### 3.2 Compound types

- **Struct**: nominal, declared `struct Name { field: T, ... }`. Layout is
  the C layout of an equivalent C struct (fields in declaration order,
  standard C alignment/padding) — this is normative for FFI (§14).
  Struct literals MUST list every field, by name, in declaration order.
  Recursive and mutually-referential structs are legal via pointers.
- **Enum**: a closed tagged union, declared
  `enum Name { Variant(T1, T2), Other }`. C layout:
  `struct { int _tag; union { struct { T1 f0; T2 f1; } Variant; ... } v; }`.
  Payload-less variants occupy no union slot. Enum values compare only by
  destruction via `match` (§8); relational and equality operators on enums
  are compile-time errors. Array-typed payloads are forbidden.
- **Pointer** `*T`: an unrestricted C pointer. `&expr` requires `expr` to be
  an *lvalue* (variable, field, dereference, or index). `*p` dereferences.
  Field access auto-dereferences exactly one pointer level: `p.f` where
  `p: *S` behaves as `(*p).f`. Pointer indexing `p[i]` is allowed and never
  bounds-checked (§12.1). `*void` exists for opaque handles.
- **Array** `[T; N]`: a fixed-size C array, `N ≥ 1`. Arrays are **not
  first-class values**: they cannot be assigned as a whole, returned from
  functions, passed by value, stored as enum payloads, or elements of
  collections. An array used as a function argument decays to `*T` (§7.4).
  A `let` with an array annotation MAY supply fewer elements than `N`; C
  zero-fills the remainder. `std::len(arr)` yields the element count.

### 3.3 Type parameters

`fn`, `struct`, `enum` and `impl` MAY declare type parameters `[T, E, ...]`.
A type parameter MUST NOT shadow a primitive type name or a reserved builtin
struct (`ArenaStats`, `File`, `Thread`, `Mutex`, `Chan`). Every parameter of
a generic function MUST be explicitly typed. Monomorphization is lazy and
per-concrete-argument (§7.5); the mangled C name is `Name__arg1__arg2`.

### 3.4 Reserved names

`ArenaStats`, `File`, `Thread`, `Mutex`, `Chan` are builtin types and cannot
be redeclared. Enum variant names share one global namespace with each other
and with struct and function names.

### 3.5 `string`

`string` is an immutable, NUL-terminated byte string (`const char *`).
- `s[i]` reads byte `i` as `u8`; reading `s[len]` (the NUL terminator) is
  legal, which makes `while s[i] != 0 { ... }` a valid iteration idiom.
- Writing through `s[i] = x` is a compile-time error
  ("strings are immutable").
- Relational operators `< > <= >=` on strings are compile-time errors; use
  `std::str_cmp`. `==` and `!=` compare **by content** (§5.5).
- `+` concatenates and allocates the result in the active arena (§5.5, §10.2).

### 3.6 `void`

`void` is legal only as a function return type (and, implicitly, as the type
of calls to such functions). A `void` value cannot be stored, passed,
returned as data, or used as an element/payload. A `void` function MAY use a
bare `return;` and MUST NOT `return expr;`.

## 4. Conversions and literals

### 4.1 Assignability

A value of type `V` is assignable to a target of type `T` if and only if:

1. `T` and `V` are the same type, **or**
2. `T` is any float width and `V` is any integer width (implicit
   int→float widening — variables included), **or**
3. `V` is an untyped literal that fits `T` (§4.2), **or**
4. `V` is `[T'; N]` and `T` is `*T'` — **argument decay only**: this rule
   applies exclusively when the array is a function-call argument, never to
   `let` initialization, assignment, struct literals, or returns.

Everything else requires an explicit cast (§4.4).

### 4.2 Literal adaptation

An unsuffixed integer literal adapts to the context's expected integer
width after a range check:

| Target width | Accepted range |
|---|---|
| `i8` | −128 .. 127 |
| `i16` | −32768 .. 32767 |
| `i32` | −2147483648 .. 2147483647 |
| `i64` (`int`) | −2^63 .. 2^63−1 |
| `u8`, `u16`, `u32`, `u64`, `usize` | 0 .. 2^N−1 (negatives rejected) |

Out of range: compile-time error ("literal N does not fit in type T").
Unsuffixed float literals adapt to any float width without a range check.

### 4.3 Mixed-width arithmetic

For a binary arithmetic or comparison operator with integer operands:

- Both operands untyped literals → computed at `i64`.
- One literal + one typed → the literal MUST fit the typed operand's width;
  the result has the typed operand's width.
- Both typed, same width → result that width.
- **Both typed, different widths → compile-time error**
  ("mixed int widths: A and B — cast explicitly").
- **Exception — shifts**: the shift amount converts freely to any integer
  width; the result takes the **left** operand's width.

For float operands: int op float widens implicitly; f32↔f64 mixing requires
an explicit cast. When both operands are already float typed, the result
takes the **left** operand's width (`f32 + f64` is typed `f32`). This
asymmetry is intentional for review (§17). `%` on floats is a compile-time
error.

### 4.4 Casts

`expr as T` is legal exactly when it is on this matrix; every other cast is
a compile-time error:

| from | to |
|---|---|
| any numeric (int/float, any width) | any numeric |
| any pointer `*A` | any pointer `*B` |
| any pointer | any integer |
| any integer | any pointer |
| `string` | any pointer |

Casts never range-check: `300 as i8` wraps (§11). Pointer↔integer and
`string`↔pointer casts are the FFI escape hatches; dereferencing a pointer
produced by an invalid cast is undefined behavior.

### 4.5 Wrap-cast canonicalization (< 32-bit)

For arithmetic on widths **narrower than 32 bits** (`i8 u8 i16 u16`), the
checker wraps the result of `+ - * / %`, `& | ^`, `<< >>`, unary `-` and
unary `~` in an explicit cast to the original width. Normative effect:
**narrow-width arithmetic wraps at its own width**, never at C's `int`
promotion width. Widths ≥ 32 wrap per §11 directly.

## 5. Expressions and evaluation order

### 5.1 Precedence and associativity

From tightest to loosest. **Every binary level is left-associative.** Unary
prefix operators (`- ! ~ & *`) bind tighter than every binary operator and
nest rightward (`- -x`).

| Level | Forms |
|---|---|
| postfix | `.field`, `.method(args)`, `[index]`, `?` (try) — chain leftward |
| unary prefix | `- ! ~ & *` |
| cast | `as T` (chains: `a as u8 as u16`) |
| multiplicative | `* / %` |
| additive | `+ -` |
| shift | `<< >>` |
| relational | `== != < > <= >=` |
| bitwise AND | `&` |
| bitwise XOR | `^` |
| bitwise OR | `\|` |
| logical AND | `&&` |
| logical OR | `\|\|` |

Examples: `-a * b` parses as `(-a) * b`; `-x as u8` as `(-x) as u8`;
`a || b && c` as `a || (b && c)`.

### 5.2 Evaluation order (normative summary)

1. **Binary operators and function arguments: order unspecified.** Wlel
   inherits C's sequencing; a program whose behavior depends on the order
   of side effects in `f(a(), b())` or `g() + h()` is non-portable.
2. **`&&` and `||` short-circuit**: the right operand is evaluated only if
   the left did not already decide the result. Safe-mode checks inside a
   short-circuited operand do not run (§12).
3. **Single evaluation guarantees**: the collection of a `for-in`, the
   scrutinee of a `match`, the inner expression of `expr?`, and the bounds
   of a range `for i in a..b` are each evaluated **exactly once**, into a
   compiler temporary, before the consuming construct proceeds.
4. **`return` ordering**: the returned value is fully evaluated **before**
   any `defer` body runs and **before** an enclosing `arena` block frees
   its memory (§10.4). The value may safely be a scalar or a pointer to
   memory that outlives the arena.
5. **Deferred expressions are evaluated when the defer runs, not when
   registered** (§6.7).
6. Assignment `x = rhs`: for `rhs` of the form `expr?` the RHS is evaluated
   into a temporary first; otherwise the statement emits C `lhs op rhs`
   with C's sequencing.

### 5.3 Operands

- `-` requires a number; `!` requires `bool`; `~` requires an integer.
- `&&`/`||` require `bool` operands and yield `bool`.
- Comparisons yield `bool`. Enums, structs and arrays cannot be compared.
- Indexing: arrays and pointers accept an `int` index (`s[i]` for strings
  reads `u8`).

### 5.4 Function calls

`name(args)` or `name[T1, T2](args)` (turbofish, generic functions only).
Resolution order is §7.6. All arguments are passed **by value** (§7.4).

### 5.5 String operators

- `s1 + s2` → new `string` of length `len(s1)+len(s2)`, allocated in the
  active arena (§10.2). Neither operand is modified.
- `s1 == s2` / `s1 != s2` → content comparison (byte-wise, NUL-terminated),
  result `bool`.

### 5.6 Compound assignment

`+= -= *= /= %=` are **statements**, not expressions. The target MUST be a
numeric lvalue for all of them; `%=` additionally requires an integer
target. `s[i] op= x` is rejected (strings are immutable). Semantics of
`x op= v` are exactly `x = x op v` (including §4.3 and §4.5 rules), with
the target evaluated once.

### 5.7 Assignment and lvalues

The target of `=`/compound assignment MUST be an lvalue: a variable, a
field, a dereference, or an index of an array/pointer. Array-type targets
and string-index targets are rejected by the checker; other non-lvalue
targets are rejected by the C compiler (this delegation is intentional and
listed for review in §17).

A plain write (`x = 5;`) does **not** count as a *read* for the
unused-variable warning (§9.6).

### 5.8 The try postfix `expr?`

Legal **only** in statement positions: `let x := expr?;`, `x = expr?;`,
`return expr?;`, or as a bare expression statement. Elsewhere it is a
compile-time error. Semantics (§8.3, §12.3):

- `expr` MUST be a std `Result[T, E]` or `Option[T]`.
- On the success variant, the payload is the value of `expr?`.
- On the error variant, the enclosing function returns that error variant
  **immediately, running deferred code first**; therefore the enclosing
  function MUST itself return the same std enum with the **same error
  type `E`** (exact match).
- `expr?` inside a `defer` block is rejected, as are `return`/`break`/
  `continue` there (§6.7).
- The inner expression is evaluated exactly once.

### 5.9 Allocator builtin forms

- `new(T)` → `*T`; `new(T, count)` → `*T` for `count` elements. `T` MUST
  not be `void` or an array. `count` MUST be an `int`. Arena-aware (§10.2).
- `wlel_sizeof(T)` → `int`, the `sizeof` of `T` in the C backend.
- `wlel_arena()` → `*void`, a handle to the innermost active arena; legal
  only lexically inside an `arena` block (§10.1).

## 6. Statements and control flow

### 6.1 Declarations

- `x := expr;` — declare with inferred type (the type of `expr`; inferring
  from `void` is an error). The optional `let` prefix is equivalent:
  `let x := expr;`.
- `let x: T = expr;` — declare with annotation; `expr` MUST be assignable
  to `T`. For arrays, fewer elements than `N` are permitted (zero-filled).
- Redeclaration of a name in the **same scope** is an error; shadowing in a
  nested scope is legal and the innermost binding wins.
- A declaration initializer MAY be a value-form `match` or `expr?` (§8.3).

### 6.2 Blocks and scope

`{ ... }` introduces a scope. Loop bodies, `if`/`else` bodies, `match` arms,
`defer` blocks and `arena` bodies each push a scope. Statements after a
terminator (`return`, `break`, `continue`) in the same block produce the
`unreachable statement` warning.

### 6.3 if / while

`if cond { } else if cond { } else { }` — braces required; `cond` MUST be
`bool`. `while cond { }` — same condition rule.

### 6.4 Range for

`for i in a..b { }` — the range is **half-open**: `i` takes
`a, a+1, ...` while `i < b`. Both bounds MUST be integers and are evaluated
once before the loop. The loop variable is always typed `int` (i64),
regardless of the bounds' widths, and is scoped to the body.

### 6.5 for-in

`for x in coll { }` accepts:

- an array `[T; N]` — element type `T`;
- any struct with a field `data: *T` (a "Vec-shaped" struct; `std::Vec[T]`
  is one) — element type `T`.

Anything else is a compile-time error. Semantics:

- The collection header/expression is evaluated **once** (snapshot).
  Pushing to a Vec during iteration does not extend the loop; the buffer
  the snapshot points at stays alive in the arena, so element reads remain
  valid.
- The loop variable is a **copy** of each element and is scoped to the body.
- In safe mode each element read is bounds-checked (§12.1).

### 6.6 break / continue

Legal only lexically inside a loop. Executing them runs the defers
registered since loop entry, innermost first (§6.7).

### 6.7 defer

Two forms:

- `defer expr;` — `expr` MUST have type `void` (e.g. a call). Being an
  expression, it syntactically cannot contain `return`/`break`/`continue`.
- `defer { stmts }` — a block. It has **its own defer scope**: a `defer`
  registered inside it runs at the end of that block. `return`, `break`,
  `continue` and `expr?` inside a defer block are compile-time errors.
  Nested defers are legal.

**LIFO at scope exit.** Registered defers run innermost-first when the
enclosing block exits, whether by fall-through, `return` (after the return
value is evaluated — §5.2), `break`/`continue` (defers since loop entry), or
the error path of `expr?`. A deferred expression is evaluated at the moment
it runs, not when registered.

### 6.8 return

`return;` for void functions; `return expr;` otherwise. Every non-void
function MUST provably return on every path. The proof accepts:

- a trailing `return`;
- an `if`/`else` where **both** branches terminate;
- a trailing nested block, `arena` block, or statement-form `match` whose
  arms all terminate;
- a trailing `panic(...)` call (§12.3), which terminates.

Otherwise: compile-time error. `main` may be declared without a return type;
it becomes C `int main` and bare `return;` in it returns 0 (§9.2).

### 6.9 arena block

`arena { ... }` (default capacity 4096 bytes) or `arena(expr) { ... }`
(`expr` MUST be `int`). Semantics are §10.1. Nested arena blocks are legal;
each inner block is an independent arena. `wlel_arena()` inside the block
yields the arena handle.

### 6.10 test blocks

`test "name" { ... }` at top level. The body is checked as a void,
parameter-less function. Test blocks are **ignored** by `wlel run` and
`wlel build` (zero overhead) and executed by `wlel test` (§9.4). `assert`/
`assert_eq`/`panic` inside a test fail that test only (the suite continues);
anywhere else they abort the process.

## 7. Functions, generics and methods

### 7.1 Declaration

```
fn name(p1: T, p2) -> Ret { ... }
```

An untyped parameter defaults to `int`; a missing return type defaults to
`void`. There is no overloading: duplicate function names are errors.
Parameters and returns follow §3 (arrays may not be parameters or returns).

### 7.2 extern functions

`extern fn name(p: T, ...) -> R;` declares a C symbol: prototype only,
terminated by `;`, no body, not generic, not named `main`. The emitted
prototype is non-`static`, so the linker resolves it. Type checking of
calls is exactly as for Wlel functions (widths are strict: C `int` is
Wlel `i32`, not `int`).

### 7.3 Calls and argument passing

All arguments pass **by value** (C values). Arrays cannot be passed by
value; they decay to `*T` at the call site (§4.1). Structs are copied
bit-wise per C semantics. Argument count and per-argument types are checked
at compile time.

### 7.4 Method sugar (`impl`)

```
impl Name { fn m(self, ...) -> R { ... } }
impl Box[T] { fn get(self) -> T { ... } }   // generic impl repeats the params
```

Every method is desugared to a plain function `Name__m` that flows through
the whole pipeline (signature check, codegen, cache) — method calls are
direct C calls, never dynamic dispatch. Rules:

- The first parameter MUST be named `self`. An unannotated `self` defaults
  to the impl type **by value**; `self: *Name` is a pointer method.
- **Receiver adaptation**: a pointer method called on a value receiver
  auto-borrows (`&recv`; receiver must be an lvalue, else compile-time
  error); a value method called on a pointer receiver auto-dereferences.
- A generic impl MUST repeat the type's own type parameters verbatim.
  Methods cannot declare their own type parameters; they monomorphize per
  receiver like generic functions (`Box__get__int`).
- Methods on enums are legal; the body destructures with `match`.
- Duplicate method names on one type are errors. `impl` on an unknown type
  is an error. A free function named `Name__m` colliding with a desugared
  method is an error.

Resolution of `recv.m(args)`: locals first (innermost scope), then the
method table of the receiver's type.

### 7.5 Generics (monomorphization)

Templates are never checked or emitted unless instantiated. Instantiation
happens at call sites / struct literals / enum constructions with concrete
types, driven by (in priority order): explicit type arguments (`id[int](5)`),
inference from argument types, inference from struct-literal field types,
and expected-type hints from annotated `let` and `return`. An parameter that
cannot be inferred is a compile-time error naming the parameter. Generic
functions and impls mangle as `Name__T1__T2` (§3.3).

### 7.6 Name resolution for calls

A call `foo(...)` resolves in this order; the first match wins:

1. enum variant constructor (if `foo` is a known variant name);
2. `sys::*` builtins (always available, no import);
3. `std::*` (requires `use std;` — §9.3);
4. checker-level builtins: `assert`, `assert_eq`, `panic`, `arena_stats`;
5. generic function (explicit type args, then inference);
6. concrete function signature;
7. raw allocator builtins (`wlel_alloc`, `wlel_free`, `wlel_arena_new`,
   `wlel_arena_alloc`, `wlel_arena_free`, `wlel_print_*`);
8. else: "call to undefined function".

Bare identifiers (values, not calls): locals shadow enum variant names.

## 8. Enums and match

### 8.1 Construction

A payload-less variant name is a value (`None`). A payload-ful variant used
as a value is a constructor call requiring exactly the declared payload
arity and types (`Some(5)`, `Circle(1.0, 2.0)`). Generics are inferred from
payloads or from expected-type hints (§7.5).

### 8.2 match

```
match scrutinee {
    Variant(a, b) => expr_or_block,
    Other         => expr_or_block,
    _             => expr_or_block,
}
```

- The scrutinee MUST be an enum type and is evaluated exactly once.
- A pattern binds each payload to a name (scope: the arm). A binding named
  `_` discards. The number of bindings MUST equal the variant's arity.
- **Exhaustiveness is enforced**: every variant MUST be covered exactly once
  by a named pattern or by `_`. `_` MUST be the last arm; an arm after `_`
  is an error; duplicate arms are errors; unknown variant names are errors.
- **Statement form**: arms may be blocks; if every arm terminates (`return`,
  `break`, `continue`, `panic`), the match terminates.
- **Value form**: legal only directly as a `let` initializer or a `return`
  value. All arm expressions MUST share one type; `void` and array results
  are rejected.

### 8.3 `Result`, `Option`, `?`

The std library defines `enum Result[T, E] { Ok(T), Err(E) }` and
`enum Option[T] { Some(T), None }`. The `?` operator (§5.8) propagates the
error variant: `Ok/Some` yields the payload; `Err(e)`/`None` makes the
enclosing function return `Err(e)`/`None` after running defers. The
enclosing function MUST return the same std enum with the same `E`.

## 9. Programs and modules

### 9.1 Files

A `.wl` file contains top-level items in any order: `fn`, `extern fn`,
`struct`, `enum`, `impl`, `test`, `use`. Names live in one flat namespace
after import merging; there are no module qualifiers except the hardcoded
`std::` and `sys::` roots.

### 9.2 Entry point

`fn main()` or `fn main() -> int` is the entry point. Command-line access is
via `sys::argc()` and `sys::arg(i)` (out-of-range → `""`); `sys::exit(code)`
terminates immediately (and still runs `atexit` cleanup, hence root-arena
freeing, §10.1).

### 9.3 Imports

- `use "path.wl";` — imports a file relative to the importing file,
  **recursively**, with diamond deduplication by canonical path. Imported
  items are tagged with their source file. A `use std;` in any imported
  file propagates to the whole import tree.
- `use std;` — splices the embedded standard library ahead of user code; it
  flows through the same checker, so only used instantiations are emitted.
- An import that contributes nothing produces the `import 'x' is never
  used` warning.

### 9.4 Toolchain contract

- `wlel run` — compile (safe-debug, §12) to a cache directory and execute.
- `wlel build [-O0..-O3]` — compile to a binary (release semantics, §12).
  `-sanitize` adds AddressSanitizer/UBSan instrumentation to the *generated
  C* compile. `--emit-c` writes the generated C next to the output.
- `wlel check` — type-check and print warnings, no codegen.
- `wlel test` — compile with safe-debug and a test runner: each `test`
  block runs isolated (a failing `assert`/`panic` fails one test, the suite
  continues); exit code 1 if any test failed.
- `wlel fmt`, `wlel doc`, `wlel lsp`, `wlel new`, `wlel add` — formatter,
  documentation generator, language server, project scaffolding, dependency
  management (see README).
- Project mode: a `wlel.toml` manifest with `[package]` and `[deps]`
  (path and git dependencies, semver caret requirements, `wlel.lock` pin).
  Entry point `src/main.wl`; `src/lib.wl` is library-only.

### 9.5 Diagnostics

One compile reports **all** syntax errors (parser recovery at statement and
top level). Type checking reports the first error with `file:line:col`.
The generated C carries `#line` directives so C-compiler diagnostics point
back to the `.wl` source.

### 9.6 Warnings (non-fatal, sorted by position)

1. `unused variable 'x'` — never read; a pure write does not count as a read.
2. `unused parameter 'p'`.
3. `import 'x' is never used` (including `use std;`).
4. `unreachable statement` — after `return`/`break`/`continue` in a block.

Names beginning with `_` are exempt from 1 and 2.

## 10. Memory model

This section is the core contract of Wlel. It is deliberately small.

### 10.1 Arenas

- An arena is a growable chain of chunks; allocation is an O(1) pointer
  bump; freeing the arena frees every chunk in O(1) amortized (per-chain).
- **Root arena**: every thread of execution owns a root arena created at
  thread start (main: capacity 65536 bytes; spawned threads: 64 KiB). The
  main root arena is released by an `atexit` handler, so temporaries that
  outlive any block (e.g. strings built with `+` at top level) are freed
  at process exit. Spawned threads free their root arena at thread exit.
- **Arena blocks**: `arena { }` / `arena(n) { }` create a fresh arena,
  make it active, run the body, then free the whole arena and restore the
  previously active one. Blocks nest arbitrarily; each has its own capacity
  (default 4096 bytes). Allocation beyond the capacity chains a new chunk
  (capacity `max(n, block capacity)`), so programs never fail on arena
  exhaustion.
- **Alignment**: every arena allocation is aligned to **8 bytes**.
- Arena state (current arena, root arena) is **thread-local**. A thread
  never allocates from another thread's arena.

### 10.2 What allocates where

- `new(T)` / `new(T, n)` allocate in the **active arena** (innermost open
  block; the root arena if no block is open).
- Every string-producing operation (`+` concat, `std::format`,
  `int_to_str`, `float_to_str`, `read_file`, `fs::read_all`, `str_sub`,
  `str_trim`, `map_keys`, ...) allocates its result in the active arena.
- `std::Vec`/`HashMap` buffers bump-allocate in the active arena; growing a
  Vec abandons the old buffer to the arena (freed with it).
- Raw escapes: `wlel_alloc(n) -> *void` (malloc) and `wlel_free(p)` are
  plain heap allocation for FFI-shaped needs; they are not arena-managed.

### 10.3 Aliasing

**Wlel has no borrow checker and no aliasing restrictions.** Any number of
aliases to the same memory may exist and mutate concurrently in the same
thread; the checker imposes no discipline beyond types.

- Within one thread, mutable aliasing is ordinary C aliasing.
- Across threads (§13): sharing a single non-thread-safe object (e.g. a
  `Vec`) between threads without a mutex is a **data race — undefined
  behavior**, exactly as in C. This is user discipline by design and is
  documented, not hidden: use `sys::mutex_*`, or communicate ownership
  through channels.
- Channels transfer **by value** (copy semantics), which is the sanctioned
  way to move data between threads without locks.

### 10.4 Lifetime and escape

- Nothing statically prevents a pointer from escaping an `arena` block:
  assigning it to an outer variable, storing it in a struct, or returning
  it are all legal.
- **A pointer into an arena that has been freed is dangling.** Reading or
  writing through it is undefined behavior. This is the single lifetime
  rule of the language: *a value borrowed from an arena dies with the
  arena.*
- `return` inside an arena evaluates the return value first (§5.2), so
  scalars and pointers to memory that outlives the arena escape safely;
  memory allocated *in* the arena does not.
- **Safe-debug poison**: in safe mode, freed arena chunks are filled with
  `0xDE` before being released, so use-after-free reads deterministic
  `0xDEDE...` garbage unless the allocator reuses the memory first. Release
  mode does not poison. The poison is a debugging aid, not a guarantee.
- `arena_stats() -> ArenaStats` reports `{ bytes, chunks, peak }` for the
  active arena (the root arena when no block is open): bytes used across
  the chunk chain, number of chunks, and the high-water mark.

## 11. Arithmetic integrity

**Arithmetic never traps and is never undefined for in-range operands.**

1. **Unsigned** widths wrap modulo 2^N (C unsigned semantics).
2. **Signed** widths wrap two's-complement: the backend compiles all
   generated C with `-fwrapv`, so signed overflow is *defined* as wrap, in
   every build mode.
3. Widths < 32 bits wrap at their own width (§4.5), not at `int`
   promotion width.
4. **Integer division by zero**: in release mode it inherits C semantics
   (undefined); in safe mode it aborts with a `file:line` message (§12.1).
5. **Float** arithmetic is IEEE-754: division by zero yields `inf`/`NaN` in
   **both** modes; float `%` does not exist (§4.3).
6. `std::checked_add(a, b, &out) -> bool` (and `sub`, `mul`) computes
   overflow-checked integer math; on overflow it returns `false` **and
   still stores the wrapped result** in `*out`.

## 12. Safe-debug vs release

| Check | `wlel run` / `wlel test` (safe) | `wlel build` (release) |
|---|---|---|
| Array bounds (`[T; N]` indexing, for-in reads) | abort, `file:line: index N, length M` | absent (zero overhead) |
| Pointer indexing (`new(T,n)` buffers, decayed arrays) | **not checked** (pointers carry no length) | absent |
| Integer `/` and `%` by zero | abort, `file:line` | C semantics (undefined) |
| Float division by zero | IEEE `inf` (never checked) | IEEE `inf` |
| Arena free | poison-fill `0xDE` | plain free |

Failure paths print to stderr and exit with code 1. Helper code is emitted
only when used; a release binary contains none of it. Short-circuit
operators skip the checks of the unevaluated operand (§5.2).

### 12.1 assert / assert_eq / panic

- `assert(cond)` — aborts (or fails the enclosing test) with
  `assertion failed at file:line` when `cond` is false.
- `assert_eq(a, b)` — mirrors `==` exactly: literal adaptation, the
  mixed-width rule, string content comparison, bool; f32-vs-f64 is
  rejected; other type pairs are rejected.
- `panic(msg: string)` — aborts with `panic at file:line: msg`; counted as
  a terminator for the return-path proof (§6.8); statements after it in the
  same block are unreachable.
- Inside a spawned thread these **abort the process** (a thread never
  unwinds into another thread's stack).

## 13. Concurrency

Available without `use std;` via `sys::*`.

- `sys::thread(work, data) -> *Thread` spawns a thread running the function
  named `work` (functions are not values: the worker is resolved **by
  name**, must be concrete, take exactly one parameter, and return `void`;
  `data` must be assignable to that parameter). The argument is boxed (copied)
  before spawn. `sys::join(t)` blocks until the thread ends and frees the
  handle (join once).
- Each thread owns its **root arena** (§10.1); arena state is thread-local.
- `sys::mutex_new/lock/unlock/free` — a dynamic mutex. Lock discipline,
  deadlock avoidance, and which data a mutex guards are **user
  responsibility**; there is no compiler check. Forgetting `unlock` on some
  path is a bug Wlel does not catch.
- `sys::chan_new[T]()` — an unbounded FIFO channel of `T` (`T` not
  `void`/array; struct/enum elements move by copy). `send(ch, v) -> bool`
  never blocks; `false` after `close` (value dropped). `recv(ch, &out) ->
  bool` blocks until a value is available; `false` means the channel is
  closed **and drained** (`*out` untouched). `close` wakes waiting
  receivers; `free` discards queued values. FIFO order per sender is
  preserved.
- `sys::sleep_ms(ms)`.
- Channel send/recv and mutex lock/unlock establish happens-before edges
  (they are pthread-backed), so handoff through a channel or lock is a
  memory barrier for ordinary data.
- `assert`/`panic` in a spawned thread abort the process (§12.1).

## 14. Foreign function interface

- `extern fn` binds a C symbol (§7.2). Link with `-l lib` / `-L dir`
  (repeatable, passed to `cc`).
- Struct layout is the C layout (§3.2) — verified by the compiler's own
  test suite with `_Static_assert`/`offsetof`.
- A C function's `int` parameter is Wlel `i32`; `long long` is `int`.
  Pointers arrive as `*T`; opaque handles are `*void`; C strings are
  `string` when the C side takes `const char *`.
- Functions declared by the libc headers with the same name (e.g. `div`)
  cannot be redeclared; bind names not already in the generated prelude.
- Memory passed to C outlives the call if allocated in an arena that is
  still open; C code MUST NOT free arena memory.

## 15. Standard library summary

Normative reference: `wlel doc --std` (generated from the embedded source).
Everything below requires `use std;` unless noted. All string-returning and
buffer-allocating functions allocate in the active arena.

**Types**: `Vec[T]` (`*T data, int len, int cap`), `HashMap[K, V]`
(open addressing + tombstones; keys: int/float/bool/string/pointer),
`Result[T, E]`, `Option[T]`.

| Area | API |
|---|---|
| Vec | `vec_new/vec_with_cap`; methods `push pop get set len clear` (get/pop assert in range) |
| HashMap | `map_new`; methods `set get get_or has del keys clear` (get asserts presence; `keys()` order unspecified) |
| Result | `is_ok is_err unwrap unwrap_or ok err` (methods + free fns; `unwrap` panics on Err) |
| Option | `is_some is_none unwrap unwrap_or` |
| Strings | `str_find str_find_from str_sub str_trim str_split str_cmp str_is_space str_parse_int int_to_str` |
| Format | `format(fmt, ...)` — `{}` slots for int/float/bool/string; `parse_float` (full-consume, `&out`), `float_to_str` (`%g`) |
| Print | `print_int println_int print_float println_float print_str println_str` |
| Basics | `streq strlen abs min max len(arr)` |
| Math | 26 libm wrappers (`sqrt pow sin atan2 fmin ...`), all `float` in/out; `f32` args need a cast |
| Random | `random::seed next int float` — xorshift64*, auto-seeded, `seed` makes runs reproducible |
| Checked ops | `checked_add/sub/mul(a, b, &out) -> bool` (§11.6) |
| Sort/search | `sort(coll, cmp)` — in-place unstable quicksort; `binary_search(coll, needle, cmp) -> int` (−1 absent); `cmp(a,b) -> int` is a named concrete function; collections: `[T; N]`, `(ptr, n)`, Vec-shaped |
| File I/O | `fs::open -> Result[*File, string]`, `fs::read_all -> Result[string, string]`, `fs::write_all -> Result[int, string]`, `fs::read/write -> Result[int, string]` (EOF = Ok(0)), `fs::close` (void, idempotent); raw layer `sys::read_file/write_file` (bool + `&out`) |
| Time | `sys::mono_ms()`, `sys::unix_ms()` (no import needed) |

## 16. Grammar

Informal EBNF; `(* ... *)` comments, `...` repetition. Precedence is §5.1,
not encoded here.

```
file        = { item } ;
item        = use | func | extern_func | struct_def | enum_def
            | impl_block | test_def ;
use         = "use" ( "std" | STRING ) ";" ;
func        = "fn" IDENT [ type_params ] "(" [ params ] ")" [ "->" type ] block ;
extern_func = "extern" "fn" IDENT "(" [ params ] ")" "->" type ";" ;
params      = param { "," param } ;   param = IDENT [ ":" type ] ;
type_params = "[" IDENT { "," IDENT } "]" ;
struct_def  = "struct" IDENT [ type_params ] "{" fields "}" ;
fields      = field { "," field } ;   field = IDENT ":" type ;
enum_def    = "enum" IDENT [ type_params ] "{" variants "}" ;
variants    = variant { "," variant } ; variant = IDENT [ "(" types ")" ] ;
impl_block  = "impl" IDENT [ type_params ] "{" { func } "}" ;
test_def    = "test" STRING block ;

block       = "{" { stmt } "}" ;
stmt        = decl | assign | if_stmt | while_stmt | for_stmt
            | return_stmt | break_stmt | continue_stmt
            | defer_stmt | arena_stmt | match_stmt | try_stmt
            | expr_or_call ";" | block ;
decl        = [ "let" ] IDENT ":=" expr ";"
            | "let" IDENT ":" type "=" expr ";" ;
assign      = lvalue ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) expr ";" ;
try_stmt    = expr "?" ";" ;
if_stmt     = "if" expr block [ "else" (if_stmt | block) ] ;
while_stmt  = "while" expr block ;
for_stmt    = "for" IDENT "in" expr ".." expr block        (* range *)
            | "for" IDENT "in" expr block ;                 (* for-in *)
defer_stmt  = "defer" ( expr ";" | block ) ;
arena_stmt  = "arena" [ "(" expr ")" ] block ;
return_stmt = "return" [ expr ] ";" ;

expr        = logic_or ;   (* "?" is a postfix form, statement-checked *)
logic_or    = logic_and { "||" logic_and } ;
logic_and   = bitor { "&&" bitor } ;
bitor       = bitxor { "|" bitxor } ;
bitxor      = bitand { "^" bitand } ;
bitand      = rel { "&" rel } ;
rel         = shift { ( "==" | "!=" | "<" | ">" | "<=" | ">=" ) shift } ;
shift       = add { ( "<<" | ">>" ) add } ;
add         = mul { ( "+" | "-" ) mul } ;
mul         = cast { ( "*" | "/" | "%" ) cast } ;
cast        = unary { "as" type } ;
unary       = ( "-" | "!" | "~" | "&" | "*" ) unary | postfix ;
postfix     = primary { "." IDENT [ call_args ] | "[" expr "]"
                        | "?" } ;
primary     = INT | FLOAT | STRING | "true" | "false"
            | IDENT [ type_args ] [ call_args ]
            | "(" expr ")" | "[" [ elems ] "]"
            | "new" "(" type [ "," expr ] ")"
            | "wlel_sizeof" "(" type ")"
            | "wlel_arena" "(" ")"
            | "match" expr "{" match_arms "}" ;
match_arms  = match_arm { "," match_arm } [ "," ] ;
match_arm   = pattern "=>" ( expr | block ) ;
pattern     = IDENT [ "(" [ IDENT { "," IDENT } ] ")" ] | "_" ;
```

Types: `IDENT` (builtin or user), `*` type, `[` type `;` INT `]`.

## 17. Open questions for review

This is a **draft**. The following points are specified as-implemented and
are explicitly up for review before 1.0 freeze:

1. **Evaluation order of operands and arguments** is unspecified (§5.2.1).
   Defining left-to-right would cost a little codegen freedom; leaving it
   unspecified matches C. Feedback welcome.
2. **Mixed float widths** take the *left* operand's width (`f32 + f64` is
   `f32`, §4.3). An alternative is "f64 wins" (symmetric, lossy-free for
   assignments like `let x: f64 = f32v + f64v`). Current behavior matches
   the shipped compiler; changing it is a one-line checker change plus
   tests.
3. **Assignment lvalue checking** is partly delegated to the C compiler
   (§5.7); a fully checker-side lvalue grammar would improve error
   messages.
4. **Array decay** happens only at call arguments (§4.1.4); explicit
   `arr as *T` is currently rejected. Either generalizing decay or adding
   the cast is under discussion.
5. **`std::print_bool`** is listed by `wlel doc` but not implemented — doc
   and implementation must converge one way or the other.
6. **`std::len`** accepts arrays only (use `.len()` for Vec-shaped
   structs); the doc table previously claimed both.
7. **Pointer arithmetic beyond indexing** (`p + 1`) does not exist; FFI
   code uses casts. Whether to add it pre-1.0 is open (default: no).
8. **Escape analysis** (a static warning when a pointer provably escapes an
   arena block) is listed in the risk register as future work; the spec
   currently defines escape as legal-but-dangling (§10.4).

---

*Wlel spec v1.0-draft — generated against the reference compiler described
in the repository README. Sections §5.2, §10, §11 and §12 (evaluation
order, arena lifetime, overflow, safe-debug) are the load-bearing
contracts; please review those first.*
