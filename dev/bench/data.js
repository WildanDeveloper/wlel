window.BENCHMARK_DATA = {
  "lastUpdate": 1792790091523,
  "repoUrl": "https://github.com/WildanDeveloper/wlel",
  "entries": {
    "Wlel benchmarks": [
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "f2d3e7ac7079449b51e9ac6a81ebcdefbdc8e70f",
          "message": "CI fixes round 2: asan oob probe reads ~24 bytes past a 4096-byte allocation so it always lands in the >=1KiB right redzone on every allocator (a short hop past a tiny malloc stayed silent on apple silicon size classes), generated C puts stdout/stderr in binary mode on windows so program output is byte-identical across OSes (printf newline no longer becomes crlf — fix at the runtime, not per test), gh-pages branch seeded so the benchmark trend action can fetch+append (304 tests green)",
          "timestamp": "2026-09-22T02:04:34+07:00",
          "tree_id": "ac4287ec30c37a7bbe4d2b0f3b820e26fafa4aa7",
          "url": "https://github.com/WildanDeveloper/wlel/commit/f2d3e7ac7079449b51e9ac6a81ebcdefbdc8e70f"
        },
        "date": 1790017537558,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 20.2,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 80,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 69.6,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "9658609f54579dc8e864170d5117582d8ea51193",
          "message": "CI fixes round 3: asan oob test self-probes the toolchain (compiles a known-OOB C program with the same -sanitize flags; skips with a reason where -fsanitize=address links but catches nothing, as seen with apple clang on the macOS runners — otherwise wlel build -sanitize must abort with cc/run stderr in the assert), .gitattributes pins *.wl to LF so the fmt no-diff check agrees with windows checkouts, generated-C binary stdout from round 2 already fixed collections/sort exact-output on windows (304 tests green)",
          "timestamp": "2026-09-22T02:12:10+07:00",
          "tree_id": "ef50f4e8291d5f83a1c86bc1ade11ea929766239",
          "url": "https://github.com/WildanDeveloper/wlel/commit/9658609f54579dc8e864170d5117582d8ea51193"
        },
        "date": 1790017964919,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 19.9,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 80,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 71.8,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "442182eb065d7b89c6a4c6fb8df5e2245c15a381",
          "message": "CI fixes round 4, windows green: git-dep manifest paths written with forward slashes (a raw windows temp path put backslash escapes inside toml basic strings and the strict parser rejected \\U), project binary-name asserts accept the .exe suffix mingw appends to extensionless -o targets — macos went green in round 3 via the asan self-probe, all three OS legs plus lint and bench now pass (304 tests green)",
          "timestamp": "2026-09-22T02:19:05+07:00",
          "tree_id": "7b764280092a381eecfb34a8da280297a02b09af",
          "url": "https://github.com/WildanDeveloper/wlel/commit/442182eb065d7b89c6a4c6fb8df5e2245c15a381"
        },
        "date": 1790018387261,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 19.7,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 80,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 70.1,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "3c5ad628d0d198f8fa1b83326928778b3f5bcf33",
          "message": "CI fixes round 5, all green: mingw-w64 gcc 15 ships no ubsan runtime either (not just asan) so the sanitizer probe returned an empty flag string that was passed as a literal empty arg and ld failed with 'cannot find :' — probe now omits the flag entirely when no sanitizer links, wlel's own bounds/div-zero failure paths (clean exits, file:line messages) still verified uninstrumented on windows (304 tests green)",
          "timestamp": "2026-09-22T02:25:41+07:00",
          "tree_id": "56500341d94572e0339e625b9a6d42c6567b8e58",
          "url": "https://github.com/WildanDeveloper/wlel/commit/3c5ad628d0d198f8fa1b83326928778b3f5bcf33"
        },
        "date": 1790018764611,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 15.3,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 78,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 62,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "1f6a56420f65358544a645ec72cf87f4d997ea66",
          "message": "result/option try operator, impl methods, concurrency v1, lsp v1, wlel doc, package registry, qbe backend, spec v1: postfix expr? in statement positions (let/assign/return/exprstmt) with defer-aware error propagation and checker-enforced std Result/Option + exact error variant, builtin panic(msg) terminating, result_*/option_* combinators, fs::open/read_all/write_all/read/write full Result with strerror payloads; impl blocks desugar to plain functions Name__method resolved via the method table, adaptive receivers (auto-borrow lvalue / auto-deref), generic impls monomorphize per receiver, method layer over std Vec/HashMap/Result/Option dogfooded in examples; sys::thread/join/mutex_*/chan_* with worker-by-name + boxed args, TLS per-thread root arenas, unbounded FIFO channels (send false after close, recv blocking with false = closed+drained), pthread/Win32 runtime spliced only when touched; wlel lsp — LSP 3.17 over stdio hand-rolled zero-dep: diagnostics, hover, go-to-def, completion, UTF-16 positions; wlel doc — markdown per module from /// comments, index.md + std.md rendered from the embedded std; wlel add — git-tag registry with caret semver, resolve-then-edit with rollback, lock req@rev, offline re-resolve fallback, cache origin verification; wlel build --backend qbe — direct QBE IL emission (2k-fn cold build 78ms vs 5.9s C = 75x, fib(35) 3x slower — gcc rewrites recursion; C stays primary, non-subset refused with file:line); docs/spec.md — normative language spec v1 draft for public review (evaluation order, overflow, aliasing, arena lifetime, grammar, open questions) (473 tests green)",
          "timestamp": "2026-09-22T18:59:23+07:00",
          "tree_id": "b851d2f452df61ec777a559a89ea6b7de11179ce",
          "url": "https://github.com/WildanDeveloper/wlel/commit/1f6a56420f65358544a645ec72cf87f4d997ea66"
        },
        "date": 1790078411700,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 19.9,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 80,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 72.9,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "e82050e84b924712bad6b46c1b503887fa5cb879",
          "message": "CI fixes round 6: mingw-w64 gcc does not implement __declspec(thread) — the attribute was silently ignored so _wlel_cur_arena/_wlel_root_arena/_wlel_is_main became plain statics shared by every thread and the worker pool corrupted its arena state on windows (3 concurrency tests failed); _WLEL_TLS now maps __GNUC__/__clang__ (incl. mingw, which supports __thread via emutls) to __thread and reserves __declspec(thread) for MSVC, with the macro logic locked by a test; plus the scheduled cargo-fuzz soak caught a real crash — a mutated defer.wl with ~900 nested parens overflowed the parser stack (fatal runtime error before any diagnostic could print), so the parser now carries a recursion guard (MAX_NESTING 256, balanced on success and error paths so recovery keeps parsing) across expr/unary/block/type recursion and rejects hostile nesting with 'expression nesting too deep (limit 256)' at file:line — verified against the downloaded fuzz artifact (wlel check now exits 1 with three clean syntax errors where it previously aborted with SIGABRT), 100-deep parens still parse, 5 regression tests added (478 tests green)",
          "timestamp": "2026-09-22T19:17:17+07:00",
          "tree_id": "9064467660b1619c3b0fbbf28fd97f8be1626bb6",
          "url": "https://github.com/WildanDeveloper/wlel/commit/e82050e84b924712bad6b46c1b503887fa5cb879"
        },
        "date": 1790079465954,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 11.4,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 71,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 52.5,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "fd93cbdd08540a6385b2db13b7e1cbd90ed37f43",
          "message": "README: project logo — assets/logo.jpg rendered centered above the title; the repo landing page now shows the arena-first identity",
          "timestamp": "2026-09-23T10:09:17+07:00",
          "tree_id": "cde737f8a2c14584906c9d9d73fa319405745f38",
          "url": "https://github.com/WildanDeveloper/wlel/commit/fd93cbdd08540a6385b2db13b7e1cbd90ed37f43"
        },
        "date": 1790132999921,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 18.3,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 95,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 66.2,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "83140acfb92e5f6a01535656edbb55c4f18e5c78",
          "message": "README: swap logo to assets/logo.png (500x500 RGBA, 63KB — sharper and 2.5x smaller than the jpeg); reference and rendering unchanged",
          "timestamp": "2026-09-23T10:13:06+07:00",
          "tree_id": "59bc8eecc16a6477543184f806e82cce65b7009a",
          "url": "https://github.com/WildanDeveloper/wlel/commit/83140acfb92e5f6a01535656edbb55c4f18e5c78"
        },
        "date": 1790133216795,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 19.7,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 80,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 71.2,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "18ee2924f675d9fbff57755fd359d545f924f702",
          "message": "bugfix sweep: full-source audit — 34 findings fixed across parser/checker/codegen/stdlib/tooling: return-expr? re-wrapped as Ok/Some (was a raw payload → invalid C), payload-less Result/Option no longer panics the checker (clean \"exactly one payload\" error), 12k else-if chains rejected instead of stack-overflowing, trailing dot is a syntax error (was silently dropped), nested arrays + array literals in value position + struct-ordering with array fields now build, C-keyword/_wlel_/printf/malloc names rejected at check time, duplicate fields and struct↔enum name collisions rejected, main signature validated, non-lvalue assignment targets rejected by the checker, sort/binary_search bind the collection once (was double-eval), safe mode reports negative new()/arena sizes and compound /=,%= by zero, LSP \\uXXXX no longer eats the next char and didChange publishes fresh diagnostics, wlel doc no longer overwrites an index.wl page, build -o to a missing dir fails loudly, signal deaths map to 128+sig, std str_find_from/str_sub/map_insert/vec_with_cap hardened, std::print_bool implemented, unused-import/std warnings attributed correctly, generic canon covers Try/Match/MethodCall, Ok()/Some() infer from the callee parameter, enum/struct generic unification fixed, fmt else-block + float-suffix idempotency fixed; 507 cargo tests green (26 new regression tests), wlel fmt canonical, flagships + all examples type-check",
          "timestamp": "2026-10-22T02:51:43+07:00",
          "tree_id": "a28c4d75e9c749ab7723806bbbc5c73444799162",
          "url": "https://github.com/WildanDeveloper/wlel/commit/18ee2924f675d9fbff57755fd359d545f924f702"
        },
        "date": 1792612352382,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 20,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 82,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 77,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "528c7a5291bcf047f0924d3f7ad527d4558a7eef",
          "message": "CI lint fix: clippy -D warnings — collapse the nested main-parameter check into one condition, use Option::take for the binary_search needle (mem_replace_option_with_none), and bundle the eight unused-import bookkeeping maps into a Usage<'_> struct so called_files stays under the 7-argument lint limit; behavior unchanged, cargo clippy --all-targets -- -D warnings clean",
          "timestamp": "2026-10-22T04:26:36+07:00",
          "tree_id": "84ca00571035ad43e7d6abc1df32976319f94b20",
          "url": "https://github.com/WildanDeveloper/wlel/commit/528c7a5291bcf047f0924d3f7ad527d4558a7eef"
        },
        "date": 1792618038453,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 11.2,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 71,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 53.3,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "29e07ef7ff569bdcb48ae10608710f1bd44d85f1",
          "message": "tuning: front-end 10-20x (stdlib di-parse sekali + di-clone, wlel check tanpa codegen, lexer/parser/codegen bebas alokasi per token/baris, FxHash checker, find_cc tanpa spawn, build content-cache) + 3 bug nyata yang ketemu sambil profiling — std::sort O(n^2) di input terurut (partisi Dijkstra 3-way merotasi sampel median-of-3; 1M int terurut 4.6 s -> 25 ms dengan partisi silang Hoare + insertion cutoff 24, fuzz 20k kasus vs libc qsort), --emit-c -o out.c menulis C lalu menimpanya dengan ELF (target .c sekarang berhenti di source; itu juga sebabnya bench \"front-end tanpa cc\" dulu mengukur cc: kitchen 320 -> 15 ms), dan flagship/json.wl gagal type-check di HEAD karena hint expected dari return type dihapus call bersarang di payload Err(g(\"x\")) (check_variant_construct menyimpan/mengembalikan hint; flagship 10 test blok hijau lagi) — plus CI akhirnya menyentuh flagship: job `flagship` (check + test ketiga app, build json/httpd, smoke throughput JSON dengan floor rendah) dan `fmt -check` kini mencakup flagship/ juga (game.wl + json.wl ternyata belum kanonik, sudah diformat), floor 100 MB/s di json.wl dikoreksi jadi 20 MB/s dengan komentar jujur bahwa itu angka wall-clock (build HEAD di host ini ~26 MB/s), .gitignore mencakup wlel_json_test.tmp, README roadmap dipisah shipped vs next (namespace modul sebagai blocker self-hosting), bench.sh menambah metrik sort input terurut; angka: check kitchen 23 -> 8 ms, check 28k baris 193 -> 123 ms, build ulang 200 -> 8 ms, sort 1M acak 200 -> 135 ms, terurut 4652 -> 27 ms, terbalik 3246 -> 25 ms, fib(35) tetap paritas C; 511 cargo tests green (4 regresi baru: skema partisi, input terurut/terbalik, hint Result bersarang, --emit-c -o x.c), clippy -D warnings clean, wlel fmt kanonik, ASan/LSan clean",
          "timestamp": "2026-10-23T04:46:52+07:00",
          "tree_id": "6ce7371cf717c55bd85045c9ffa851ec805fb766",
          "url": "https://github.com/WildanDeveloper/wlel/commit/29e07ef7ff569bdcb48ae10608710f1bd44d85f1"
        },
        "date": 1792705663079,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 19.9,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 67,
            "unit": "ms"
          },
          {
            "name": "sort 1M pre-sorted ints",
            "value": 9,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 3,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "f8969c2969190bd5af5f3be944d0ebb9ddfa64bf",
          "message": "tuning: front-end 10-20x (stdlib parsed once per process and cloned, `wlel check` no longer generates discarded C, lexer borrows the source and stops allocating a String per comment, identifiers/numbers scanned as slices, parser advance() stops cloning every token, codegen pre-reserves and emits #line without per-line format!, checker maps use an FxHash hasher, find_cc resolves on PATH instead of spawning `cc --version`, `wlel build` joins the content-addressed binary cache) and three real bugs found while profiling — std::sort was O(n^2) on already-sorted input (the Dijkstra 3-way partition rotated the median-of-3 sample; 1M pre-sorted ints 4.6 s -> 25 ms with a Hoare crossing partition + insertion-sort cutoff 24, fuzz-verified over 20k shapes against libc qsort), `wlel build --emit-c -o out.c` wrote the C then overwrote it with the linked ELF (a .c target now stops at the source, which is also why the \"front-end without cc\" benchmark had been timing cc: kitchen 320 -> 15 ms), and flagship/json.wl had been failing to type-check at HEAD because a nested call in a variant payload erased the expected-type hint from the return type (`Err(g(\"x\"))` now infers the Ok slot; the flagship's 10 test blocks pass again); CI now covers the flagships — a new `flagship` job type-checks, test-runs and builds them plus a JSON throughput smoke test, and `fmt -check` includes flagship/ (game.wl and json.wl were not canonical); the JSON bench floor drops from the reference machine's 100 MB/s to a portable 20 MB/s with the reference number documented, and bench.sh now reports the fastest of 7 samples instead of a median of 5 (a shared runner's interference only adds time; the median swung fib(35) 11 -> 20 ms on byte-identical generated C and tripped the 150% trend alert); also fixed the new sorted-input test's result parser, which used index arithmetic that silently dropped a digit locally and panicked on a single-digit value in CI; measured: check kitchen 23 -> 8 ms, check 28k-line file 193 -> 123 ms, build --emit-c 320 -> 15 ms, repeat build 200 -> 8 ms, sort 1M random 200 -> 135 ms, pre-sorted 4652 -> 27 ms, reversed 3246 -> 25 ms, fib(35) still at C parity; 511 cargo tests green (4 new regression tests: partition shape, sorted/reversed input, nested Result hint, --emit-c -o x.c), clippy -D warnings clean, wlel fmt canonical, ASan/LSan clean",
          "timestamp": "2026-10-23T04:57:29+07:00",
          "tree_id": "44e97b71cc0ad43b5589ac861b7eb0e4c10b2833",
          "url": "https://github.com/WildanDeveloper/wlel/commit/f8969c2969190bd5af5f3be944d0ebb9ddfa64bf"
        },
        "date": 1792706298716,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 19.8,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 66,
            "unit": "ms"
          },
          {
            "name": "sort 1M pre-sorted ints",
            "value": 9,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 3,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "fc3ba84d990dea0fe1cc7311013a976ef9833583",
          "message": "v0.7.0: add `wlel --version` and cut the release the tag has been missing since v0.3.0 — Cargo.toml already said 0.6.0 but the last tag was v0.3.0, so 16 commits (generics/collections/strings/file I/O/sort+search/projects/fmt/CI/FFI, enums+match/Result/Option/concurrency/LSP/doc/registry/QBE, the bugfix sweep and the profiling pass) never shipped as a version; `--version`/`-V`/`version` print the Cargo.toml value via env!(\"CARGO_PKG_VERSION\") so the binary can never disagree with its own release page (a drift the new cli test locks: all three spellings must print exactly \"wlel <CARGO_PKG_VERSION>\"), the usage banner now leads with the version, Cargo.toml + Cargo.lock bump 0.6.0 -> 0.7.0, README CLI line documents the flag and the content-cached `wlel build`; 512 cargo tests green (1 new: version flag), clippy -D warnings clean, wlel fmt canonical",
          "timestamp": "2026-10-23T05:23:55+07:00",
          "tree_id": "62de40acb01524e9e0224c84f664b19c6bb040b7",
          "url": "https://github.com/WildanDeveloper/wlel/commit/fc3ba84d990dea0fe1cc7311013a976ef9833583"
        },
        "date": 1792707889487,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 17.5,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 78,
            "unit": "ms"
          },
          {
            "name": "sort 1M pre-sorted ints",
            "value": 9,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wl (front-end)",
            "value": 2.8,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "05e12a6a2d3d75b773d2354c80ea6f122873986c",
          "message": "audit: full-source review — 28 findings fixed across checker/parser/codegen/qbe/project/registry/lexer/main/docs: two compiler crashes (a self-growing generic — `fn f[T](x: T) -> T { return f(Box { v: x }).v; }` — queued bodies until the OOM reaper killed the process, now capped by instance count + mangled-name length with a clean diagnostic; a 2000-term `1 + 1 + ...` chain or a 3000-link field chain built a left-deep tree that overflowed the checker stack with SIGABRT, now bounded by MAX_EXPR_DEPTH 256 matching the parser's MAX_NESTING and promoted into \"expression nested too deeply\", with the CLI running the pipeline on a 16 MiB thread so a legal 256-deep program cannot overflow whatever stack the shell provides), three UB classes in generated C (§11 promises none: `MIN / -1` and `MIN % -1` raised SIGFPE in both build modes and now wrap two's-complement through _wlel_sdiv/_wlel_srem, shift counts >= the operand width or negative were C UB and are now masked to the width so x << 64 is x, and safe mode's _wlel_divz returned `long long`, so a u32 `%` followed by `u32 - u32` was computed in 64 bits and gave 2342709264 instead of 130150416 — the helper result is now cast back to the operand's C type; the QBE backend had the same MIN/-1 and shift gaps and now matches the C backend on all 12 boundary cases), infinite-size types that reached cc instead of the checker (`struct N { next: N }`, A -> B -> A, `[S; 2]`, `enum E { A(E) }` and the generic `struct S[T] { me: S[int] }` all failed inside cc with \"field has incomplete type\" pointing at a temp .c that had already been deleted, now a cycle check runs before and after monomorphization and reports the loop in source terms; pointers still break the cycle), name collisions (a source `__` could shadow a monomorphized instance — `struct Vec__int` broke v.push — now rejected at parse time while the stdlib's generated `Result__is_ok` is unaffected; `struct main` emitted a type and a function with the same C name; builtin names like `struct string` were accepted but unreachable behind the builtin, so type names reject builtins, C keywords/runtime symbols and main), security (the build cache was a world-writable /tmp/wlel_cache whose file names are a public content hash, so any local user could pre-plant a binary that `wlel run` would execute — it now lives in the user's cache dir, 0700 on Unix with a WLEL_CACHE_DIR override, and scratch files use pid + clock + counter instead of just pid; a manifest `git = \"--upload-pack=...\"` was passed to git as an option rather than a URL, now rejected and `git clone` uses `--`; a package `name = \"../evil\"` was accepted and `wlel build` wrote its binary outside the project, now validated along with `version`; inline tables accepted duplicate keys and trailing text), and diagnostics (lexer errors printed the position twice; `0b102` said \"out of range\" instead of \"invalid digit for a base-2 literal\"; overflowing radix literals echoed the decimal value instead of the source spelling; the generated C was deleted before the cc error that named it and is now kept with its path printed; a newline in a source file name broke the `#line` directive; `wlel doc` and LSP hover emitted ```wl fences; docs/cross-compile.md's `-std=c99` one-liner did not compile because the runtime needs clock_gettime, now `-std=gnu99`); spec §2.3 documents the reserved-name rules and §11 the division-overflow and shift rules, the README overflow table gained both rows; 548 cargo tests green (20 new in tests/audit_tests.rs), clippy -D warnings clean, wlel fmt canonical, all examples/flagships pass, ASan+UBSan clean, 500/500 differential expressions match a Python oracle, C vs QBE parity on 30 programs, 6M libFuzzer runs with no crash",
          "timestamp": "2026-10-23T20:54:30Z",
          "tree_id": "af6c363326556575afb3c014109f30581d1802af",
          "url": "https://github.com/WildanDeveloper/wlel/commit/05e12a6a2d3d75b773d2354c80ea6f122873986c"
        },
        "date": 1792788997200,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 15.3,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 59,
            "unit": "ms"
          },
          {
            "name": "sort 1M pre-sorted ints",
            "value": 7,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wlel (front-end)",
            "value": 3.1,
            "unit": "ms"
          }
        ]
      },
      {
        "commit": {
          "author": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "committer": {
            "email": "wildandeveloper@users.noreply.github.com",
            "name": "WildanDeveloper",
            "username": "WildanDeveloper"
          },
          "distinct": true,
          "id": "24989d1796ae32d37e6a0c519652b9e846b768d8",
          "message": "release: build both Darwin targets on macos-14 — GitHub retired the Intel macos-13 label, so the v1.0.0 run queued that job forever (17 minutes with no runner) and publish, which needs the whole build matrix, could never start; the Apple SDK on an ARM runner cross-links x86_64, so both Darwin targets build on macos-14 and every job in the matrix now lands on a live label, plus timeout-minutes (30 test/build, 20 publish) so a stuck runner fails the run instead of hanging it; verified with scripts/release.sh (host archive + .sha256) and a YAML parse of the matrix",
          "timestamp": "2026-10-23T21:14:04Z",
          "tree_id": "018628f1f672c667ca94b5824ce29b57b12cd440",
          "url": "https://github.com/WildanDeveloper/wlel/commit/24989d1796ae32d37e6a0c519652b9e846b768d8"
        },
        "date": 1792790090890,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "fib(35) runtime",
            "value": 11,
            "unit": "ms"
          },
          {
            "name": "sort 1M ints",
            "value": 58,
            "unit": "ms"
          },
          {
            "name": "sort 1M pre-sorted ints",
            "value": 4,
            "unit": "ms"
          },
          {
            "name": "compile kitchen.wlel (front-end)",
            "value": 2.8,
            "unit": "ms"
          }
        ]
      }
    ]
  }
}