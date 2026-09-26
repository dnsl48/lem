# Lem crossterm 0.29.0 patch

This directory vendors the published crates.io `crossterm` 0.29.0 source,
originally from <https://github.com/crossterm-rs/crossterm>, under the adjacent
MIT `LICENSE`. The crates.io archive SHA-256 is
`d8b9f2e4c67f833b660cdb0a3523065869fb35570177239812ed4c905aeff87b`.
`Cargo.toml.orig` retains the upstream manifest. Registry marker
files, the upstream lockfile and CI metadata are omitted; source, examples and
documentation are otherwise retained.

The Rust workspace selects this copy using `[patch.crates-io]`. Rebase or remove
this patch when upstream provides equivalent behaviour; do not silently replace
it with a registry update.

The focused changes are:

- Export `terminal::query_keyboard_enhancement_flags_with_timeout(Duration)`
  (Windows returns `Ok(None)`, matching upstream's unsupported status).
- Write queries to a writable controlling `/dev/tty` with no stdout fallback.
- Share one deadline across reader-lock acquisition and response waits, hold the
  lock through poll/read, propagate errors, and bound the DA1-response drain.
  Keyboard flags without DA1 still return successfully at the deadline.
- Parse flag replies as decimal integers and reject malformed replies.
- Keep skipped input queued in order on poll errors; bound retries in the Unix
  input source and propagate terminal EOF/read failures instead of looping.
  The default Unix reader opens a separate nonblocking `/dev/tty` descriptor,
  so malformed or partial input cannot cause a blocking drain and stdin flags
  remain untouched.
- Decode legacy C0 bytes `0x1c`–`0x1f` as Ctrl-\, Ctrl-], Ctrl-^ and Ctrl-_,
  leaving enhanced CSI-u Ctrl-4 through Ctrl-7 as digits.

Verification from the parent Rust workspace:

```sh
cargo test --manifest-path vendor/crossterm/Cargo.toml --lib
python3 vendor/crossterm/tests/keyboard_probe_pty.py
```

The PTY fixture builds a small isolated probe in an ignored target directory,
simulates terminal replies, verifies input queue ordering and protocol-stdout
isolation, and never queries the user's active terminal.
