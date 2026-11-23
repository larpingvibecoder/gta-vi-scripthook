# Architecture

```
  shk-loader.exe ──starts / attaches──▶ game process (the game's executable, or a runtime running it)
                                         │
                                         └─ scripthook.dll  (core/)
                                              ├─ mem / locate   modules (loader list + images mapped by others),
                                              │                 sections, exports, byte patterns, pointer chains
                                              ├─ x64 / hook     length decoder, relocation, trampolines, inline hooks
                                              ├─ natives        handler table (pattern), call context, typed calls
                                              ├─ scripts        fibers; ticked by the hook of the script tick
                                              ├─ overlay        hooks of Present / Present1 / ExecuteCommandLists,
                                              │                 D3D12 renderer, Inter glyph atlas
                                              ├─ input          keyboard (foreground window), XInput, capture
                                              ├─ host           the scripthook.h tables served to plugins
                                              └─ plugins\       menu, trainer, your scripts (DLLs)
```

`scripthook.dll` needs nothing from the process it runs in except Windows itself: no runtime API,
no injected helpers. Everything about the game it relies on is data in `natives.tsv`.

## Start-up

1. `DllMain` starts a thread (nothing runs under the loader lock).
2. `scripthook.toml` next to the DLL: the game module's name, the natives files, the plugins
   folder (`core/scripthook.toml` documents every key).
3. `natives.tsv`: natives, identifiers, argument types, and the `@native_table`,
   `@native_context` and `@script_tick` directives.
4. The overlay: a throw-away D3D12 device, queue and swap chain on a hidden window give the
   addresses of `IDXGISwapChain::Present` / `Present1` and `ID3D12CommandQueue::ExecuteCommandLists`
   inside the system DLLs; those functions are hooked (inline, see below) for every swap chain
   and queue of the process.
5. Plugins load from `plugins\` and get the host table.
6. The script tick is looked for (patterns, retried while the game maps its code) and hooked with a
   "call before" stub; until then (or when it is not given) scripts run before each present.

## Finding the game

`locate::module(name)` matches the Windows loader's module list, then images mapped by other
means — every allocation that starts with a valid PE header — named by their export directory,
and the configured game module (`game_module`, or `"game"`): the largest unlisted executable image
(a title a runtime mapped itself), else the process's executable. Locations are
`module` + `export` / `pattern` (`section`, `nth`, `rip = { disp, len }`) / `offset`, or an
absolute `address`, followed by `offset` and a pointer `chain`.

## Inline hooks

`x64.rs` decodes the first instructions of the target (legacy prefixes, REX, VEX, EVEX, the
one-byte, 0F, 0F38 and 0F3A maps, ModRM / SIB / displacements / immediates) until 5 bytes are
covered, and relocates them into a trampoline within ±2 GiB: RIP-relative operands are re-based,
`jmp` / `jcc` / `call` rel8 / rel32 become absolute branches, `loop` / `jrcxz` and branches back
into the moved bytes are refused. The target gets `jmp rel32` to a relay `jmp [rip]` → detour.
Patching happens with every other thread suspended and none of them inside the patched bytes.
The script tick uses a generated stub that saves the argument registers (rcx, rdx, r8, r9, xmm0–3),
calls the scheduler, restores them and continues into the original function.

## Natives

The call context is built at the layout `@native_context` gives (result pointer, argument
count, argument pointer, vector fix-up count, fix-up out pointers, fix-up in vectors, slot size).
The handler is found in the table `@native_table` describes — buckets (index by low byte or
modulo, node layout, XOR on hashes), (hash, handler) pairs, or an array by index — and must lie in
executable memory. Arguments are converted to the declared types (an int for a float, text for a
hash is hashed, a vector fills three float slots); out parameters get slots of the hook and are
copied back after the vector fix-ups. Natives can only be called from script functions.

## Script fibers

Every script tick the ticking thread becomes a fiber (or uses its own when it already is one),
switches to each script whose wake time has come, and gets control back when the script calls
`wait` or returns. A plugin's scripts are removed first when it unloads, after a tick in progress.

## The overlay

Each present: the hook polls input, calls the plugins' frame callbacks, runs the present-time
script fallback, then the draw callbacks (layers HUD, menu, notices) fill a draw list of quads.
The renderer records one command list on the queue the presenting thread last used for direct
command lists: back buffer to render target, one draw (rectangles use a solid texel; glyphs are
anti-aliased Inter coverage from an atlas in an upload buffer read by the pixel shader), back to
present. Shaders are compiled at first use by Windows' `d3dcompiler_47.dll`. Nothing is drawn and
no GPU work is added when the list is empty. `SHK_AUTOMATION` can capture the presented image
with the overlay as PNG.

## Input

Keys of the process's foreground window are polled every frame and turned into key events (with
auto-repeat and typed characters); pads are read through XInput. While a plugin captures input
(an open menu), hotkeys are suspended and the game's XInput reads report neutral pads.

## The ABI (`scripthook.h`, ABI version 1)

Plugins export `shk_plugin_load(const ShkHost*, ShkPlugin*)` (and optionally
`shk_plugin_unload`, `shk_plugin_info`). `ShkHost` is a set of group tables — `core`, `memory`,
`hooks`, `input`, `natives`, `scripts`, `menu`, `draw`, `services` — each starting with
`{ size, version }` and only ever growing at the end; plugins test `SHK_HAS(group, member)` or use
the SDKs, which do. `tests/tests/abi.rs` checks the header (compiled by MSVC) against the Rust
declarations.

## The menu service

The menu plugin publishes `ShkMenuService` as `scripthook.menu`; other plugins find it at their
first frame and add pages and items with an event callback. With the hook (which offers `draw`,
`input->on_key` and `input->capture`) the menu draws itself; with a host that cannot draw for
plugins it mirrors its pages into that host's basic menu.

## The trainer

`trainer/features/*.toml` define 44 features as statements (docs/trainer.md). At the first frame
the trainer asks the natives database about every native each feature calls (through the global
variables it reads), checks lists and memory locations, and picks a backend: natives, else a
memory cheat (`cheats.toml`), else greyed out with the first reason. Toggles run in one script
every tick; actions, values and list picks each get a script of their own.

## Tests without the game

`tests/target` is the repository's own stand-in for a game: a natives table found by pattern (8
chained buckets, XOR-obfuscated hashes), a script thread calling a tick function, a player record,
and a hidden D3D12 window presenting every frame. `tests/tests/standalone.rs` starts it with
`shk-loader`, and checks the natives, the fibers on its script thread, the trainer, the examples'
hotkeys, the menu driven by injected keys, and the overlay's pixels in a captured frame.
