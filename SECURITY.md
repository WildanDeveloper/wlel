# Security policy

## Supported versions

Security fixes are shipped for the latest release line. As of 1.0, that is
**1.0.x**.

| Version | Supported |
|---|---|
| 1.0.x | yes |
| < 1.0 | no |

## Reporting a vulnerability

Please **do not open a public issue** for a security problem. Report it
privately through GitHub:

1. Go to <https://github.com/WildanDeveloper/wlel/security/advisories/new>.
2. Describe the impact, the affected component, and a minimal reproduction.
3. If the report involves generated code, include the Wlel source and the
   output of `wlel build app.wlel -o app -O2 --emit-c`.

You will get an acknowledgement within a few days. Once a fix is ready, a
release is cut and the advisory is published with credit (unless you prefer
to stay anonymous).

## What counts as a security issue

Wlel compiles untrusted source, so the compiler is treated as a parser of
hostile input:

- **Compiler crash on input** — a panic, abort, stack overflow, unbounded
  memory use, or a hang triggered by a source file, manifest, or LSP
  message. The compiler must always fail with a diagnostic.
- **Code generation escape** — Wlel source that injects C beyond the
  program's own semantics (for example through an identifier, string
  literal, `#line` path, or manifest field).
- **Cache or temp-file attacks** — writing outside the user's cache
  directory, executing a binary planted by another local user, or
  predictable scratch paths.
- **Dependency resolution** — a manifest that reads, writes, or executes
  outside its project directory, or a git URL that turns into a git option.
- **LSP/server** — a request that crashes the server or escapes the
  workspace.

Bugs in *programs written in Wlel* (for example, an unsafe `extern fn`
declaration or an arena escape) are the program author's responsibility,
as documented in the specification; they are not compiler vulnerabilities.
