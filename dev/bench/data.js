window.BENCHMARK_DATA = {
  "lastUpdate": 1790018765887,
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
      }
    ]
  }
}