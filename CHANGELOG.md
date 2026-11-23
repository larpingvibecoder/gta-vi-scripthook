# Changelog

## 0.1.0 (unreleased)

- `scripthook.dll`: a self-contained hook. Finds the game module (the loader's list or images
  mapped by a runtime), locates the natives table and the script tick by patterns from
  `natives.tsv`, calls natives with its own call context, runs script fibers from its own hook of
  the script tick (present-time fallback otherwise), hooks `IDXGISwapChain::Present` and draws with
  its own D3D12 renderer and the Inter font, reads the keyboard and XInput, holds back pad input
  while a menu is open, loads plugins from `plugins\` and serves them `scripthook.h`. An x86-64
  length decoder, relocator and inline hooks with trampolines of its own.
- `shk-loader`: starts or attaches to your own single-player game and loads the hook.
- Plugin SDK: `scripthook.h` (ABI 1), `scripthook.hpp`, the Rust crate `scripthook-sdk` (no
  external crates) with TOML / JSON readers, a TrueType reader and rasterizer, a software canvas
  and PNG writer.
- Natives database format and `shk-natives` (check, generate C++ / C / Rust, import, merge,
  verify against an executable, coverage).
- Menu plugin: the menu service, a new look, keyboard and gamepad, filterable lists, text entry.
- Trainer plugin: 44 data-driven features, memory cheats, lists, saved state, automation.
- Examples in C, C++ and Rust; a synthetic test target and end-to-end tests.
- Optional: loading the hook as a Sonoma Runtime mod; SDK plugins loadable through Sonoma
  Runtime's older plugin API (adapter).
