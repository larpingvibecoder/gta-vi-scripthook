# Runtimes (optional)

gta-vi-scripthook does not need any runtime: `scripthook.dll` does its work itself inside the
process it is loaded into, and `shk-loader` loads it. This page is about two optional
conveniences.

## Loading the hook through a runtime's plugin mechanism

A runtime that loads DLLs from mod folders can load `scripthook.dll` instead of `shk-loader`.
Sonoma Runtime does: copy `dist\gta-vi-scripthook\` into its mods folder (it holds a `mod.toml`
naming `scripthook.dll`). The DLL exports Sonoma Runtime's `srt_plugin_init` /
`srt_plugin_shutdown` only so that the runtime accepts it; they do nothing but unload the plugins
at shutdown. The hook starts from `DllMain`, finds the title the runtime mapped (an executable
image outside the Windows loader's list), its natives table and script tick by pattern, and hooks
the runtime's DXGI `Present` like any other D3D12 presenter. `tests/tests/host.rs`
(`hook_dll_as_a_sonoma_runtime_mod`) checks this on Sonoma Runtime's test title when a Sonoma
Runtime checkout is next to this repository (skipped otherwise).

Notes for runtimes that also have a menu or a script hook of their own:

- Two menus on F4: change one (`menu.toml` `open_key`, or the runtime's setting).
- Two script hooks: give the natives database to one of them only (the hook reads its own
  `natives.tsv` next to the DLL; Sonoma Runtime reads `natives.tsv` from its trainer data).
- Input capture: the hook holds back XInput while its menu is open; input a runtime emulates by
  other means (GameInput) still reaches the game.

## Plugins loaded by Sonoma Runtime directly (adapter)

Plugins built with the SDK also export Sonoma Runtime's plugin API version 2 entry point through
an adapter (`sdk/c/scripthook_srt2.c` for C / C++, built into the Rust SDK), so such a runtime can
load a plugin without the hook. Through the adapter a plugin gets the runtime's memory, hooks,
input, natives and script functions, no drawing (`host->draw` is NULL), input group version 1, and
services emulated in a process-local shared section. The menu then mirrors its pages into the
runtime's menu. `tests/tests/host.rs` checks this path too (optional, like above). With the hook
loaded, use its `plugins\` folder instead.

## If a runtime wants to serve `scripthook.h` itself

Everything a runtime would need to offer plugins natively (instead of loading the hook) is the
table in `sdk/include/scripthook.h`: call `shk_plugin_load(host, self)` with groups built from its
own functions; leave groups it cannot provide NULL. Not required by anything in this repository.
