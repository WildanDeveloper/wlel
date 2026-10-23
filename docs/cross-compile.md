# Cross-compiling Wlel

Two separate things can be cross-compiled:

1. **The compiler** (`wlel` itself) — a pure-Rust binary. Build it once per
   host OS/arch and ship it (`scripts/release.sh`).
2. **Wlel programs** — Wlel transpiles to C99, so cross-compiling one means
   pointing it at a C compiler for the target. No Rust toolchain is needed
   for this step.

## 1. Building the compiler for another platform

The compiler has no C dependency at build time and no runtime dependencies
beyond libc:

```bash
rustup target add aarch64-unknown-linux-gnu
scripts/release.sh aarch64-unknown-linux-gnu
# -> dist/wlel-<version>-aarch64-unknown-linux-gnu.tar.gz (+ .sha256)
```

What each target needs:

| target | linker needed |
|---|---|
| `x86_64-unknown-linux-gnu` | host `cc` (native) |
| `aarch64-unknown-linux-gnu` | `gcc-aarch64-linux-gnu` + `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc` |
| `x86_64-unknown-linux-musl` | `musl-gcc` (static binary; `rustup target add x86_64-unknown-linux-musl`) |
| `x86_64-pc-windows-gnu` | `mingw-w64` (`x86_64-w64-mingw32-gcc`) |
| `aarch64-apple-darwin` | build natively on an Apple Silicon runner |
| `x86_64-apple-darwin` | build natively on an Intel runner |

Apple targets must be built on macOS because of the SDK licence. The
`release` workflow does exactly this: one native runner per target, no
cross-OS faking.

## 2. Compiling a Wlel program for another platform

`wlel build` invokes a C compiler — it looks at `$CC`, then `cc`, `gcc`,
`clang` on `PATH`. Set `$CC` to the cross compiler:

```bash
# Linux x86_64 -> Linux aarch64
CC=aarch64-linux-gnu-gcc wlel build app.wl -O2 -o app-arm64

# Linux -> Windows
CC=x86_64-w64-mingw32-gcc wlel build app.wl -O2 -o app.exe

# statically linked Linux binary (musl)
CC=musl-gcc wlel build app.wl -O2 -o app-static
```

`$CC` is used for both the compile and the link, so a wrapper script works
when extra flags are needed:

```bash
cat > cc-aarch64-static.sh <<'EOF'
#!/bin/sh
exec aarch64-linux-gnu-gcc "$@" -static
EOF
chmod +x cc-aarch64-static.sh
CC=$PWD/cc-aarch64-static.sh wlel build app.wl -O2 -o app
```

### The two-step form (full control)

`--emit-c` writes the generated C99 and stops, which is the escape hatch
for any toolchain the one-shot build cannot express:

```bash
wlel build app.wl --emit-c -o app.c
aarch64-linux-gnu-gcc -O2 -std=c99 -fwrapv app.c -o app-arm64 -lm -lpthread
```

The generated C needs `-lm` when `std::math`, `std::random` or
`parse_float` is used, and `-lpthread` when `sys::thread`/`sys::mutex`/
`sys::chan_*` is used. `wlel build` adds both automatically when the
program uses them; a manual link must add them explicitly, **after** the
source/object, or `--as-needed` linkers will drop them.

`-fwrapv` is part of Wlel's signed-overflow contract (§11) — the backend
always passes it, and manual links must too.

### FFI libraries

`-l <lib>` / `-L <dir>` pass straight through to the link, repeatable:

```bash
wlel build app.wl -O2 -o app -L ./vendor/lib -l raylib -l GL
```

### Running a cross-compiled binary

`wlel run` executes the program on the host, so it only works for native
targets. Use `wlel build` for cross targets and copy the binary to the
target machine (or run it under `qemu-aarch64 ./app-arm64`).

`wlel test` also runs locally — tests for cross-compiled programs are best
run on the target, or natively on the same OS/arch.

## Release artifacts

`scripts/release.sh` (no arguments) builds the host target; with arguments
it builds each requested Rust target and writes
`dist/wlel-<version>-<target>.tar.gz` plus a `.sha256`:

```bash
./scripts/release.sh x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
```

The `release` GitHub workflow runs the same script on native runners for
every target when a `v*` tag is pushed and attaches the archives to the
release.
