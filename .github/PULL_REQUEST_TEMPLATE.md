## What this changes

<!-- One or two sentences. The commit subject is the changelog entry. -->

## Why

<!-- The problem it solves. Link the issue: "Closes #123". -->

## Checklist

- [ ] `cargo test --release --locked` passes
- [ ] `cargo clippy --all-targets -- -D warnings` is clean
- [ ] `wlel fmt -check` passes over `examples/` and `flagship/`
- [ ] A new or changed behavior has a test (a bug fix has a test that fails before it)
- [ ] `docs/spec.md` updated if observable behavior changed
- [ ] `CHANGELOG.md` updated for a user-visible change
- [ ] Generated C stays readable and `#line`-correct (`wlel build --emit-c`)
- [ ] The QBE backend either supports the construct or rejects it cleanly

## Notes for the reviewer

<!-- Trade-offs, follow-ups, things you were unsure about. -->
