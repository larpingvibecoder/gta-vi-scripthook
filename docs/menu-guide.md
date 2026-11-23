# Menu guide

The menu is a plugin (`plugins\menu\shk_menu.dll`). It publishes the menu service
`scripthook.menu` that the trainer and other plugins add their pages to, and draws everything
with the hook's overlay.

## Using it

| | Keyboard | Gamepad |
|---|---|---|
| open / close | F4 | RB + D-pad left |
| move | Up / Down (numpad 8 / 2), Page Up / Down, Home / End | D-pad |
| change a value | Left / Right (numpad 4 / 6) | D-pad left / right |
| select, toggle, edit | Enter (numpad 5) | A |
| back (clears a filter first) | Esc / Backspace (numpad 0) | B |
| fine / coarse steps | — | LB (×0.1) / RB (×10) |
| filter a list | type | — |

Sliders and numbers: Enter starts typing a value (Enter confirms, Esc cancels). Text entries:
Enter, type, Enter. A greyed entry says why in the panel below the menu (for example *needs native
STATS::STAT_SET_INT (unknown until the executable)*). While the menu is open the game gets no pad
input and plugin hotkeys are suspended.

The root page lists the plugins' pages, then **Settings** (position left / right, text size,
HUD lines) and **About** (version, host, what this project is).

## `menu.toml`

Next to `shk_menu.dll`, every key optional:

```toml
open_key = "F4"          # any key name: F1..F24, A..Z, 0..9, NUM0..NUM9, INSERT, ...
open_chord = "RB+LEFT"   # pad buttons joined by +; "" = none
numpad = true
position = "left"        # or "right"
scale = 0                # text size: 0 automatic, 1, 2, 3
show_hud = true
mode = "auto"            # "mirror": always use a host's basic menu (where one exists)
```

## The look

A dark panel with a header bar (the project's mark and name, the version), a breadcrumb bar
(page path, position), rows with a teal selection bar, switches for toggles, tracks for sliders,
`< value >` for choices and numbers, boxes for text, a scroll bar for long pages, key hints in the
footer, a description panel (hint or reason), HUD lines at the top right. Text is Inter, drawn
anti-aliased by the hook's renderer. `menu-preview <dir>` renders the pages to PNG with the same
code and font (`docs/images`).

## Adding pages from a plugin

Find the service at your first frame callback (every plugin is loaded by then), add pages and
items, and remove them when you unload:

```rust
use scripthook_sdk::menu::{EventKind, ItemSpec, Menu};

let menu = Menu::connect("");                 // the menu service (or a host's basic menu)
let page = menu.add_page(0, "My plugin", "What it does.");
menu.add_item(page, &ItemSpec::toggle("Enabled", false).hint("Turns it on."), |e| {
    if e.kind == EventKind::Change { /* e.value: 0 or 1 */ }
});
menu.add_item(page, &ItemSpec::slider("Speed", 1.0, 0.0, 5.0, 0.25), |e| { /* e.value */ });
menu.add_item(page, &ItemSpec::choice("Mode", vec!["A".into(), "B".into()], 0), |e| { /* index */ });
let list = menu.add_page(page, "Things", "");
menu.add_item(page, &ItemSpec::subpage("Things", list).filterable(), |_| {});
menu.set_hud(0, "a line at the top right");
// at unload: menu.remove_all();
```

Item kinds: text, separator, button, toggle, slider, number, choice, text input, sub-page; any
item can be disabled with a reason (`.disabled(reason)`), carry a hint, a right-hand text and a
hotkey label. In C, the same through `ShkMenuService` (`scripthook.h`).
