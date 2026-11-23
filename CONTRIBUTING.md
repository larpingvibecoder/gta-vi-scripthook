# Contributing

Thanks for helping. A few rules keep the project clean and safe to share.

## Ground rules

- **Single-player / offline only.** No code that touches online features or services, and nothing
  that bypasses copy protection, anti-cheat, licence or integrity checks.
- **No game data.** No game files, no data extracted from the game (natives identifiers, lists,
  offsets, patterns of the real executable), no game screenshots. Formats, tools, placeholders and
  synthetic test data only. Your local database belongs in `natives/local/` (git-ignored).
- **Your own work.** Write code yourself; do not copy from other script hooks, trainers or
  menus, or from projects whose licence is not compatible with MIT OR Apache-2.0.
- **No external crates** and no third-party build systems: the Rust standard library, Windows,
  and MSVC / clang for the C parts.

## Workflow

1. `cargo build --release` and `cargo test --release -- --test-threads=4` (with MSVC installed the
   C header and example tests run too).
2. Keep `sdk/include/scripthook.h` and `sdk/rust/src/ffi.rs` in step (`tests/tests/abi.rs` checks
   them). The ABI only grows at the end of a group; anything else needs a new ABI version.
3. Update the docs (`docs/`) and `CHANGELOG.md` with behaviour changes.
4. Format with `cargo fmt` (line width 200, as in the existing code).

## Licence

Contributions are dual-licensed under MIT OR Apache-2.0, like the project, unless you say
otherwise in the pull request.
