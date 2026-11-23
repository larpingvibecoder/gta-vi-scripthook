# The natives database

Natives are the game's script commands. `scripthook.dll` calls them through the game's own
handler table, so it needs to know, for the executable you own: where that table is and what it
looks like, the layout of the call context, where the script tick is, and every native's
identifier. None of this is in the repository — it ships the **format** and the **tools**; you
build your database from your own copy of the game.

## `natives.tsv`

Tab-separated, `#` comments, a header row naming the columns, one native per row. Rows starting
with `@` carry a table (an inline TOML table in the next column).

```
identifier	args	returns	name	params	doc
@native_context	{ size = 0xE0, return_ptr = 0x00, arg_count = 0x08, args_ptr = 0x10, data_count = 0x18, fixup_out = 0x20, fixup_in = 0x40 }
@native_table	{ kind = "buckets", buckets = 256, bucket_index = "low_byte", module = "game", pattern = "48 8D 0D ?? ?? ?? ?? E8", rip = { disp = 3, len = 7 } }
@script_tick	{ module = "game", pattern = "40 53 48 83 EC 20 ...", offset = 0 }
0x0123456789ABCDEF	0	Ped	PLAYER::PLAYER_PED_ID	-	-
0x1111111111111111	Player, BOOL	void	PLAYER::SET_PLAYER_INVINCIBLE	player, toggle	-
-	Entity, BOOL	Vector3	ENTITY::GET_ENTITY_COORDS	entity, alive	identifier not found yet
```

(The identifiers above are placeholders.)

| Column | Meaning |
|---|---|
| `identifier` | the game's hash (`0x…`, 64-bit) or index (decimal); `-` = unknown (the native is listed but not callable) |
| `args` | argument types, comma-separated (`int`, `float`, `BOOL`, `Hash`, `const char*`, `Vector3`, `u64`, `ptr`, out parameters `int*`, `float*`, `BOOL*`, `Vector3*`, and handle names such as `Ped`, `Vehicle`, `Entity`, `Player`, `Any` — all `int`), or a slot count |
| `returns` | a type, or `0` (void) / `3` (Vector3) |
| `name` | `NAMESPACE::NAME` (or `NAME`) |
| `params`, `doc` | optional: parameter names and a line of documentation for the generated code |

Other columns are ignored. The same file works for Sonoma Runtime's own script hook.

### Directives

| Directive | Keys |
|---|---|
| `@native_context` | byte offsets `return_ptr`, `arg_count` (u32), `args_ptr`, `data_count` (u32), `fixup_out` (pointers to the caller's vectors), `fixup_in` (16-byte vectors), `fixups` (count, 4), `slot` (bytes per slot, 8), `size` |
| `@native_table` | `kind = "buckets"` (`buckets`, `bucket_index` = `low_byte` / `modulo`, node offsets `next`, `handlers`, `capacity`, `count`, `count_type`, `hashes`, `hash_xor`), `"pairs"` (`stride`, `hash`, `handler`, `count`, `hash_type`) or `"array"` (`stride`, `handler`, `count`), plus a location |
| `@script_tick` | a location: the function the game's script thread calls once per tick |

Locations: `module` (a loaded module's file name, the configured `game_module`, or `"game"`) with
`export`, or `pattern` (hex bytes, `??` wildcards; `section`, `nth`, `rip = { disp, len }` for a
RIP-relative operand), or just `offset`; or `address`; then `offset`; then `chain = [..]` (follow
pointers).

## `shk-natives`

```
shk-natives check <natives.tsv>                         parse and check
shk-natives gen <natives.tsv> --out <dir> [--lang cpp,c,rust]
                                                        natives.h (C++ namespaces), natives_c.h, natives.rs
shk-natives fmt <natives.tsv> [--out <file>]            canonical form
shk-natives import-json <doc.json> --out <natives.tsv>  names, types, parameters from your copy of the
                                                        public documentation's JSON form (identifiers are
                                                        NOT imported: they belong to another build)
shk-natives merge <natives.tsv> <ids.tsv> --out <natives.tsv>
                                                        identifiers you found (name, identifier)
shk-natives verify <natives.tsv> --exe <game.exe>       @native_table / @script_tick against your executable
shk-natives coverage <natives.tsv> trainer/natives-needed.txt
                                                        which of the trainer's 69 natives are known
shk-natives pe-info --exe <file>                        sections of an executable
```

Generated C++:

```cpp
#include "natives.h"
natives::Ped ped = natives::PLAYER::PLAYER_PED_ID();
shk::Vec3 p = natives::ENTITY::GET_ENTITY_COORDS(ped, true);
```

Generated Rust (`mod natives;`): `natives::player::player_ped_id()?`.

## Building your local database

`tools\build-natives-db.ps1` runs the steps; everything it writes goes to `natives\local\`
(git-ignored):

```powershell
tools\build-natives-db.ps1 -Exe "<your game_scarlett_master.exe>" `
    -DocJson <your copy of public native documentation (JSON)> `
    -Ids <identifiers you found: name<TAB>identifier> `
    -Directives <your @native_table / @native_context / @script_tick rows> `
    -Install "<the folder of scripthook.dll>"
```

1. names, types and parameters (optional, from documentation you have);
2. identifiers you found for this executable;
3. the directives you worked out (table location and shape, context layout, script tick);
4. `verify`: each location must match exactly once in your executable;
5. `coverage` of the trainer's natives, `gen` of the headers;
6. install as `natives.tsv` next to `scripthook.dll`.

Finding identifiers and the table in the executable is analysis of your own copy; this
repository does not contain the results and does not ship tools that circumvent protection.
