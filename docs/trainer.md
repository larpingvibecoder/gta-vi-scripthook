# The trainer

A plugin (`plugins\trainer\shk_trainer.dll`) with 44 features in six groups — **Player** (14),
**Teleport** (4), **Vehicle** (7), **Weapons** (6), **World** (8), **Misc** (5) — plus memory cheats
with no feature of their own (unlimited money, unlimited fuel, gangs friendly, traffic speed).
Every feature is data: `trainer/features/*.toml` (also built into the DLL).

## Availability

At the first frame the trainer asks the hook's natives database about every native a feature
calls (including through the global variables it reads), checks the lists and memory locations it
uses, and picks a backend:

1. **natives** — every native is callable;
2. **memory cheat** — the cheat with the feature's id (or `cheat = "<id>"`) has a known location;
3. **none** — greyed out with the first reason, e.g. *needs native PLAYER::SET_PLAYER_INVINCIBLE
   (identifier unknown until the executable); memory cheat: player health: location unknown until
   the executable is available*.

The native names are the engine lineage's public names, used as candidates: they are unverified
for this game until your database confirms them (`trainer/natives-needed.txt` lists all 69).

## Features

```toml
[vars]                                   # global variables, evaluated at first use each tick
player = "PLAYER::PLAYER_ID()"
ped = "PLAYER::PLAYER_PED_ID()"

[[feature]]
id = "health"                            # also the memory cheat it falls back to
name = "Invincible"
group = "Player"
kind = "toggle"                          # toggle | action | value | list
on = []                                  # once when switched on
tick = ["PLAYER::SET_PLAYER_INVINCIBLE($player, true)"]   # every script tick while on
off = ["PLAYER::SET_PLAYER_INVINCIBLE($player, false)"]
# run = [...] for action / value / list; slider = { min, max, step }; value = default ($value)
# list = "vehicles"; input = true (typed name); hotkey = "Ctrl+F6"; enabled = true; note = "..."
```

A file of your own in `plugins\trainer\features\` (or `trainer.toml` in a trainer data folder)
replaces features by id.

## Statements

One statement per string; `;` separates several; the body of `if` / `each` / `repeat` is the rest
of the string.

| Statement | |
|---|---|
| `NS::NAME(args)`, `NAME(args)`, `0xHASH(args)` | call a native |
| `$var = expr` | assign (`&$var` passes an out parameter) |
| `wait MS` | yield the script |
| `waitfor EXPR, MS` | wait until true or the timeout; sets `$ok` |
| `if EXPR: ...` | condition |
| `each LIST: ...` | loop over a list (`$item`, `$label`, `$hash`, `$pos`, `$x $y $z`, `$heading`, `$i`) |
| `repeat N: ...` | loop (`$i`) |
| `break`, `stop` | |

Expressions: numbers (`0x` hex), `"text"`, `true` / `false`, `$var`, `.x .y .z`, `! -`, `* /`,
`+ -` (text concatenates; vectors add), comparisons, `&& ||`, parentheses. Builtins: `hash(text)`,
`vec(x, y, z)`, `int`, `float`, `abs`, `min`, `max`, `text(...)`, `notify(text)`, `hud(text)` (a
HUD line while the feature is on), `fly(pos, heading)` (the fly controller: W A S D / left stick,
Space / RT up, Ctrl / LT down, Shift / left-stick click fast; speed = `$value`), `fps()`,
`on("id")`, `value("id")`, `mem("location")`, `setmem("location", v)`,
`save_location(pos, heading)`, `fail("message")`.

Toggles' `on` / `tick` / `off` run in one script every tick; each `run` gets a script of its own.
A failing `tick` switches its feature off and says why.

## Memory cheats (`cheats.toml`)

```toml
[[location]]
id = "player_money"
type = "i32"                    # i32 i64 f32 f64 vec3
unknown = "player money: location unknown until the executable is available"
# or: address / module + offset / module + export / module + section + pattern (+ rip) ; chain

[[cheat]]
id = "money"
name = "Unlimited money"
group = "Player"
kind = "freeze"                 # freeze | set | multiply (factor, direction) | patch (bytes)
type = "i32"
value = 999999999
location = "player_money"
```

All shipped locations are `unknown`: find them in your copy (for example with a value scanner) and
put them in `plugins\trainer\cheats.toml` or a trainer data folder's `cheats.toml`.

## Trainer data

Lists come from folders you provide (the repository ships none): `plugins\trainer\data\`,
`data = [...]` in `trainer.toml`, `SHK_TRAINER_DATA` (`;`-separated). Every `.txt` / `.tsv` /
`.csv` / `.json` / `.toml` file is the list named by its file (`vehicles`, `weapons`, `peds`,
`weather` → `weathers`, `locations`): tab-separated rows (`name 0xHASH ...`, hash-first rows, or a
header row naming `name x y z heading`), or JSON / TOML arrays and maps. `features.tsv` there adds
details to the description panel.

## State and automation

`state.toml` in the plugin folder keeps which toggles are on and changed values (applied at the
next start). `SHK_TRAINER_SCRIPT=<file>` (or `autorun.txt` in the plugin folder) runs commands one
step per frame: `wait N`, `feature ID on|off|run|value N|pick NAME|text NAME|expect on|off`,
`cheat ID on|off`, `log TEXT`.
