# Plugin guide

A plugin is an x64 DLL that exports `shk_plugin_load`. `scripthook.dll` loads every plugin in
its `plugins\` folder at start:

```
gta-vi-scripthook\
    scripthook.dll  scripthook.toml  natives.tsv
    plugins\
        my-plugin\
            plugin.toml      plugin = "my_plugin.dll"   (or a mod.toml with the same key)
            my_plugin.dll    + any data files the plugin reads from its folder
        loose.dll            (a plugin without a folder of its own)
```

The examples in `examples\` each do the same three things: show a notice, call natives from a
script function, register a hotkey.

Rules that keep plugins safe: single-player only (do not touch online features, protection or
licence checks); call natives only from script functions; never block in callbacks; everything you
register is removed when you unload, but stop your own threads (better: do not start any).

## C

```c
#include "scripthook.h"

SHK_PLUGIN_INFO("hello-c", "0.1.0", "you", "says hello")

static const ShkHost* host;
static ShkPlugin* me;

static void SHK_CALL main_script(void* user) {
    ShkNative ped_id = host->natives->lookup("PLAYER::PLAYER_PED_ID", NULL);
    for (;;) {
        ShkValue r = {0};
        r.type = SHK_TYPE_INT;
        if (host->natives->call(ped_id, NULL, 0, &r)) { /* r.u.i is the ped */ }
        host->scripts->wait(1000);
    }
}

SHK_EXPORT int SHK_CALL shk_plugin_load(const ShkHost* h, ShkPlugin* self) {
    host = h; me = self;
    h->core->notify(self, "Hello from C", 4.0f);
    if (h->scripts) h->scripts->start(self, "main", main_script, NULL);
    return 0;
}
```

Build it (MSVC; clang-cl takes the same options):

```bat
cl /nologo /O2 /MT /LD /I sdk\include hello.c sdk\c\scripthook_srt2.c /Fe:hello_c.dll
```

`sdk\c\scripthook_srt2.c` is optional: it lets a runtime with Sonoma Runtime's older plugin API
load the plugin directly (docs/host-integration.md). The hook itself only uses `shk_plugin_load`.

Typed values: `ShkValue` holds `type` (`SHK_TYPE_INT`, `FLOAT`, `BOOL`, `HASH`, `STRING`, `VEC3`,
`U64`, `PTR`, `OUT_INT`, `OUT_FLOAT`, `OUT_BOOL`, `OUT_VEC3`) and a 16-byte union. The host converts
values to the types the natives database declares. Out parameters point at your `int32_t`,
`float` or `ShkVec3`. A generated `natives_c.h` gives typed functions (`PLAYER__PLAYER_PED_ID()`;
define `SHK_NATIVES_HOST` or a global `shk_host`).

## C++

```cpp
#include "scripthook.hpp"

SHK_EXPORT int SHK_CALL shk_plugin_load(const ShkHost* host, ShkPlugin* self) {
    shk::init(host, self);
    shk::on_hotkey(VK_F8, SHK_MOD_NONE, [] { shk::notify("F8"); });
    shk::start_script("main", [] {
        for (;;) {
            int ped = shk::invoke<int>("PLAYER::PLAYER_PED_ID");
            shk::Vec3 p = shk::invoke<shk::Vec3>("ENTITY::GET_ENTITY_COORDS", ped, true);
            shk::wait(0);
        }
    });
    return 0;
}
SHK_EXPORT void SHK_CALL shk_plugin_unload() { shk::shutdown(); }
```

- `shk::invoke<R>(name or hash, args...)`: arguments `int32_t`, `bool`, `float`, `shk::Hash`,
  `const char*`, `shk::Vec3`, `uint64_t`, out parameters `int32_t*`, `float*`, `shk::Vec3*`,
  `shk::OutBool{&i}`; results `void`, `int`, `bool`, `float`, `shk::Hash`, `shk::Vec3`,
  `uint64_t`, `std::string`. Returns `R{}` on failure; `shk::Native` gives the details
  (`available()`, `info().reason`, `call()` returning success).
- RAII: `shk::Hook<Fn>` restores its hook, `shk::Script` stops its script function.
- `shk::on_frame`, `on_hotkey`, `on_pad_chord`, `on_draw` (false when the host cannot draw),
  `shk::read<T>` / `write<T>` / `scan`, `shk::menu_service()`.
- A generated `natives.h` gives `natives::PLAYER::PLAYER_PED_ID()` etc.

Compile with `/std:c++17 /EHsc` (and, optionally, the adapter as C: `/Tc sdk\c\scripthook_srt2.c`).

## Rust

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
scripthook-sdk = { path = "../gta-vi-scripthook/sdk/rust" }
```

```rust
use scripthook_sdk::{self as shk, natives::invoke};

struct Mine;

impl shk::Plugin for Mine {
    fn load(host: shk::Host) -> Result<Self, String> {
        host.hotkey(shk::keys::vk_from_name("F9").unwrap(), 0, || shk::host().notify("F9", 2.0));
        host.on_frame(|f| { /* every presented frame: f.dt, f.width, ... */ });
        host.start_script("main", || loop {
            let ped: i32 = invoke("PLAYER::PLAYER_PED_ID", ()).unwrap_or(0);
            if !shk::wait(0) { return; }
        });
        Ok(Mine)
    }
    fn unload(&mut self) {}
}

shk::export_plugin!(Mine, name = "mine", version = "0.1.0", author = "you", description = "...");
```

- `invoke::<R>(name, (args,))` with the same argument and result types as C++ (`&mut i32` etc.
  for out parameters); `Native::lookup` for details; a generated `natives.rs` gives
  `natives::player::player_ped_id()`.
- `Host`: `log`/`info`/`warn`, `notify`, `plugin_folder`, `read`/`write`/`patch`/`scan`/
  `resolve_chain`/`find_export`/`module`, `hook` (unsafe), `on_frame`, `hotkey`, `pad_chord`,
  `key_down`, `pad`, `on_key` / `capture_input` (when offered), `on_draw`, `start_script`,
  `script_status`, `publish` / `find_service`.
- `scripthook_sdk::menu::Menu::connect("")` gives the menu service or the host's basic menu with
  the same calls (`add_page`, `add_item(page, &ItemSpec::toggle(...), |event| ...)`, `set_value`,
  `set_hud`, `toast`).
- `scripthook_sdk::config::{toml, json}` read configuration files; `draw::SoftCanvas` renders the
  host's font to PNG for tests.
- `.cargo/config.toml` links the C runtime statically.

## Menu pages from any plugin

```c
const ShkMenuService* m = host->services ? host->services->find(SHK_MENU_SERVICE) : NULL;
if (m) {
    ShkMenuPage p = m->add_page(self, 0, "My plugin", "What it does");
    ShkMenuItemDesc d = {0};
    d.size = sizeof d; d.kind = SHK_ITEM_TOGGLE; d.label = "Enabled"; d.on_event = on_event;
    m->add_item(self, p, &d);
}
/* in shk_plugin_unload: look it up again and call m->remove_owner(self) */
```

Look the service up at your first frame callback, not in `shk_plugin_load` (load order), and look
it up again when you unload (the menu plugin may already be gone). Strings are copied.

## Testing plugins

- Unit-test logic without a host (the menu's model, navigation and drawing are tested this way,
  with `SoftCanvas` for pixels).
- On the repository's synthetic test target: `tests\tests\standalone.rs` shows how (a natives
  database for the target's handler table, `SHK_AUTOMATION` key presses and frame captures,
  `SHK_TRAINER_SCRIPT`, the target's summary line).
