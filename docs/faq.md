# FAQ

**Is this official?**
No. It is an unofficial fan project, not affiliated with or endorsed by Rockstar Games or Take-Two
Interactive.

**Does it work online?**
No, and it must not be used there. It is for single-player, offline play. Nothing in the hook,
the SDK or the plugins touches online features or services, and nothing bypasses copy protection,
anti-cheat, licence or integrity checks.

**Does the repository contain anything from the game?**
No game files, no data extracted from the game, no screenshots of the game. The natives database,
lists (vehicles, weapons, locations, ...) and memory locations are yours to build from your own
copy; the repository ships formats, tools and placeholders.

**Do I need Sonoma Runtime?**
No. `scripthook.dll` works in any process it is loaded into, with any D3D12 presenter. Sonoma
Runtime (or another runtime that loads DLLs from mod folders) is just one optional way to load it
(docs/host-integration.md).

**Why is everything greyed out in the trainer?**
Until your `natives.tsv` gives identifiers and the table's location, no native is callable. Each
entry says which native it needs. `shk-natives coverage natives.tsv trainer/natives-needed.txt`
shows which are still missing.

**The menu does not appear.**
See `scripthook.log` next to the DLL: the overlay line says whether `Present` was hooked; the
renderer logs why it cannot draw (for example no D3D12 queue seen on the presenting thread).
The menu opens with F4 or RB + D-pad left (`menu.toml`).

**Scripts run "before each present" instead of on the script thread.**
The script tick's location is not given or was not found (`@script_tick` in `natives.tsv`). Natives
then run on the presenting thread, which the game may not expect: give the tick's location.

**Can plugins be written in other languages?**
Anything that can build an x64 DLL with C exports and call function pointers: `scripthook.h` is
plain C.

**Why the Inter font?**
It reads well at small sizes on top of game pictures. It is bundled under the SIL Open Font
License (THIRD_PARTY.md) and rasterized by the repository's own TrueType reader.
