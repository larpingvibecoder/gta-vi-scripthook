# Building

Windows 10/11 x64. Rust (stable; the repository uses no external crates and downloads nothing).
For the C and C++ parts (examples, tests of the C headers): Visual Studio 2019+ or the Build Tools
with "Desktop development with C++"; `tools\build-examples.cmd` finds it through `vswhere`. A GPU
with D3D12 for the overlay and its tests.

## Rust

```powershell
$env:CARGO_BUILD_JOBS = 6            # optional: leave cores for other work
cargo build --release                # every crate
cargo test --release -- --test-threads=4
```

| Crate | Output |
|---|---|
| `core` (`shk-core`) | `target\release\scripthook.dll` — the hook |
| `core/sys` (`shk-sys`) | Windows FFI used by the hook, the loader and the test target |
| `loader` (`shk-loader`) | `shk-loader.exe` |
| `sdk/rust` (`scripthook-sdk`) | the Rust SDK (library) |
| `natives/gen` (`shk-natives`) | `shk-natives.exe` |
| `menu` (`shk-menu`) | `shk_menu.dll`, `menu-preview.exe` |
| `trainer` (`shk-trainer`) | `shk_trainer.dll` |
| `examples/hello-rust` | `hello_rust.dll` |
| `tools/title` (`shk-title`) | the README title SVGs from Inter |
| `tests` (`shk-tests`) | the test target and the integration tests |

`.cargo/config.toml` links the C runtime statically, so the DLLs need nothing installed.

## C and C++

```bat
tools\build-examples.cmd          :: build\examples\hello_c.dll, hello_cpp.dll
```

or by hand (MSVC; clang-cl takes the same options):

```bat
cl /nologo /O2 /MT /LD /I sdk\include examples\hello-c\hello.c /Fe:hello_c.dll
cl /nologo /O2 /MT /LD /std:c++17 /EHsc /I sdk\include examples\hello-cpp\hello.cpp /Fe:hello_cpp.dll
```

(add `sdk\c\scripthook_srt2.c` to also let a runtime with Sonoma Runtime's older plugin API load
the plugin directly; see host-integration.md.)

## Packaging

```powershell
powershell -File tools\package.ps1 [-NoExamples] [-NoCExamples]
```

gives `dist\gta-vi-scripthook\` — `shk-loader.exe`, `scripthook.dll`, `scripthook.toml`,
`shk-natives.exe`, `mod.toml` (optional), and `plugins\` with the menu, the trainer and the
examples. Add your `natives.tsv`.

## The loader

```
shk-loader --launch <program.exe> [--dll <scripthook.dll>] [--cwd <dir>] [--wait] [-- <arguments>]
shk-loader --pid <id> | --name <process.exe> [--dll <scripthook.dll>]
```

It starts the program (or opens a running process of yours) and calls `LoadLibraryW` on a remote
thread. Use it with your own single-player game only; it does not bypass protection, anti-cheat
or integrity checks, and must not be used with online games or services.

## Tests

| Test | What |
|---|---|
| unit tests (`cargo test`) | the x86-64 decoder and relocator, inline hooks on compiled functions and generated code (RIP-relative loads, branches), the "call before" stub keeping integer and float arguments, fibers, the natives table / context / typed calls / out parameters / vector fix-ups, memory and patterns, the overlay draw list, the TrueType reader and rasterizer, PNG, TOML / JSON, the menu (model, navigation, drawing, the C service), the trainer (statements, lists, features, memory cheats), the natives tool |
| `tests/tests/abi.rs` | `scripthook.h` compiled by MSVC agrees with the Rust SDK on every size and offset; the example database parses; the table and tick of the example are found in Sonoma Runtime's test title (when present) |
| `tests/tests/generated.rs` | generated `natives.rs` compiles; `natives.h` and `natives_c.h` are compiled in `tests/build.rs` |
| `tests/tests/standalone.rs` | the whole hook on the synthetic test target, through `shk-loader`: patterns, natives, fibers on the target's script thread, Present hook and overlay pixels, the menu driven by keys, the trainer, the examples' natives and hotkeys |
| `tests/tests/host.rs` | optional, with a Sonoma Runtime checkout next to this repository: the hook as a Sonoma Runtime mod on its test title, and SDK plugins loaded through the older plugin API |

Environment for tests and automation: `SHK_AUTOMATION` (overlay: `wait`, `key`, `capture`,
`stats`, `log`), `SHK_TRAINER_SCRIPT`, `SHK_INPUT=synthetic:<buttons>,<lx>,<ly>`, `SHK_LOG`,
`SHK_NATIVES`, `SHK_PLUGINS`, `SHK_OVERLAY=0`.
